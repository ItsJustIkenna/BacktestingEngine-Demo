//! Strategy configuration types and parameter management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a trading strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Unique identifier for the strategy
    pub id: String,
    
    /// Human-readable name
    pub name: String,
    
    /// Type of strategy (e.g., "AvellanedaStoikov", "MeanReversion")
    pub strategy_type: String,
    
    /// Trading symbol (e.g., "BTCUSD")
    pub symbol: String,
    
    /// Exchange name (e.g., "kraken", "coinbase")
    pub exchange: String,
    
    /// Strategy parameters
    pub parameters: HashMap<String, config::ParameterValue>,
    
    /// Risk management limits
    pub risk_limits: RiskLimits,
    
    /// Whether the strategy is enabled
    pub enabled: bool,
}

// Re-export ParameterValue from config crate to avoid duplication
pub use config::ParameterValue;

// Old local ParameterValue commented out to avoid conflicts
/*
/// Parameter value that can be of different types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    Float(f64),
    Int(i64),
    String(String),
    Bool(bool),
    Array(Vec<ParameterValue>),
}

// Note: Methods like as_int(), as_float(), etc. are defined in config::ParameterValue
*/

/// Risk management limits for a strategy
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskLimits {
    /// Maximum position size
    pub max_position_size: Option<f64>,
    
    /// Maximum daily loss allowed
    pub max_daily_loss: Option<f64>,
    
    /// Maximum number of open orders
    pub max_open_orders: Option<u32>,
    
    /// Maximum drawdown percentage
    pub max_drawdown_percent: Option<f64>,
    
    /// Stop loss percentage
    pub stop_loss_percent: Option<f64>,
    
    /// Take profit percentage
    pub take_profit_percent: Option<f64>,
}
