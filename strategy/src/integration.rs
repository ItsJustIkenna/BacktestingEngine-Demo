//! Strategy Export Integration System
//!
//! This module provides a comprehensive integration layer between the backtesting engine 
//! and strategy export system to automatically export successful strategies for live trading.
//!
//! ## Features
//! - Automatic export based on performance thresholds
//! - Manual force export capabilities
//! - Export statistics and monitoring
//! - Customizable export configurations
//! - Integration with backtesting workflow
//!
//! ## Usage
//! ```no_run
//! use strategy::integration::BacktestIntegration;
//! # use anyhow::Result;
//! # async fn example() -> Result<()> {
//! 
//! let mut integration = BacktestIntegration::new()?;
//! integration.enable_auto_export(None); // Use default config
//! 
//! // Process backtest results and potentially auto-export
//! // (requires actual backtest data structures)
//! # Ok(())
//! # }
//! ```

use crate::simple_export::{StrategyExporter, ExportConfig, StrategyMetrics, BacktestResults};
use config::{StrategyConfig, ParameterValue};
use bigdecimal::BigDecimal;
use chrono::{Utc, Duration};
use std::collections::HashMap;
use anyhow::Result;

/// Comprehensive integration layer between backtesting engine and strategy export system
/// 
/// This struct manages the automatic export of successful trading strategies based on
/// configurable performance criteria. It bridges the gap between backtesting results
/// and live trading deployment.
pub struct BacktestIntegration {
    exporter: StrategyExporter,
    auto_export_enabled: bool,
    custom_config: Option<ExportConfig>,
}

impl BacktestIntegration {
    /// Create new backtest integration
    pub fn new() -> Result<Self> {
        let exporter = StrategyExporter::new();
        Ok(Self {
            exporter,
            auto_export_enabled: false,
            custom_config: None,
        })
    }

    /// Enable automatic export with optional custom configuration
    pub fn enable_auto_export(&mut self, config: Option<ExportConfig>) {
        self.auto_export_enabled = true;
        self.custom_config = config;
    }

    /// Disable automatic export
    pub fn disable_auto_export(&mut self) {
        self.auto_export_enabled = false;
    }

    /// Process backtest results and potentially export strategy
    pub async fn process_backtest_results(
        &self,
        strategy_config: StrategyConfig,
        backtest_result: BacktestResults,
        _strategy_metrics: StrategyMetrics,
        optimized_parameters: HashMap<String, ParameterValue>,
    ) -> Result<Option<String>> {
        if !self.auto_export_enabled {
            return Ok(None);
        }

        // Use custom config if available, otherwise default
        let exporter = if let Some(config) = &self.custom_config {
            StrategyExporter::with_config(config.clone())
        } else {
            StrategyExporter::new()
        };

        // Convert ParameterValue to f64 for export
        let parameters: HashMap<String, f64> = optimized_parameters.iter()
            .filter_map(|(k, v)| match v {
                ParameterValue::Float(f) => Some((k.clone(), *f)),
                ParameterValue::Int(i) => Some((k.clone(), *i as f64)),
                ParameterValue::Bool(b) => Some((k.clone(), if *b { 1.0 } else { 0.0 })),
                ParameterValue::String(_) => None, // Skip strings for simplicity
                ParameterValue::Array(_) => None, // Skip arrays
            }).collect();

        // Export strategy if it meets criteria
        let export_path = exporter.export_if_qualified(
            &strategy_config.name,
            &strategy_config.symbol,
            &strategy_config.exchange,
            parameters,
            backtest_result,
        ).await?;

        if let Some(path) = &export_path {
            println!("✅ Strategy auto-exported to: {}", path);
        }

        Ok(export_path)
    }

    /// Force export a strategy regardless of criteria
    pub async fn force_export_strategy(
        &self,
        strategy_config: StrategyConfig,
        backtest_result: BacktestResults,
        optimized_parameters: HashMap<String, ParameterValue>,
    ) -> Result<String> {
        let exporter = if let Some(config) = &self.custom_config {
            StrategyExporter::with_config(config.clone())
        } else {
            StrategyExporter::new()
        };

        // Convert ParameterValue to f64 for export
        let parameters: HashMap<String, f64> = optimized_parameters.iter()
            .filter_map(|(k, v)| match v {
                ParameterValue::Float(f) => Some((k.clone(), *f)),
                ParameterValue::Int(i) => Some((k.clone(), *i as f64)),
                ParameterValue::Bool(b) => Some((k.clone(), if *b { 1.0 } else { 0.0 })),
                ParameterValue::String(_) => None,
                ParameterValue::Array(_) => None, // Skip arrays
            }).collect();

        let export_path = exporter.force_export(
            &strategy_config.name,
            &strategy_config.symbol,
            &strategy_config.exchange,
            parameters,
            backtest_result,
        ).await?;

        println!("📤 Strategy force exported to: {}", export_path);
        Ok(export_path)
    }

    /// Get list of exported strategies
    pub async fn get_exported_strategies(&self) -> Result<Vec<String>> {
        self.exporter.list_exported().await
    }

    /// Get simple export statistics
    pub async fn get_export_statistics(&self) -> Result<ExportStatistics> {
        let exported_strategies = self.get_exported_strategies().await?;
        Ok(ExportStatistics {
            total_exported: exported_strategies.len(),
            strategies_exported: exported_strategies,
        })
    }

    /// Check if a strategy would meet export criteria without actually exporting
    pub fn would_export(&self, backtest_result: &BacktestResults) -> bool {
        let exporter = if let Some(config) = &self.custom_config {
            StrategyExporter::with_config(config.clone())
        } else {
            StrategyExporter::new()
        };
        exporter.should_export(backtest_result)
    }

    /// Get current export configuration
    pub fn get_export_config(&self) -> ExportConfig {
        self.custom_config.clone().unwrap_or_default()
    }

    /// Update export configuration
    pub fn set_export_config(&mut self, config: ExportConfig) {
        self.custom_config = Some(config);
    }

    /// Get auto-export status
    pub fn is_auto_export_enabled(&self) -> bool {
        self.auto_export_enabled
    }
}

impl Default for BacktestIntegration {
    fn default() -> Self {
        Self::new().expect("Failed to create BacktestIntegration")
    }
}

/// Export statistics for monitoring and analytics
#[derive(Debug, Default)]
pub struct ExportStatistics {
    /// Total number of exported strategies
    pub total_exported: usize,
    /// List of exported strategy filenames
    pub strategies_exported: Vec<String>,
}

impl ExportStatistics {
    /// Get statistics summary as formatted string
    pub fn summary(&self) -> String {
        format!(
            "Export Statistics:\n  Total Exported: {}\n  Strategies: [{}]",
            self.total_exported,
            self.strategies_exported.join(", ")
        )
    }
    
    /// Check if any strategies have been exported
    pub fn has_exports(&self) -> bool {
        self.total_exported > 0
    }
}

/// Simple integration example
pub async fn simple_integration_example() -> Result<()> {
    // Initialize the integration
    let mut integration = BacktestIntegration::new()?;

    // Enable auto export with default config
    integration.enable_auto_export(None);

    // Create example strategy configuration
    let mut strategy_config = StrategyConfig::default();
    strategy_config.name = "SimpleMA".to_string();
    strategy_config.symbol = "BTCUSDT".to_string();
    strategy_config.exchange = "binance".to_string();
    strategy_config.parameters.insert(
        "fast_period".to_string(), 
        ParameterValue::Float(12.0)
    );
    strategy_config.parameters.insert(
        "slow_period".to_string(), 
        ParameterValue::Float(26.0)
    );

    // Example backtest results (successful strategy)
    let backtest_result = BacktestResults {
        total_pnl: BigDecimal::from(5000),
        num_trades: 250,
        max_drawdown: BigDecimal::from(300),
        sharpe_ratio: 2.1,
        profit_factor: 1.6,
        win_rate: 62.0,
        avg_trade_return: 0.0025,
        median_trade_return: 0.002,
        avg_trade_duration_hours: 1.5,
        volatility: 0.28,
        gross_profit: 8000.0,
        gross_loss: 3000.0,
        net_profit: 5000.0,
        signals_generated: 280,
        strategy_metrics: StrategyMetrics::default(),
        backtest_start: Utc::now() - Duration::days(90),
        backtest_end: Utc::now(),
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
        
        // List exported strategies
        let exported = integration.get_exported_strategies().await?;
        println!("Exported strategies: {:?}", exported);
    } else {
        println!("Strategy did not meet export criteria");
    }

    Ok(())
}
