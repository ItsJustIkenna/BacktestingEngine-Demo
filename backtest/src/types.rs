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
    /// Number of closed (completed) trades - these contribute to win_rate and realized P&L
    pub closed_trades: usize,
    /// Number of positions still open at end of backtest - contribute to unrealized P&L
    pub open_positions: usize,
    /// Timestamp of the first trade in the backtest data
    pub first_trade_timestamp: Option<DateTime<Utc>>,
    /// Timestamp of the last trade in the backtest data
    pub last_trade_timestamp: Option<DateTime<Utc>>,
    /// Price at the first trade
    pub first_trade_price: Option<f64>,
    /// Price at the last trade
    pub last_trade_price: Option<f64>,
    /// Realized P&L from closed trades only
    pub realized_pnl: Option<f64>,
    /// Unrealized P&L from open positions (mark-to-market)
    pub unrealized_pnl: Option<f64>,
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
    /// Individual trade P&L values (realized) for Monte Carlo simulation
    #[serde(default)]
    pub trade_returns: Vec<f64>,
    pub initial_capital: f64,
    pub data_quality: Option<DataQualityReport>,
    pub transaction_costs: Option<TransactionCostAnalysis>,
    pub market_impact: Option<MarketImpactAnalysis>,
    pub inventory_metrics: Option<InventoryMetrics>,
    pub execution_metrics: Option<ExecutionMetrics>,
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

/// Order lifecycle and execution quality metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Total orders placed (quotes submitted)
    pub orders_placed: usize,
    /// Orders that received at least one fill
    pub orders_filled: usize,
    /// Orders cancelled before any fill
    pub orders_cancelled: usize,
    /// Total individual fills received (may exceed orders if partial fills)
    pub total_fills: usize,
    /// Fill rate: orders_filled / orders_placed
    pub fill_rate: f64,
    /// Cancellation rate: orders_cancelled / orders_placed
    pub cancellation_rate: f64,
    /// Average fills per filled order (1.0 = no partial fills)
    pub avg_fills_per_order: f64,
    /// Average time from order placement to first fill (seconds)
    pub avg_time_to_fill_seconds: f64,
    /// Median time to fill (seconds)
    pub median_time_to_fill_seconds: f64,
    /// Bid orders placed
    pub bid_orders: usize,
    /// Ask orders placed
    pub ask_orders: usize,
    /// Bid orders filled
    pub bid_fills: usize,
    /// Ask orders filled
    pub ask_fills: usize,
    /// Bid fill rate
    pub bid_fill_rate: f64,
    /// Ask fill rate
    pub ask_fill_rate: f64,
    /// Average quote spread at time of placement (bps)
    pub avg_quote_spread_bps: f64,
    
    // Market-Making Specific Win Rate Metrics
    /// Round-trip win rate: profitable_round_trips / total_round_trips
    pub round_trip_win_rate: Option<f64>,
    /// Total round-trips completed (buy+sell pairs)
    pub total_round_trips: usize,
    /// Profitable round-trips (spread captured > fees)
    pub profitable_round_trips: usize,
    /// Spread capture rate: actual_spread / theoretical_spread
    pub spread_capture_rate: Option<f64>,
    /// Adverse selection rate: % of fills where price moved against us
    pub adverse_selection_rate: Option<f64>,
    /// Average P&L per round-trip in quote currency
    pub avg_pnl_per_round_trip: Option<f64>,
}

/// Inventory management metrics for market making strategies
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventoryMetrics {
    /// Total volume traded (buy + sell) in quote currency (USD)
    pub total_volume_traded: f64,
    /// Average inventory value over the backtest period
    pub avg_inventory_value: f64,
    /// Inventory turnover per day (total_volume / avg_inventory / days)
    pub turnover_per_day: f64,
    /// Number of buy fills
    pub buy_fills: usize,
    /// Number of sell fills
    pub sell_fills: usize,
    /// Total buy volume in quote currency
    pub buy_volume: f64,
    /// Total sell volume in quote currency
    pub sell_volume: f64,
    /// Inventory imbalance ratio (buy_volume - sell_volume) / total_volume
    pub imbalance_ratio: f64,
    
    // === Starting/Ending Position Tracking ===
    /// Starting inventory quantity (base asset units)
    #[serde(default)]
    pub starting_inventory: f64,
    /// Starting inventory value in USD
    #[serde(default)]
    pub starting_inventory_value: f64,
    /// Starting mark price used for valuation
    #[serde(default)]
    pub starting_mark_price: f64,
    /// Ending inventory quantity (base asset units)
    #[serde(default)]
    pub ending_inventory: f64,
    /// Ending inventory value in USD
    #[serde(default)]
    pub ending_inventory_value: f64,
    /// Ending mark price used for valuation
    #[serde(default)]
    pub ending_mark_price: f64,
    /// Starting capital (USD)
    #[serde(default)]
    pub starting_capital: f64,
    /// Ending capital (USD cash after all trades)
    #[serde(default)]
    pub ending_capital: f64,
    /// Final equity (ending_capital + ending_inventory_value)
    #[serde(default)]
    pub final_equity: f64,
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
