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
    pub parameters: HashMap<String, ParameterValue>,
    
    /// Risk management limits
    pub risk_limits: RiskLimits,
    
    /// Whether the strategy is enabled
    pub enabled: bool,
}

/// Parameter value that can be of different types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    Float(f64),
    Int(i64),
    String(String),
    Bool(bool),
    Array(Vec<ParameterValue>),
}

impl ParameterValue {
    /// Convert to integer if possible
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ParameterValue::Int(i) => Some(*i),
            ParameterValue::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    /// Convert to float if possible
    pub fn as_float(&self) -> Option<f64> {
        match self {
            ParameterValue::Float(f) => Some(*f),
            ParameterValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Convert to string if possible
    pub fn as_string(&self) -> Option<&str> {
        match self {
            ParameterValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Convert to boolean if possible
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParameterValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Convert to array if possible
    pub fn as_array(&self) -> Option<&Vec<ParameterValue>> {
        match self {
            ParameterValue::Array(a) => Some(a),
            _ => None,
        }
    }
}

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
