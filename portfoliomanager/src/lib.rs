//! Portfolio Manager Module
//!
//! Provides portfolio state tracking, position management, and P&L calculation for trading systems.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;

/// Represents the state of the trading portfolio at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioState {
    pub timestamp: DateTime<Utc>,
    pub balance: BigDecimal,           // Total account balance (quote currency)
    pub net_position: f64,             // Net position (base asset, e.g. BTC)
    pub positions: Vec<Position>,      // Open positions
    pub realized_pnl: BigDecimal,      // Realized profit and loss
    pub unrealized_pnl: BigDecimal,    // Unrealized profit and loss
    pub fees_paid: BigDecimal,         // Total fees paid
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            balance: BigDecimal::from(0),
            net_position: 0.0,
            positions: Vec::new(),
            realized_pnl: BigDecimal::from(0),
            unrealized_pnl: BigDecimal::from(0),
            fees_paid: BigDecimal::from(0),
        }
    }
}

/// Represents an open or closed position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,                // Trading pair (e.g. BTC/USDT)
    pub side: PositionSide,            // Long or Short
    pub quantity: f64,                 // Position size (base asset)
    pub entry_price: BigDecimal,       // Entry price
    pub mark_price: Option<BigDecimal>,// Current mark price
    pub realized_pnl: BigDecimal,      // Realized P&L for this position
    pub unrealized_pnl: BigDecimal,    // Unrealized P&L for this position
    pub open_time: DateTime<Utc>,
    pub close_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionSide {
    Long,
    Short,
}
