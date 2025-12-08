//! Types and enums for the backtest module
//!
//! Shared types for simulation events, results, and configuration.

use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use serde::{Serialize, Deserialize};

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BacktestResult {
    pub total_pnl: BigDecimal,
    pub num_trades: usize,
    pub max_drawdown: BigDecimal,
    pub sharpe_ratio: Option<f64>,
    pub sortino_ratio: Option<f64>,
    pub calmar_ratio: Option<f64>,
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
    pub data_quality: Option<DataQualityReport>,
    pub transaction_costs: Option<TransactionCostAnalysis>,
    pub market_impact: Option<MarketImpactAnalysis>,
    pub risk_halt_reason: Option<String>,
}

/// Comprehensive data quality report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQualityReport {
    pub overall_score: f64,
    pub total_data_points: usize,
    pub time_span_seconds: i64,
    pub gaps_detected: Vec<DataGap>,
    pub average_gap_seconds: f64,
    pub max_gap_seconds: i64,
    pub gap_ratio: f64, // percentage of time with gaps
    pub outliers_detected: usize,
    pub data_completeness_score: f64,
    pub timestamp_consistency_score: f64,
    pub order_quality: Option<OrderStreamQuality>,
}

/// Information about a detected data gap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGap {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub duration_seconds: i64,
    pub severity: GapSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GapSeverity {
    Minor,      // < 2x expected frequency
    Moderate,   // 2-5x expected frequency
    Severe,     // 5-10x expected frequency
    Critical,   // > 10x expected frequency
}

/// Transaction cost breakdown and analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionCostAnalysis {
    pub total_commission: f64,
    pub total_slippage: f64,
    pub total_market_impact: f64,
    pub total_transaction_costs: f64,
    pub costs_as_percentage_of_pnl: f64,
    pub avg_cost_per_trade: f64,
    pub commission_by_exchange: std::collections::HashMap<String, f64>,
}

/// Market impact modeling results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketImpactAnalysis {
    pub trades_with_impact: usize,
    pub avg_impact_bps: f64,
    pub max_impact_bps: f64,
    pub total_impact_cost: f64,
    pub liquidity_warnings: Vec<LiquidityWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityWarning {
    pub timestamp: DateTime<Utc>,
    pub order_size: f64,
    pub available_liquidity: f64,
    pub liquidity_ratio: f64, // order_size / available_liquidity
    pub message: String,
}

/// Order stream quality analysis (for market making strategies)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderStreamQuality {
    pub total_orders: usize,
    pub order_gaps: Vec<OrderGap>,
    pub unpaired_quotes: Vec<UnpairedQuote>,
    pub expected_quote_interval_seconds: u64,
    pub avg_quote_interval_seconds: f64,
    pub quote_consistency_score: f64,
    pub missing_quote_cycles: u64,
    pub bid_ask_pair_ratio: f64, // Should be close to 1.0 for market making
}

/// Detected gap in order/quote stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderGap {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub duration_seconds: i64,
    pub missing_quote_cycles: u64,
    pub severity: GapSeverity,
}

/// Quote without matching counterpart (bid without ask, or vice versa)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnpairedQuote {
    pub timestamp: DateTime<Utc>,
    pub order_id: String,
    pub side: String,
    pub price: BigDecimal,
}
