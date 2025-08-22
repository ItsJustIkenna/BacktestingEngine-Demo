//! Metrics module for backtesting engine
//
// Provides functions to calculate performance, risk, and trade statistics metrics for trading strategies.

/// Performance-related metrics (e.g., Sharpe ratio, profit factor, net/gross profit, average/median trade return, number of trades)
pub mod performance;

/// Risk-related metrics (e.g., maximum drawdown, volatility)
pub mod risk;

/// Trade statistics (e.g., win rate, average trade duration)
pub mod trade_stats;
