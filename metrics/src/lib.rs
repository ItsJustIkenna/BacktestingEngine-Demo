//! Metrics module for backtesting engine
//
// Provides functions to calculate performance, risk, and trade statistics metrics for trading strategies.

/// Performance-related metrics (e.g., Sharpe ratio, profit factor, net/gross profit, average/median trade return, number of trades)
pub mod performance;

/// Risk-related metrics (e.g., maximum drawdown, volatility)
pub mod risk;

/// Trade statistics (e.g., win rate, average trade duration)
pub mod trade_stats;

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub sharpe_ratio: f64,
    pub profit_factor: f64,
    pub net_profit: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub average_trade_return: f64,
    pub median_trade_return: f64,
    pub number_of_trades: usize,
    pub max_drawdown: f64,
    pub volatility: f64,
    pub win_rate: f64,
    pub average_trade_duration: f64,
    // Add more fields as needed
}
