//! Simple Strategy Export System
//!
//! Exports successful strategies with minimal configuration.

use crate::logging_facade::STRATEGY_LOGGER;
use crate::{log_info, log_error};
use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

/// Simple strategy export configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// Minimum Sharpe ratio to qualify for export
    pub min_sharpe_ratio: f64,
    /// Minimum profit factor to qualify for export  
    pub min_profit_factor: f64,
    /// Minimum win rate (%) to qualify for export
    pub min_win_rate: f64,
    /// Export directory path
    pub export_path: String,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            min_sharpe_ratio: 1.5,
            min_profit_factor: 1.3,
            min_win_rate: 55.0,
            export_path: "./exported_strategies".to_string(),
        }
    }
}

/// Strategy performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyMetrics {
    /// Total number of signals generated
    pub total_signals: u64,
    /// Number of buy signals
    pub buy_signals: u64,
    /// Number of sell signals
    pub sell_signals: u64,
    /// Average signal strength
    pub avg_signal_strength: f64,
    /// Strategy uptime
    pub uptime_percentage: f64,
    /// Last signal timestamp
    pub last_signal_time: Option<DateTime<Utc>>,
    /// Strategy-specific metrics
    pub custom_metrics: HashMap<String, f64>,
}

impl Default for StrategyMetrics {
    fn default() -> Self {
        Self {
            total_signals: 0,
            buy_signals: 0,
            sell_signals: 0,
            avg_signal_strength: 0.0,
            uptime_percentage: 0.0,
            last_signal_time: None,
            custom_metrics: HashMap::new(),
        }
    }
}

/// Backtest results for export evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub total_pnl: BigDecimal,
    pub num_trades: u64,
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
    pub signals_generated: u64,
    pub strategy_metrics: StrategyMetrics,
    pub backtest_start: DateTime<Utc>,
    pub backtest_end: DateTime<Utc>,
}

/// Exportable strategy
#[derive(Debug, Clone, Serialize, Deserialize)]  
pub struct ExportableStrategy {
    /// Strategy name
    pub name: String,
    /// Strategy symbol/market
    pub symbol: String,
    /// Strategy exchange
    pub exchange: String,
    /// Strategy parameters
    pub parameters: HashMap<String, f64>,
    /// Backtest performance
    pub performance: BacktestResults,
    /// Export timestamp
    pub exported_at: DateTime<Utc>,
}

/// Simple strategy exporter
pub struct StrategyExporter {
    config: ExportConfig,
}

impl StrategyExporter {
    /// Create new exporter with default config
    pub fn new() -> Self {
        Self {
            config: ExportConfig::default(),
        }
    }

    /// Create new exporter with custom config
    pub fn with_config(config: ExportConfig) -> Self {
        Self { config }
    }

    /// Check if strategy meets export criteria
    pub fn should_export(&self, results: &BacktestResults) -> bool {
        results.sharpe_ratio >= self.config.min_sharpe_ratio
            && results.profit_factor >= self.config.min_profit_factor
            && results.win_rate >= self.config.min_win_rate
    }

    /// Export strategy to JSON file if it meets criteria
    pub async fn export_if_qualified(
        &self,
        strategy_name: &str,
        symbol: &str,
        exchange: &str,
        parameters: HashMap<String, f64>,
        results: BacktestResults,
    ) -> Result<Option<String>> {
        if !self.should_export(&results) {
            return Ok(None);
        }

        let exportable = ExportableStrategy {
            name: strategy_name.to_string(),
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            parameters,
            performance: results,
            exported_at: Utc::now(),
        };

        let filename = format!(
            "{}_{}_{}_exported.json",
            strategy_name,
            symbol,
            exchange,
        );
        let filepath = Path::new(&self.config.export_path).join(&filename);

        // Create directory if it doesn't exist
        if let Some(parent) = filepath.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write strategy to file
        let json = serde_json::to_string_pretty(&exportable)?;
        fs::write(&filepath, json).await?;

        log_info!(STRATEGY_LOGGER, "Strategy exported: {} (Sharpe: {:.2}, PF: {:.2}, Win Rate: {:.1}%)",
            filename, exportable.performance.sharpe_ratio,
            exportable.performance.profit_factor, exportable.performance.win_rate);

        Ok(Some(filepath.to_string_lossy().to_string()))
    }

    /// Export strategy unconditionally (for testing)
    pub async fn force_export(
        &self,
        strategy_name: &str,
        symbol: &str,
        exchange: &str,
        parameters: HashMap<String, f64>,
        results: BacktestResults,
    ) -> Result<String> {
        let exportable = ExportableStrategy {
            name: strategy_name.to_string(),
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            parameters,
            performance: results,
            exported_at: Utc::now(),
        };

        let filename = format!(
            "{}_{}_{}_forced_export.json",
            strategy_name,
            symbol,
            exchange,
        );
        let filepath = Path::new(&self.config.export_path).join(&filename);

        // Create directory if it doesn't exist
        if let Some(parent) = filepath.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write strategy to file
        let json = serde_json::to_string_pretty(&exportable)?;
        fs::write(&filepath, json).await?;

        log_info!(STRATEGY_LOGGER, "Strategy force exported: {}", filename);

        Ok(filepath.to_string_lossy().to_string())
    }

    /// List all exported strategies
    pub async fn list_exported(&self) -> Result<Vec<String>> {
        let export_dir = Path::new(&self.config.export_path);
        if !export_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(export_dir).await?;
        let mut exported_files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            if let Some(filename) = entry.file_name().to_str() {
                if filename.ends_with("_exported.json") || filename.ends_with("_forced_export.json") {
                    exported_files.push(filename.to_string());
                }
            }
        }

        Ok(exported_files)
    }

    /// Load exported strategy from file
    pub async fn load_exported(&self, filename: &str) -> Result<ExportableStrategy> {
        let filepath = Path::new(&self.config.export_path).join(filename);
        let content = fs::read_to_string(&filepath).await?;
        let strategy: ExportableStrategy = serde_json::from_str(&content)?;
        Ok(strategy)
    }

    /// Evaluate strategy performance (alias for should_export)
    pub fn evaluate_strategy(&self, results: &BacktestResults) -> bool {
        self.should_export(results)
    }

    /// Export strategy (wrapper around export_if_qualified)
    pub async fn export_strategy(
        &self,
        strategy_name: &str,
        symbol: &str,
        exchange: &str,
        parameters: HashMap<String, f64>,
        results: BacktestResults,
    ) -> Result<Option<String>> {
        self.export_if_qualified(strategy_name, symbol, exchange, parameters, results).await
    }

    /// List exported strategies (alias for list_exported)
    pub async fn list_exported_strategies(&self) -> Result<Vec<String>> {
        self.list_exported().await
    }

    /// Load exported strategy (alias for load_exported)
    pub async fn load_exported_strategy(&self, filename: &str) -> Result<ExportableStrategy> {
        self.load_exported(filename).await
    }
}

impl Default for StrategyExporter {
    fn default() -> Self {
        Self::new()
    }
}
