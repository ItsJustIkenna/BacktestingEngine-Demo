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
    pub sharpe_ratio: Option<f64>,
    pub profit_factor: Option<f64>,
    pub win_rate: Option<f64>,
    pub avg_trade_return: Option<f64>,
    pub median_trade_return: Option<f64>,
    pub avg_trade_duration: Option<f64>,
    pub volatility: Option<f64>,
    pub gross_profit: Option<f64>,
    pub gross_loss: Option<f64>,
    pub net_profit: Option<f64>,
    pub equity_curve: Vec<f64>,
    pub initial_capital: f64,
}
