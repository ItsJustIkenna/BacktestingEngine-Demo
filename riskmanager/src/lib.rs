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
    pub max_drawdown: Option<f64>,      // Maximum allowed drawdown (0.0 to 1.0)
    pub stop_loss_pct: Option<f64>,     // Stop loss per trade as percentage (0.0 to 1.0)
    pub max_daily_loss: Option<f64>,    // Maximum daily loss in currency units
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position: Some(10.0),
            max_order_size: Some(1.0),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(0.10),
            max_drawdown: Some(0.20),      // 20% max drawdown
            stop_loss_pct: Some(0.05),     // 5% stop loss per trade
            max_daily_loss: Some(1000.0),  // $1000 max daily loss
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
    pub current_drawdown: f64,           // Current drawdown from peak (0.0 to 1.0)
    pub unrealized_pnl: f64,             // Unrealized P&L of open positions
    pub daily_pnl: f64,                  // Today's realized + unrealized P&L
}

impl Default for RiskMetrics {
    fn default() -> Self {
        Self {
            current_position: 0.0,
            current_order_size: 0.0,
            current_inventory_skew: 0.0,
            current_volatility: 0.0,
            current_drawdown: 0.0,
            unrealized_pnl: 0.0,
            daily_pnl: 0.0,
        }
    }
}

/// Risk check result indicating what action to take
#[derive(Debug, Clone, PartialEq)]
pub enum RiskAction {
    /// All checks passed, proceed normally
    Proceed,
    /// Reduce position size to stay within limits
    ReducePosition(f64),
    /// Close all positions immediately (stop loss triggered)
    CloseAllPositions(String),
    /// Reject the order entirely
    RejectOrder(String),
    /// Halt trading for the day
    HaltTrading(String),
}

/// Generic risk manager
#[derive(Debug)]
pub struct RiskManager {
    pub limits: RiskLimits,
    peak_equity: f64,
    daily_start_equity: f64,
}

impl Default for RiskManager {
    fn default() -> Self {
        Self {
            limits: RiskLimits::default(),
            peak_equity: 0.0,
            daily_start_equity: 0.0,
        }
    }
}

impl RiskManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(limits: RiskLimits) -> Self {
        Self { 
            limits,
            peak_equity: 0.0,
            daily_start_equity: 0.0,
        }
    }
    
    /// Initialize with starting equity
    pub fn initialize(&mut self, initial_equity: f64) {
        self.peak_equity = initial_equity;
        self.daily_start_equity = initial_equity;
    }
    
    /// Update peak equity tracking
    pub fn update_equity(&mut self, current_equity: f64) {
        if current_equity > self.peak_equity {
            self.peak_equity = current_equity;
        }
    }
    
    /// Get current drawdown from peak
    pub fn get_current_drawdown(&self, current_equity: f64) -> f64 {
        if self.peak_equity <= 0.0 {
            return 0.0;
        }
        ((self.peak_equity - current_equity) / self.peak_equity).clamp(0.0, 1.0)
    }
    
    /// Check if a proposed order should be allowed
    pub fn check_order(&self, order_size: f64, current_position: f64, metrics: &RiskMetrics) -> RiskAction {
        // Check max order size
        if let Some(max_order) = self.limits.max_order_size {
            if order_size > max_order {
                return RiskAction::ReducePosition(max_order);
            }
        }
        
        // Check if new position would exceed limits
        if let Some(max_pos) = self.limits.max_position {
            let new_position = current_position + order_size;
            if new_position.abs() > max_pos {
                let allowed_size = (max_pos - current_position.abs()).max(0.0);
                if allowed_size <= 0.0 {
                    return RiskAction::RejectOrder(format!(
                        "Position {} would exceed max {}", new_position.abs(), max_pos
                    ));
                }
                return RiskAction::ReducePosition(allowed_size);
            }
        }
        
        // Check drawdown limit
        if let Some(max_dd) = self.limits.max_drawdown {
            if metrics.current_drawdown > max_dd {
                return RiskAction::CloseAllPositions(format!(
                    "Drawdown {:.2}% exceeds max {:.2}%", 
                    metrics.current_drawdown * 100.0, max_dd * 100.0
                ));
            }
        }
        
        // Check daily loss limit
        if let Some(max_daily) = self.limits.max_daily_loss {
            if metrics.daily_pnl < -max_daily {
                return RiskAction::HaltTrading(format!(
                    "Daily loss ${:.2} exceeds max ${:.2}", 
                    -metrics.daily_pnl, max_daily
                ));
            }
        }
        
        RiskAction::Proceed
    }
    
    /// Check if a position should be stopped out
    pub fn check_stop_loss(&self, entry_price: f64, current_price: f64, is_long: bool) -> bool {
        if let Some(stop_pct) = self.limits.stop_loss_pct {
            let pnl_pct = if is_long {
                (current_price - entry_price) / entry_price
            } else {
                (entry_price - current_price) / entry_price
            };
            
            return pnl_pct < -stop_pct;
        }
        false
    }

    /// Check if the given metrics are within risk limits (legacy method)
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
        if let Some(max_dd) = self.limits.max_drawdown {
            if metrics.current_drawdown > max_dd {
                return Err(format!("Drawdown {:.2}% exceeds max {:.2}%", 
                    metrics.current_drawdown * 100.0, max_dd * 100.0));
            }
        }
        Ok(())
    }
}
