//! Core strategy traits and types
//!
//! This module defines the fundamental Strategy trait that all trading strategies
//! must implement, along with supporting types for strategy execution context.

use crate::signal::Signal;
use portfoliomanager::PortfolioState;
use riskmanager::RiskLimits;
use anyhow::Result;
use async_trait::async_trait;
use config::ParameterValue;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, collections::HashMap};
use bigdecimal::BigDecimal;

/// Result type for strategy operations
pub type StrategyResult<T> = Result<T, StrategyError>;

/// Strategy-specific error types
#[derive(Debug, thiserror::Error)]
pub enum StrategyError {
    #[error("Insufficient data: {0}")]
    InsufficientData(String),
    
    #[error("Invalid parameter: {parameter} = {value}")]
    InvalidParameter { parameter: String, value: String },
    
    #[error("Market data error: {0}")]
    MarketDataError(String),
    
    #[error("Risk limit exceeded: {0}")]
    RiskLimitExceeded(String),
    
    #[error("Strategy not initialized")]
    NotInitialized,
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Calculation error: {0}")]
    CalculationError(String),
}

/// Context information provided to strategies during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyContext {
    /// Current timestamp
    pub timestamp: DateTime<Utc>,
    /// Current portfolio state
    pub portfolio_state: PortfolioState,
    /// Historical price data window
    pub price_history: Vec<PricePoint>,
    /// Volume history
    pub volume_history: Vec<VolumePoint>,
    /// Current spread information
    pub spread: Option<BigDecimal>,
    /// Market session information
    pub market_session: MarketSession,
}

/// Price point with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub timestamp: DateTime<Utc>,
    pub price: BigDecimal,
    pub source: PriceSource,
}

/// Volume point with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumePoint {
    pub timestamp: DateTime<Utc>,
    pub volume: BigDecimal,
    pub side: Option<OrderSide>,
}

/// Source of price data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriceSource {
    Trade,
    BestBid,
    BestAsk,
    MidPrice,
    VWAP,
}

/// Order side for volume tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Market session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSession {
    pub is_open: bool,
    pub session_type: SessionType,
    pub next_open: Option<DateTime<Utc>>,
    pub next_close: Option<DateTime<Utc>>,
}

/// Type of market session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionType {
    PreMarket,
    Regular,
    PostMarket,
    Closed,
}

/// Core strategy trait that all trading strategies must implement
#[async_trait]
pub trait Strategy: Send + Sync + Debug {
    /// Get the strategy name
    fn name(&self) -> &str;
    
    /// Get the strategy version
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    /// Get strategy description
    fn description(&self) -> &str {
        "Trading strategy"
    }
    
    /// Initialize the strategy with parameters
    async fn initialize(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()>;
    
    /// Generate trading signals based on market data
    async fn generate_signals(
        &mut self,
        market_data: &dataloader::MarketData,
        context: &StrategyContext,
    ) -> StrategyResult<Vec<Signal>>;
    
    /// Update strategy state with new market data
    async fn update_state(
        &mut self,
        market_data: &dataloader::MarketData,
        context: &StrategyContext,
    ) -> StrategyResult<()> {
        // Default implementation does nothing
        let _ = (market_data, context);
        Ok(())
    }
    
    /// Get current strategy parameters
    fn get_parameters(&self) -> HashMap<String, ParameterValue>;
    
    /// Update strategy parameters
    async fn update_parameters(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()>;
    
    /// Get strategy risk limits
    fn get_risk_limits(&self) -> RiskLimits;
    
    /// Check if strategy is ready to generate signals
    fn is_ready(&self) -> bool {
        true
    }
    
    /// Get required data window size (number of historical data points needed)
    fn required_data_window(&self) -> usize {
        100 // Default window size
    }
    
    /// Get strategy performance metrics (if available)
    fn get_metrics(&self) -> Option<StrategyMetrics> {
        None
    }
    
    /// Handle strategy warmup period
    async fn warmup(
        &mut self,
        historical_data: &[dataloader::MarketData],
        context: &StrategyContext,
    ) -> StrategyResult<()> {
        // Default implementation processes all historical data
        for data in historical_data {
            self.update_state(data, context).await?;
        }
        Ok(())
    }
    
    /// Reset strategy state
    async fn reset(&mut self) -> StrategyResult<()> {
        // Default implementation does nothing
        Ok(())
    }
    
    /// Validate strategy configuration
    fn validate_config(&self, parameters: &HashMap<String, ParameterValue>) -> StrategyResult<()> {
        // Default implementation accepts all parameters
        let _ = parameters;
        Ok(())
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

impl StrategyMetrics {
    /// Create new empty metrics
    pub fn new() -> Self {
        Self {
            total_signals: 0,
            buy_signals: 0,
            sell_signals: 0,
            avg_signal_strength: 0.0,
            uptime_percentage: 100.0,
            last_signal_time: None,
            custom_metrics: HashMap::new(),
        }
    }
    
    /// Update metrics with a new signal
    pub fn record_signal(&mut self, signal: &Signal) {
        self.total_signals += 1;
        
        match signal.signal_type {
            crate::signal::SignalType::Buy => self.buy_signals += 1,
            crate::signal::SignalType::Sell => self.sell_signals += 1,
            _ => {}
        }
        
        // Update average signal strength
        let strength_value = match signal.strength {
            crate::signal::SignalStrength::Weak => 1.0,
            crate::signal::SignalStrength::Medium => 2.0,
            crate::signal::SignalStrength::Strong => 3.0,
            crate::signal::SignalStrength::Custom(val) => val,
        };
        
        self.avg_signal_strength = ((self.avg_signal_strength * (self.total_signals - 1) as f64) 
                                   + strength_value) / self.total_signals as f64;
        
        self.last_signal_time = Some(signal.timestamp);
    }
}

impl Default for StrategyMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{SignalType, SignalStrength};

    #[test]
    fn test_strategy_metrics_creation() {
        let metrics = StrategyMetrics::new();
        assert_eq!(metrics.total_signals, 0);
        assert_eq!(metrics.buy_signals, 0);
        assert_eq!(metrics.sell_signals, 0);
        assert_eq!(metrics.avg_signal_strength, 0.0);
    }

    #[test]
    fn test_strategy_metrics_signal_recording() {
        let mut metrics = StrategyMetrics::new();
        
        let signal = Signal {
            id: "test".to_string(),
            timestamp: Utc::now(),
            signal_type: SignalType::Buy,
            strength: SignalStrength::Strong,
            price: Some(BigDecimal::from(100)),
            quantity: Some(BigDecimal::from(10)),
            reason: crate::signal::SignalReason::TechnicalAnalysis("test".to_string()),
            metadata: HashMap::new(),
        };
        
        metrics.record_signal(&signal);
        
        assert_eq!(metrics.total_signals, 1);
        assert_eq!(metrics.buy_signals, 1);
        assert_eq!(metrics.avg_signal_strength, 3.0); // Strong signal = 3.0
        assert!(metrics.last_signal_time.is_some());
    }
}
