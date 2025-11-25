//! Metrics module for backtesting engine
//
// Provides functions to calculate performance, risk, and trade statistics metrics for trading strategies.

/// Performance-related metrics (e.g., Sharpe ratio, profit factor, net/gross profit, average/median trade return, number of trades)
pub mod performance;

/// Risk-related metrics (e.g., maximum drawdown, volatility)
pub mod risk;

/// Report generation for comprehensive backtest analysis
pub mod reporting;

/// Database persistence for reports and metrics
#[cfg(feature = "persistence")]
pub mod persistence;

/// Trade statistics (e.g., win rate, average trade duration)
pub mod trade_stats;

use serde::{Serialize, Deserialize};

// Re-export commonly used functions for convenience
pub use performance::{sharpe_ratio, profit_factor, net_profit, gross_profit, gross_loss, 
                     average_trade_return, median_trade_return, number_of_trades};
pub use risk::{max_drawdown, volatility};
pub use trade_stats::{win_rate, average_trade_duration};

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
    // Transaction cost metrics
    pub total_commission: f64,
    pub total_slippage: f64,
    pub total_market_impact: f64,
    pub transaction_costs_percentage: f64,
}

impl PerformanceMetrics {
    /// Create PerformanceMetrics from raw trading data
    pub fn from_trade_data(
        trade_returns: &[f64],
        equity_curve: &[f64], 
        trade_durations: &[f64],
        risk_free_rate: f64,
    ) -> Self {
        Self {
            sharpe_ratio: sharpe_ratio(trade_returns, risk_free_rate).unwrap_or(0.0),
            profit_factor: profit_factor(
                gross_profit(trade_returns), 
                gross_loss(trade_returns)
            ).unwrap_or(0.0),
            net_profit: net_profit(trade_returns),
            gross_profit: gross_profit(trade_returns),
            gross_loss: gross_loss(trade_returns),
            average_trade_return: average_trade_return(trade_returns).unwrap_or(0.0),
            median_trade_return: median_trade_return(trade_returns).unwrap_or(0.0),
            number_of_trades: number_of_trades(trade_returns),
            max_drawdown: max_drawdown(equity_curve).unwrap_or(0.0),
            volatility: volatility(trade_returns).unwrap_or(0.0),
            win_rate: win_rate(trade_returns).unwrap_or(0.0),
            average_trade_duration: average_trade_duration(trade_durations).unwrap_or(0.0),
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        }
    }
}

// Re-export reporting components
pub use reporting::{
    ReportGenerator, BacktestReport,
    ReportMetadata, BacktestAnalysis, RiskMetrics, TradeDistribution,
    PerformanceAttribution, BenchmarkComparison, TradeRecord,
    EquityPoint, DrawdownPeriod, create_basic_report
};

// Re-export config types used by reporting
pub use config::{ReportConfig, ReportFormat};
