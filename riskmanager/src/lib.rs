//! Risk Manager Module
//!
//! Provides risk limit types and a generic risk manager for use by strategies and portfolio manager.

use serde::{Deserialize, Serialize};

/// Risk limits for a strategy or portfolio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    pub max_position: Option<f64>,      // Maximum allowed net position
    pub max_order_size: Option<f64>,    // Maximum allowed order size
    pub max_inventory_skew: Option<f64>,// Maximum allowed inventory skew
    pub max_volatility: Option<f64>,    // Maximum allowed volatility
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position: Some(10.0),
            max_order_size: Some(1.0),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(0.10),
        }
    }
}

/// Risk metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub current_position: f64,
    pub current_order_size: f64,
    pub current_inventory_skew: f64,
    pub current_volatility: f64,
}

/// Generic risk manager
#[derive(Debug)]
pub struct RiskManager {
    pub limits: RiskLimits,
}

impl RiskManager {
    pub fn new() -> Self {
        Self {
            limits: RiskLimits::default(),
        }
    }

    pub fn with_limits(limits: RiskLimits) -> Self {
        Self { limits }
    }

    /// Check if the given metrics are within risk limits
    pub fn check(&self, metrics: &RiskMetrics) -> Result<(), String> {
        if let Some(max_pos) = self.limits.max_position {
            if metrics.current_position.abs() > max_pos {
                return Err(format!("Position {} exceeds max {}", metrics.current_position, max_pos));
            }
        }
        if let Some(max_order) = self.limits.max_order_size {
            if metrics.current_order_size > max_order {
                return Err(format!("Order size {} exceeds max {}", metrics.current_order_size, max_order));
            }
        }
        if let Some(max_skew) = self.limits.max_inventory_skew {
            if metrics.current_inventory_skew.abs() > max_skew {
                return Err(format!("Inventory skew {} exceeds max {}", metrics.current_inventory_skew, max_skew));
            }
        }
        if let Some(max_vol) = self.limits.max_volatility {
            if metrics.current_volatility > max_vol {
                return Err(format!("Volatility {} exceeds max {}", metrics.current_volatility, max_vol));
            }
        }
        Ok(())
    }
}
