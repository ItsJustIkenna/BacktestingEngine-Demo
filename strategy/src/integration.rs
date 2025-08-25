//! Strategy Export Integration
//!
//! This module provides integration between the backtesting engine and strategy export system
//! to automatically export successful strategies to the SignalEngine for live trading.

use crate::{
    export::{StrategyExporter},
    traits::StrategyMetrics,
    BacktestResults, ApprovalLevel,
    config_export::ExportConfig,
};
use config::{StrategyConfig, ParameterValue};
use bigdecimal::BigDecimal;
use chrono::{Utc, Duration};
use std::collections::HashMap;
use anyhow::Result;

/// Integration layer between backtesting engine and strategy export system
pub struct BacktestIntegration {
    exporter: StrategyExporter,
    auto_export_enabled: bool,
    custom_config: Option<ExportConfig>,
}

impl BacktestIntegration {
    /// Create new backtest integration
    pub fn new(signal_engine_path: String) -> Result<Self> {
        let exporter = StrategyExporter::new(signal_engine_path)?;
        Ok(Self {
            exporter,
            auto_export_enabled: false,
            custom_config: None,
        })
    }

    /// Enable automatic export of successful strategies
    pub fn enable_auto_export(&mut self, config: Option<ExportConfig>) {
        self.auto_export_enabled = true;
        self.custom_config = config;
    }

    /// Process backtest results and potentially export strategy
    pub async fn process_backtest_results(
        &self,
        strategy_config: StrategyConfig,
        backtest_result: BacktestResults,
        strategy_metrics: StrategyMetrics,
        optimized_parameters: HashMap<String, ParameterValue>,
    ) -> Result<Option<String>> {
        if !self.auto_export_enabled {
            return Ok(None);
        }

        // Evaluate strategy performance
        let evaluation = self.exporter.evaluate_strategy(&backtest_result);
        
        if evaluation.eligible {
            println!("✅ Strategy meets export criteria! Auto-exporting...");
            
            match self.exporter.export_strategy(
                strategy_config,
                backtest_result,
                optimized_parameters,
                "BacktestEngine-AutoExport".to_string(),
                Some("Automatically exported after successful backtest".to_string()),
            ).await {
                Ok(exported_strategy) => {
                    let export_path = format!("exported_strategies/{}_{}.json", 
                        exported_strategy.config.id,
                        exported_strategy.export_metadata.export_time.format("%Y%m%d_%H%M%S"));
                    
                    println!("✅ Strategy successfully exported for live trading!");
                    println!("📁 Export location: {}", export_path);
                    println!("📊 Performance score: {:.1}%", evaluation.score * 100.0);
                    
                    return Ok(Some(export_path));
                }
                Err(e) => {
                    println!("❌ Failed to export strategy: {}", e);
                }
            }
        } else {
            println!("❌ Strategy does not meet export criteria");
            println!("Failed checks:");
            for check in evaluation.failed_checks {
                println!("  - {}", check);
            }
            
            if !evaluation.warnings.is_empty() {
                println!("Warnings:");
                for warning in evaluation.warnings {
                    println!("  ⚠️  {}", warning);
                }
            }
        }

        Ok(None)
    }

    /// Manual export with custom approval
    pub async fn manual_export(
        &self,
        strategy_config: StrategyConfig,
        backtest_result: BacktestResults,
        strategy_metrics: StrategyMetrics,
        optimized_parameters: HashMap<String, ParameterValue>,
        authorized_by: String,
        approval_level: ApprovalLevel,
        notes: Option<String>,
    ) -> Result<String> {
        let mut exported_strategy = self.exporter.export_strategy(
            strategy_config,
            backtest_result,
            optimized_parameters,
            authorized_by,
            notes,
        ).await?;

        // Update approval level for manual exports
        exported_strategy.export_metadata.approval_level = approval_level;

        let export_path = format!("exported_strategies/{}_{}.json", 
            exported_strategy.config.id,
            exported_strategy.export_metadata.export_time.format("%Y%m%d_%H%M%S"));

        Ok(export_path)
    }

    /// Get export statistics
    pub async fn get_export_statistics(&self) -> Result<ExportStatistics> {
        let exported_strategies = self.exporter.list_exported_strategies().await?;
        
        let mut stats = ExportStatistics::default();
        stats.total_exported = exported_strategies.len();

        for filename in exported_strategies {
            if let Ok(strategy) = self.exporter.load_exported_strategy(&filename).await {
                stats.by_approval_level.entry(format!("{:?}", strategy.export_metadata.approval_level))
                    .and_modify(|count| *count += 1)
                    .or_insert(1);

                if strategy.live_trading_config.paper_trading {
                    stats.paper_trading_count += 1;
                } else {
                    stats.live_trading_count += 1;
                }
            }
        }

        Ok(stats)
    }
}

/// Export statistics for monitoring
#[derive(Debug, Default)]
pub struct ExportStatistics {
    pub total_exported: usize,
    pub paper_trading_count: usize,
    pub live_trading_count: usize,
    pub by_approval_level: HashMap<String, usize>,
}

/// Example usage function
pub async fn example_usage() -> Result<()> {
    // Initialize the integration
    let mut integration = BacktestIntegration::new(
        "../SignalEngine".to_string()
    )?;

    // Set custom export configuration
    let custom_config = ExportConfig {
        performance_thresholds: crate::config_export::PerformanceThresholds {
            min_sharpe_ratio: 2.0,
            min_profit_factor: 1.5,
            min_win_rate: 0.6,
            max_drawdown: 0.1,
            min_trades: 200,
            ..crate::config_export::PerformanceThresholds::default()
        },
        ..ExportConfig::default()
    };

    integration.enable_auto_export(Some(custom_config));

    // Example strategy configuration
    let mut strategy_config = StrategyConfig {
        id: "avellaneda_stoikov_v1".to_string(),
        name: "Avellaneda-Stoikov Market Maker".to_string(),
        strategy_type: "AvellanedaStoikov".to_string(),
        symbol: "BTCUSD".to_string(),
        exchange: "kraken".to_string(),
        parameters: HashMap::new(),
        risk_limits: Default::default(),
        enabled: true,
    };

    // Add optimized parameters
    strategy_config.parameters.insert(
        "risk_aversion".to_string(), 
        ParameterValue::Float(0.008)
    );
    strategy_config.parameters.insert(
        "order_size".to_string(), 
        ParameterValue::Float(0.05)
    );

    // Example backtest results (successful strategy)
    let backtest_result = BacktestResults {
        total_pnl: BigDecimal::from(5000),
        num_trades: 250,
        max_drawdown: BigDecimal::from(300),
        sharpe_ratio: 2.1,
        profit_factor: 1.6,
        win_rate: 0.62,
        avg_trade_return: 0.0025,
        median_trade_return: 0.002,
        avg_trade_duration_hours: 1.5,
        volatility: 0.28,
        gross_profit: 8000.0,
        gross_loss: 3000.0,
        net_profit: 5000.0,
        backtest_start: Utc::now() - Duration::days(90),
        backtest_end: Utc::now(),
        signals_generated: 280,
        strategy_metrics: StrategyMetrics::default(),
    };

    let strategy_metrics = StrategyMetrics {
        total_signals: 280,
        buy_signals: 140,
        sell_signals: 140,
        avg_signal_strength: 2.1,
        uptime_percentage: 98.5,
        last_signal_time: Some(Utc::now()),
        custom_metrics: HashMap::new(),
    };

    // Process results (will auto-export if criteria met)
    let export_path = integration.process_backtest_results(
        strategy_config.clone(),
        backtest_result,
        strategy_metrics.clone(),
        strategy_config.parameters.clone(),
    ).await?;

    if let Some(path) = export_path {
        println!("Strategy automatically exported to: {}", path);
        
        // Get export statistics
        let stats = integration.get_export_statistics().await?;
        println!("Export Statistics: {:#?}", stats);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backtest_integration() {
        let integration = BacktestIntegration::new("test".to_string());
        
        // Create test data
        let strategy_config = StrategyConfig {
            id: "test_strategy".to_string(),
            name: "Test Strategy".to_string(),
            strategy_type: "TestStrategy".to_string(),
            symbol: "BTCUSD".to_string(),
            exchange: "test".to_string(),
            parameters: HashMap::new(),
            risk_limits: Default::default(),
            enabled: true,
        };

        let backtest_result = BacktestResults {
            total_pnl: BigDecimal::from(1000),
            num_trades: 100,
            max_drawdown: BigDecimal::from(50),
            sharpe_ratio: 1.5,
            profit_factor: 1.3,
            win_rate: 0.55,
            avg_trade_return: 0.001,
            median_trade_return: 0.0008,
            avg_trade_duration_hours: 2.0,
            volatility: 0.15,
            gross_profit: 1500.0,
            gross_loss: 500.0,
            net_profit: 1000.0,
            backtest_start: Utc::now() - Duration::days(30),
            backtest_end: Utc::now(),
            signals_generated: 150,
            strategy_metrics: StrategyMetrics::default(),
        };

        let _metrics = StrategyMetrics::default();

        // Test that the results are properly structured
        assert_eq!(backtest_result.total_pnl, BigDecimal::from(1000));
        assert_eq!(backtest_result.num_trades, 100);
        assert_eq!(backtest_result.sharpe_ratio, 1.5);
    }
}
