//! Trading Strategy Module
//!
//! This module provides a comprehensive framework for implementing and managing
//! trading strategies with support for technical analysis, market data processing,
//! signal generation, and strategy export to live trading systems.

pub mod traits;
pub mod signal;
pub mod indicators;
pub mod strategies;
pub mod export;
pub mod integration;
pub mod config_export;

// Re-export main types for convenience
pub use traits::{Strategy, StrategyContext, StrategyError, StrategyResult, StrategyMetrics};
pub use signal::{Signal, SignalType, SignalStrength, SignalReason};
pub use indicators::{IndicatorValue};
pub use export::{
    StrategyExporter, BacktestResults, ExportableStrategy,
    LiveTradingConfig, ExportEvaluation, ApprovalLevel,
};
pub use integration::{BacktestIntegration, ExportStatistics};
pub use config_export::{ExportConfig, PerformanceThresholds};
pub use config::{ParameterValue, StrategyConfig};
pub use riskmanager::{RiskManager, RiskMetrics};

use anyhow::Result;
use std::collections::HashMap;

/// Strategy manager that coordinates multiple strategies
#[derive(Debug)]
pub struct StrategyManager {
    /// Active strategies
    strategies: HashMap<String, Box<dyn Strategy>>,
    /// Strategy configurations
    configs: HashMap<String, StrategyConfig>,
    /// Global risk manager
    risk_manager: RiskManager,
}

impl StrategyManager {
    /// Create a new strategy manager
    pub fn new() -> Self {
        Self {
            strategies: HashMap::new(),
            configs: HashMap::new(),
            risk_manager: RiskManager::new(),
        }
    }

    /// Register a new strategy
    pub fn register_strategy<S>(&mut self, config: StrategyConfig, strategy: S) -> Result<()>
    where
        S: Strategy + 'static,
    {
        let id = config.id.clone();
        self.configs.insert(id.clone(), config);
        self.strategies.insert(id, Box::new(strategy));
        Ok(())
    }

    /// Remove a strategy
    pub fn unregister_strategy(&mut self, strategy_id: &str) -> Result<()> {
        self.strategies.remove(strategy_id);
        self.configs.remove(strategy_id);
        Ok(())
    }

    /// Get a reference to a strategy
    pub fn get_strategy(&self, strategy_id: &str) -> Option<&dyn Strategy> {
        self.strategies.get(strategy_id).map(|s| s.as_ref())
    }

    /// Get a mutable reference to a strategy


    /// Apply a closure to a mutable reference to a strategy, avoiding trait object lifetime issues
    pub fn with_strategy_mut<F, R>(&mut self, strategy_id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn Strategy) -> R,
    {
        self.strategies.get_mut(strategy_id).map(|s| f(s.as_mut()))
    }

    /// Get strategy configuration
    pub fn get_config(&self, strategy_id: &str) -> Option<&StrategyConfig> {
        self.configs.get(strategy_id)
    }

    /// List all registered strategy IDs
    pub fn list_strategies(&self) -> Vec<&str> {
        self.strategies.keys().map(|s| s.as_str()).collect()
    }

    /// Process market data for all enabled strategies
    pub async fn process_market_data(
        &mut self,
        market_data: &dataloader::MarketData,
        context: &StrategyContext,
    ) -> Result<Vec<(String, Vec<Signal>)>> {
        let mut all_signals = Vec::new();

        for (strategy_id, strategy) in &mut self.strategies {
            if let Some(config) = self.configs.get(strategy_id) {
                if !config.enabled {
                    continue;
                }

                // Check if this strategy applies to this market data
                match market_data {
                    dataloader::MarketData::Order(order) => {
                        if order.get_symbol() != &config.symbol || 
                           order.get_exchange() != &config.exchange {
                            continue;
                        }
                    }
                    dataloader::MarketData::Snapshot(snapshot) => {
                        if snapshot.get_symbol() != &config.symbol || 
                           snapshot.get_exchange() != &config.exchange {
                            continue;
                        }
                    }
                    _ => {}
                }

                // Generate signals
                match strategy.generate_signals(market_data, context).await {
                    Ok(signals) => {
                        if !signals.is_empty() {
                            all_signals.push((strategy_id.clone(), signals));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Strategy {} failed to generate signals: {}", strategy_id, e);
                    }
                }
            }
        }

        Ok(all_signals)
    }

    /// Get the global risk manager
    pub fn risk_manager(&self) -> &RiskManager {
        &self.risk_manager
    }

    /// Get mutable reference to risk manager
    pub fn risk_manager_mut(&mut self) -> &mut RiskManager {
        &mut self.risk_manager
    }
}

impl Default for StrategyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Utility functions for strategy development
pub mod utils {

    /// Calculate simple moving average
    pub fn sma(values: &[f64], period: usize) -> Option<f64> {
        if values.len() < period {
            return None;
        }
        let sum: f64 = values.iter().rev().take(period).sum();
        Some(sum / period as f64)
    }

    /// Calculate exponential moving average
    pub fn ema(values: &[f64], period: usize, previous_ema: Option<f64>) -> Option<f64> {
        if values.is_empty() {
            return None;
        }

        let current_value = *values.last().unwrap();
        let alpha = 2.0 / (period as f64 + 1.0);

        match previous_ema {
            Some(prev) => Some(alpha * current_value + (1.0 - alpha) * prev),
            None => {
                // Bootstrap with SMA
                if values.len() >= period {
                    sma(values, period)
                } else {
                    Some(current_value)
                }
            }
        }
    }

    /// Calculate standard deviation
    pub fn std_dev(values: &[f64]) -> Option<f64> {
        if values.len() < 2 {
            return None;
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / (values.len() - 1) as f64;

        Some(variance.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_value_conversions() {
        let int_param = ParameterValue::Integer(42);
        assert_eq!(int_param.as_int(), Some(42));
        assert_eq!(int_param.as_float(), Some(42.0));

        let float_param = ParameterValue::Float(3.14);
        assert_eq!(float_param.as_float(), Some(3.14));

        let string_param = ParameterValue::String("test".to_string());
        assert_eq!(string_param.as_string(), Some("test"));

        let bool_param = ParameterValue::Boolean(true);
        assert_eq!(bool_param.as_bool(), Some(true));
    }

    #[test]
    fn test_strategy_manager_creation() {
        let manager = StrategyManager::new();
        assert_eq!(manager.list_strategies().len(), 0);
    }

    #[test]
    fn test_sma_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(utils::sma(&values, 3), Some(4.0)); // (3+4+5)/3
        assert_eq!(utils::sma(&values, 10), None); // Not enough data
    }

    #[test]
    fn test_ema_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ema = utils::ema(&values, 3, None);
        assert!(ema.is_some());
        
        let ema2 = utils::ema(&[6.0], 3, ema);
        assert!(ema2.is_some());
        assert!(ema2.unwrap() > ema.unwrap()); // Should trend upward
    }
}
