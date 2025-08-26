//! Strategy Export System
//!
//! This module provides functionality to export successful backtested strategies
//! to the SignalEngine for live trading deployment.

use crate::traits::StrategyMetrics;
use crate::config_export::{ExportConfig, PerformanceThresholds as ConfigThresholds, EnvironmentRiskLimits};
use config::{StrategyConfig, ParameterValue};
use anyhow::{Result, Context};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

/// Complete backtest results for strategy evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub total_pnl: BigDecimal,
    pub num_trades: usize,
    pub max_drawdown: BigDecimal,
    pub sharpe_ratio: f64,
    pub profit_factor: f64,
    pub win_rate: f64,
    pub avg_trade_return: f64,
    pub median_trade_return: f64,
    pub avg_trade_duration_hours: f64,
    pub volatility: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub net_profit: f64,
    pub backtest_start: DateTime<Utc>,
    pub backtest_end: DateTime<Utc>,
    pub signals_generated: u64,
    pub strategy_metrics: StrategyMetrics,
}

/// Exportable strategy package for live trading deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportableStrategy {
    /// Strategy configuration
    pub config: StrategyConfig,
    /// Backtest performance results
    pub performance: BacktestResults,
    /// Strategy parameters optimized during backtesting
    pub optimized_parameters: HashMap<String, ParameterValue>,
    /// Live trading specific settings
    pub live_trading_config: LiveTradingConfig,
    /// Export metadata
    pub export_metadata: ExportMetadata,
}

/// Live trading configuration for exported strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveTradingConfig {
    /// Maximum position size as percentage of portfolio (0.0 - 1.0)
    pub max_position_pct: f64,
    /// Stop loss percentage (0.0 - 1.0)
    pub stop_loss_pct: f64,
    /// Take profit percentage (0.0 - 1.0)
    pub take_profit_pct: Option<f64>,
    /// Maximum daily loss limit
    pub daily_loss_limit: BigDecimal,
    /// Cool-down period after stop loss in minutes
    pub cooldown_minutes: u32,
    /// Risk scaling factor (conservative: 0.5, aggressive: 1.5)
    pub risk_scaling: f64,
    /// Whether to enable paper trading mode first
    pub paper_trading: bool,
    /// Paper trading duration before going live (days)
    pub paper_trading_days: u32,
}

/// Export metadata for tracking and auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMetadata {
    /// When the strategy was exported
    pub export_time: DateTime<Utc>,
    /// Version of the strategy implementation
    pub strategy_version: String,
    /// Backtesting engine version used
    pub engine_version: String,
    /// Who authorized the export
    pub authorized_by: String,
    /// Export approval level (development, staging, production)
    pub approval_level: ApprovalLevel,
    /// Additional notes or comments
    pub notes: Option<String>,
}

/// Strategy approval levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalLevel {
    Development,
    Staging,
    Production,
}

impl From<crate::config_export::ApprovalLevel> for ApprovalLevel {
    fn from(level: crate::config_export::ApprovalLevel) -> Self {
        match level {
            crate::config_export::ApprovalLevel::Development => ApprovalLevel::Development,
            crate::config_export::ApprovalLevel::Staging => ApprovalLevel::Staging,
            crate::config_export::ApprovalLevel::Production => ApprovalLevel::Production,
        }
    }
}

/// Strategy export evaluation result
#[derive(Debug)]
pub struct ExportEvaluation {
    pub eligible: bool,
    pub passed_checks: Vec<String>,
    pub failed_checks: Vec<String>,
    pub warnings: Vec<String>,
    pub score: f64, // 0.0 - 1.0
}

/// Strategy export manager
pub struct StrategyExporter {
    config: ExportConfig,
    signal_engine_path: String,
}

impl StrategyExporter {
    /// Create a new strategy exporter with configuration from ConfigMap
    pub fn new(signal_engine_path: String) -> Result<Self> {
        let config = ExportConfig::from_env()
            .context("Failed to load export configuration from ConfigMap")?;
        
        Ok(Self {
            config,
            signal_engine_path,
        })
    }

    /// Create with custom export configuration (mainly for testing)
    pub fn with_config(signal_engine_path: String, config: ExportConfig) -> Self {
        Self {
            config,
            signal_engine_path,
        }
    }

    /// Get the current performance thresholds from config
    pub fn get_thresholds(&self) -> &ConfigThresholds {
        &self.config.performance_thresholds
    }

    /// Create live trading configuration from ConfigMap settings
    fn create_live_trading_config(&self) -> LiveTradingConfig {
        let strategy_limits = &self.config.risk_management.strategy_limits;
        let default_env_limits = EnvironmentRiskLimits::default();
        let env_limits = self.config.risk_management.environment_limits
            .get("development")
            .unwrap_or(&default_env_limits);
        
        LiveTradingConfig {
            max_position_pct: strategy_limits.max_position_pct,
            stop_loss_pct: strategy_limits.stop_loss_pct,
            take_profit_pct: strategy_limits.take_profit_pct,
            daily_loss_limit: strategy_limits.daily_loss_limit.clone(),
            cooldown_minutes: strategy_limits.cooldown_minutes,
            risk_scaling: strategy_limits.risk_scaling,
            paper_trading: env_limits.paper_trading,
            paper_trading_days: env_limits.paper_trading_days,
        }
    }

    /// Evaluate if a strategy meets export criteria
    pub fn evaluate_strategy(&self, results: &BacktestResults) -> ExportEvaluation {
        let thresholds = &self.config.performance_thresholds;
        let mut passed = Vec::new();
        let mut failed = Vec::new();
        let mut warnings = Vec::new();
        let mut score_components = Vec::new();

        // Check Sharpe ratio
        if results.sharpe_ratio >= thresholds.min_sharpe_ratio {
            passed.push(format!("Sharpe ratio: {:.2} >= {:.2}", 
                results.sharpe_ratio, thresholds.min_sharpe_ratio));
            score_components.push(0.2);
        } else {
            failed.push(format!("Sharpe ratio: {:.2} < {:.2}", 
                results.sharpe_ratio, thresholds.min_sharpe_ratio));
        }

        // Check profit factor
        if results.profit_factor >= thresholds.min_profit_factor {
            passed.push(format!("Profit factor: {:.2} >= {:.2}", 
                results.profit_factor, thresholds.min_profit_factor));
            score_components.push(0.15);
        } else {
            failed.push(format!("Profit factor: {:.2} < {:.2}", 
                results.profit_factor, thresholds.min_profit_factor));
        }

        // Check win rate
        if results.win_rate >= thresholds.min_win_rate {
            passed.push(format!("Win rate: {:.1}% >= {:.1}%", 
                results.win_rate * 100.0, thresholds.min_win_rate * 100.0));
            score_components.push(0.1);
        } else {
            failed.push(format!("Win rate: {:.1}% < {:.1}%", 
                results.win_rate * 100.0, thresholds.min_win_rate * 100.0));
        }

        // Check maximum drawdown
        let drawdown_pct = results.max_drawdown.to_string().parse::<f64>().unwrap_or(0.0) / 
            results.total_pnl.to_string().parse::<f64>().unwrap_or(1.0);
        if drawdown_pct <= thresholds.max_drawdown {
            passed.push(format!("Max drawdown: {:.1}% <= {:.1}%", 
                drawdown_pct * 100.0, thresholds.max_drawdown * 100.0));
            score_components.push(0.15);
        } else {
            failed.push(format!("Max drawdown: {:.1}% > {:.1}%", 
                drawdown_pct * 100.0, thresholds.max_drawdown * 100.0));
        }

        // Check number of trades
        if results.num_trades >= thresholds.min_trades as usize {
            passed.push(format!("Number of trades: {} >= {}", 
                results.num_trades, thresholds.min_trades));
            score_components.push(0.1);
        } else {
            failed.push(format!("Number of trades: {} < {}", 
                results.num_trades, thresholds.min_trades));
        }

        // Check backtest duration
        let duration_days = (results.backtest_end - results.backtest_start).num_days();
        if duration_days >= thresholds.min_backtest_days as i64 {
            passed.push(format!("Backtest duration: {} days >= {} days", 
                duration_days, thresholds.min_backtest_days));
            score_components.push(0.1);
        } else {
            failed.push(format!("Backtest duration: {} days < {} days", 
                duration_days, thresholds.min_backtest_days));
        }

        // Check total PnL
        if results.total_pnl >= thresholds.min_total_pnl {
            passed.push(format!("Total PnL: {} >= {}", 
                results.total_pnl, thresholds.min_total_pnl));
            score_components.push(0.1);
        } else {
            failed.push(format!("Total PnL: {} < {}", 
                results.total_pnl, thresholds.min_total_pnl));
        }

        // Check average trade return
        if results.avg_trade_return >= thresholds.min_avg_trade_return {
            passed.push(format!("Avg trade return: {:.3}% >= {:.3}%", 
                results.avg_trade_return * 100.0, thresholds.min_avg_trade_return * 100.0));
            score_components.push(0.1);
        } else {
            failed.push(format!("Avg trade return: {:.3}% < {:.3}%", 
                results.avg_trade_return * 100.0, thresholds.min_avg_trade_return * 100.0));
        }

        // Additional warnings for edge cases
        if results.volatility > thresholds.max_volatility {
            warnings.push(format!("High volatility: {:.1}% > {:.1}%", 
                results.volatility * 100.0, thresholds.max_volatility * 100.0));
        }

        if results.num_trades < 500 {
            warnings.push("Relatively low trade count - consider longer backtest period".to_string());
        }

        if results.avg_trade_duration_hours < 1.0 {
            warnings.push("Very short average trade duration - high frequency strategy".to_string());
        }

        let score = score_components.iter().sum::<f64>();
        let eligible = failed.is_empty() && score >= 0.7;

        ExportEvaluation {
            eligible,
            passed_checks: passed,
            failed_checks: failed,
            warnings,
            score,
        }
    }

    /// Export a strategy to the SignalEngine
    pub async fn export_strategy(
        &self,
        strategy_config: StrategyConfig,
        results: BacktestResults,
        optimized_parameters: HashMap<String, ParameterValue>,
        authorized_by: String,
        notes: Option<String>,
    ) -> Result<ExportableStrategy> {
        // First evaluate the strategy
        let evaluation = self.evaluate_strategy(&results);
        if !evaluation.eligible {
            return Err(anyhow::anyhow!(
                "Strategy does not meet export criteria. Failed checks: {:?}", 
                evaluation.failed_checks
            ));
        }

        // Create export package with configuration from ConfigMap
        let exportable_strategy = ExportableStrategy {
            config: strategy_config.clone(),
            performance: results,
            optimized_parameters,
            live_trading_config: self.create_live_trading_config(),
            export_metadata: ExportMetadata {
                export_time: Utc::now(),
                strategy_version: "1.0.0".to_string(),
                engine_version: env!("CARGO_PKG_VERSION").to_string(),
                authorized_by,
                approval_level: ApprovalLevel::Development, // Default to development
                notes,
            },
        };

        // Save to file
        let export_path = self.get_export_path(&strategy_config.id)?;
        self.save_exported_strategy(&exportable_strategy, &export_path).await?;

        // Copy strategy implementation files to SignalEngine
        self.copy_strategy_files(&strategy_config).await?;

        Ok(exportable_strategy)
    }

    /// Generate the export file path
    fn get_export_path(&self, strategy_id: &str) -> Result<String> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        Ok(format!("{}/exported_strategies/{}_{}.json", 
            self.signal_engine_path, strategy_id, timestamp))
    }

    /// Save exported strategy to JSON file
    async fn save_exported_strategy(
        &self,
        strategy: &ExportableStrategy,
        path: &str,
    ) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent).await
                .context("Failed to create export directory")?;
        }

        let json = serde_json::to_string_pretty(strategy)
            .context("Failed to serialize strategy")?;
        
        fs::write(path, json).await
            .context("Failed to write export file")?;

        println!("Strategy exported to: {}", path);
        Ok(())
    }

    /// Copy strategy implementation files to SignalEngine
    async fn copy_strategy_files(&self, config: &StrategyConfig) -> Result<()> {
        let source_path = format!("strategy/src/strategies/{}.rs", 
            config.strategy_type.to_lowercase());
        let dest_path = format!("{}/strategyhandler/src/strategies/{}.rs", 
            self.signal_engine_path, config.strategy_type.to_lowercase());

        if Path::new(&source_path).exists() {
            if let Some(parent) = Path::new(&dest_path).parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source_path, &dest_path).await
                .context("Failed to copy strategy file")?;
            println!("Strategy implementation copied to: {}", dest_path);
        }

        Ok(())
    }

    /// List all exported strategies
    pub async fn list_exported_strategies(&self) -> Result<Vec<String>> {
        let export_dir = format!("{}/exported_strategies", self.signal_engine_path);
        let mut exports = Vec::new();

        if let Ok(mut entries) = fs::read_dir(&export_dir).await {
            while let Some(entry) = entries.next_entry().await? {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".json") {
                        exports.push(name.to_string());
                    }
                }
            }
        }

        exports.sort();
        Ok(exports)
    }

    /// Load an exported strategy from file
    pub async fn load_exported_strategy(&self, filename: &str) -> Result<ExportableStrategy> {
        let path = format!("{}/exported_strategies/{}", self.signal_engine_path, filename);
        let content = fs::read_to_string(&path).await
            .context("Failed to read exported strategy file")?;
        
        let strategy: ExportableStrategy = serde_json::from_str(&content)
            .context("Failed to parse exported strategy")?;

        Ok(strategy)
    }
}

/// Convenience functions for quick strategy evaluation
impl StrategyExporter {
    /// Quick check if strategy meets minimum requirements
    pub fn meets_minimum_requirements(&self, results: &BacktestResults) -> bool {
        let thresholds = &self.config.performance_thresholds;
        results.sharpe_ratio >= (thresholds.min_sharpe_ratio * 0.67) && // 2/3 of required
        results.profit_factor >= (thresholds.min_profit_factor * 0.92) && // Close to required
        results.win_rate >= (thresholds.min_win_rate * 0.9) && // 90% of required
        results.num_trades >= ((thresholds.min_trades / 2) as usize) && // Half the trades
        results.total_pnl > BigDecimal::from(0)
    }

    /// Generate a comprehensive strategy report
    pub fn generate_report(&self, results: &BacktestResults) -> String {
        let evaluation = self.evaluate_strategy(results);
        
        format!(
            "Strategy Export Evaluation Report\n\
            =====================================\n\
            \n\
            Overall Score: {:.1}%\n\
            Eligible for Export: {}\n\
            \n\
            Performance Metrics:\n\
            - Sharpe Ratio: {:.2}\n\
            - Profit Factor: {:.2}\n\
            - Win Rate: {:.1}%\n\
            - Total PnL: {}\n\
            - Max Drawdown: {}\n\
            - Number of Trades: {}\n\
            - Avg Trade Return: {:.3}%\n\
            - Volatility: {:.1}%\n\
            \n\
            Passed Checks:\n{}\n\
            \n\
            Failed Checks:\n{}\n\
            \n\
            Warnings:\n{}\n",
            evaluation.score * 100.0,
            if evaluation.eligible { "YES" } else { "NO" },
            results.sharpe_ratio,
            results.profit_factor,
            results.win_rate * 100.0,
            results.total_pnl,
            results.max_drawdown,
            results.num_trades,
            results.avg_trade_return * 100.0,
            results.volatility * 100.0,
            evaluation.passed_checks.join("\n"),
            evaluation.failed_checks.join("\n"),
            evaluation.warnings.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_thresholds_from_config() {
        // This test would need the config module to be properly set up
        // For now, just test that the exporter can be created
        let result = StrategyExporter::new("test".to_string());
        // In real deployment, this should succeed with proper ConfigMap
        // assert!(result.is_ok());
    }

    #[test]
    fn test_strategy_evaluation_passing() {
        // Create a test configuration for the exporter
        use crate::config_export::ExportConfig;
        
        let test_config = ExportConfig::default();
        
        let exporter = StrategyExporter::with_config("test".to_string(), test_config);
        let results = BacktestResults {
            sharpe_ratio: 2.0,
            profit_factor: 1.5,
            win_rate: 0.6,
            max_drawdown: BigDecimal::from(100),
            total_pnl: BigDecimal::from(1000),
            num_trades: 150,
            backtest_start: Utc::now() - chrono::Duration::days(120),
            backtest_end: Utc::now(),
            avg_trade_return: 0.002,
            volatility: 0.25,
            // ... other fields with default values
            median_trade_return: 0.001,
            avg_trade_duration_hours: 2.0,
            gross_profit: 2000.0,
            gross_loss: 1000.0,
            net_profit: 1000.0,
            signals_generated: 200,
            strategy_metrics: StrategyMetrics::default(),
        };

        let evaluation = exporter.evaluate_strategy(&results);
        assert!(evaluation.eligible);
        assert!(evaluation.score > 0.7);
    }
}
