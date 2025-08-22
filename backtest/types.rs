//! Types and enums for the backtest module
//!
//! Shared types for simulation events, results, and configuration.

use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;

#[derive(Debug, Clone)]
pub enum BacktestEvent {
    MarketData {
        timestamp: DateTime<Utc>,
        // ... other fields as needed
    },
    OrderSubmitted {
        order_id: String,
        timestamp: DateTime<Utc>,
    },
    OrderFilled {
        order_id: String,
        fill_price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    },
    // Add more event types as needed
}

#[derive(Debug, Clone, Default)]
pub struct BacktestResult {
    pub total_pnl: BigDecimal,
    pub num_trades: usize,
    pub max_drawdown: BigDecimal,
    // Add more result fields as needed
}
