//! Job queue system for async backtest processing
//! 
//! Supports PostgreSQL backend
//! Workers poll the queue and execute backtests asynchronously.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Job status states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Backtest job parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobParams {
    pub symbol: String,
    pub exchange: String,
    pub strategy_type: String,
    pub initial_capital: f64,
    
    // Optimization method
    #[serde(default = "default_optimization_method")]
    pub optimization_method: String, // "genetic" or "manual"
    
    // Genetic algorithm parameters
    #[serde(default = "default_population_size")]
    pub population_size: usize,
    #[serde(default = "default_generations")]
    pub generations: usize,
    
    // Walk-forward analysis parameters (used when optimization_method == "walkforward")
    #[serde(default)]
    pub walkforward_optimization_days: Option<i64>, // Length of optimization window in days (default: 180)
    #[serde(default)]
    pub walkforward_test_days: Option<i64>, // Length of test window in days (default: 30)
    #[serde(default)]
    pub walkforward_step_days: Option<i64>, // Step size between windows in days (default: 30)
    
    // Manual strategy parameters (used when optimization_method == "manual")
    #[serde(default = "default_risk_aversion")]
    pub risk_aversion: f64,
    #[serde(default = "default_inventory_target")]
    pub inventory_target: f64,
    #[serde(default = "default_order_size")]
    pub order_size: f64,
    /// Order size as percentage of capital (e.g., 0.02 = 2% of capital per trade)
    #[serde(default = "default_order_size_pct")]
    pub order_size_pct: f64,
    #[serde(default = "default_window_ms")]
    pub window_ms: u64,
    #[serde(default = "default_min_quote_lifetime")]
    pub min_quote_lifetime_ms: u64,
    #[serde(default)]
    pub fee_rate: f64, // Will be auto-populated from exchange
    #[serde(default = "default_volatility_cap")]
    pub volatility_cap: f64,
    #[serde(default = "default_time_horizon")]
    pub time_horizon_minutes: u64,
    #[serde(default = "default_market_depth_k")]
    pub market_depth_k: f64,
    
    // Phase 2 adjustment factors
    #[serde(default = "default_volatility_adjustment")]
    pub volatility_adjustment_factor: f64,
    #[serde(default = "default_inventory_penalty")]
    pub inventory_penalty_factor: f64,
    #[serde(default = "default_trend_sensitivity")]
    pub trend_sensitivity: f64,
    #[serde(default = "default_flow_sensitivity")]
    pub flow_sensitivity: f64,
    
    // Exit and trading control parameters
    #[serde(default = "default_take_profit_pct")]
    pub take_profit_pct: f64,
    #[serde(default = "default_stop_loss_pct")]
    pub stop_loss_pct: f64,
    #[serde(default = "default_min_spread_bps")]
    pub min_spread_bps: f64,
    #[serde(default = "default_max_trades_per_minute")]
    pub max_trades_per_minute: f64,
    
    // Monte Carlo configuration
    #[serde(default = "default_monte_carlo_enabled")]
    pub monte_carlo_enabled: bool,
    #[serde(default = "default_monte_carlo_runs")]
    pub monte_carlo_runs: usize,
    
    // Walk-Forward configuration
    #[serde(default = "default_walk_forward_enabled")]
    pub walk_forward_enabled: bool,
    
    // Profitability criteria for auto-saving strategies to database
    #[serde(default = "default_auto_save_enabled")]
    pub auto_save_profitable: bool,
    #[serde(default = "default_min_sharpe_ratio")]
    pub min_sharpe_ratio: f64,
    #[serde(default = "default_min_profit_factor")]
    pub min_profit_factor: f64,
    #[serde(default = "default_max_drawdown")]
    pub max_drawdown: f64,
    #[serde(default = "default_min_win_rate")]
    pub min_win_rate: f64,
    #[serde(default = "default_min_trades")]
    pub min_trades: usize,
    #[serde(default)]
    pub min_sortino_ratio: Option<f64>,
    #[serde(default)]
    pub min_calmar_ratio: Option<f64>,
    #[serde(default)]
    pub min_total_pnl: Option<f64>,
    
    // Auto-populated fields
    #[serde(default)]
    pub commission_rate: f64, // Auto-populated from exchange
    #[serde(default)]
    pub start_date: Option<String>, // Auto-populated from database
    #[serde(default)]
    pub end_date: Option<String>, // Auto-populated from database
}

// Default functions for serde
fn default_optimization_method() -> String { "genetic".to_string() }
fn default_population_size() -> usize { 50 }
fn default_generations() -> usize { 20 }
fn default_risk_aversion() -> f64 { 0.5 }
fn default_inventory_target() -> f64 { 0.5 }
fn default_order_size() -> f64 { 0.01 }
fn default_order_size_pct() -> f64 { 0.02 }  // 2% of capital per trade
fn default_window_ms() -> u64 { 5000 }
fn default_min_quote_lifetime() -> u64 { 100 }
fn default_volatility_cap() -> f64 { 0.05 }
fn default_time_horizon() -> u64 { 60 }
fn default_market_depth_k() -> f64 { 1.5 }
fn default_volatility_adjustment() -> f64 { 0.5 }
fn default_inventory_penalty() -> f64 { 0.3 }
fn default_trend_sensitivity() -> f64 { 0.2 }
fn default_flow_sensitivity() -> f64 { -0.1 }
fn default_take_profit_pct() -> f64 { 0.005 }  // 0.5% take profit
fn default_stop_loss_pct() -> f64 { 0.003 }    // 0.3% stop loss
fn default_min_spread_bps() -> f64 { 15.0 }    // 15 basis points minimum spread
fn default_max_trades_per_minute() -> f64 { 30.0 }  // 30 trades per minute max
fn default_monte_carlo_enabled() -> bool { true }
fn default_monte_carlo_runs() -> usize { 1000 }
fn default_walk_forward_enabled() -> bool { true } // Re-enabled - now uses sampled data (200K events/window max)
fn default_auto_save_enabled() -> bool { true }
fn default_min_sharpe_ratio() -> f64 { 1.0 }
fn default_min_profit_factor() -> f64 { 1.2 }
fn default_max_drawdown() -> f64 { 0.25 }
fn default_min_win_rate() -> f64 { 0.40 }
fn default_min_trades() -> usize { 30 }

impl Default for BacktestJobParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            strategy_type: "market_making".to_string(),
            initial_capital: 10000.0,
            optimization_method: default_optimization_method(),
            population_size: default_population_size(),
            generations: default_generations(),
            walkforward_optimization_days: None,
            walkforward_test_days: None,
            walkforward_step_days: None,
            risk_aversion: default_risk_aversion(),
            inventory_target: default_inventory_target(),
            order_size: default_order_size(),
            order_size_pct: default_order_size_pct(),
            window_ms: default_window_ms(),
            min_quote_lifetime_ms: default_min_quote_lifetime(),
            fee_rate: 0.0,
            volatility_cap: default_volatility_cap(),
            time_horizon_minutes: default_time_horizon(),
            market_depth_k: default_market_depth_k(),
            volatility_adjustment_factor: default_volatility_adjustment(),
            inventory_penalty_factor: default_inventory_penalty(),
            trend_sensitivity: default_trend_sensitivity(),
            flow_sensitivity: default_flow_sensitivity(),
            monte_carlo_enabled: default_monte_carlo_enabled(),
            monte_carlo_runs: default_monte_carlo_runs(),
            walk_forward_enabled: default_walk_forward_enabled(),
            auto_save_profitable: default_auto_save_enabled(),
            min_sharpe_ratio: default_min_sharpe_ratio(),
            min_profit_factor: default_min_profit_factor(),
            max_drawdown: default_max_drawdown(),
            min_win_rate: default_min_win_rate(),
            min_trades: default_min_trades(),
            min_sortino_ratio: None,
            min_calmar_ratio: None,
            min_total_pnl: None,
            take_profit_pct: default_take_profit_pct(),
            stop_loss_pct: default_stop_loss_pct(),
            min_spread_bps: default_min_spread_bps(),
            max_trades_per_minute: default_max_trades_per_minute(),
            commission_rate: 0.0,
            start_date: None,
            end_date: None,
        }
    }
}

/// Backtest job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: String,
    pub params: BacktestJobParams,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: f64, // 0.0 to 100.0
    pub current_generation: i32, // Current generation for genetic optimization
    pub total_generations: i32, // Total generations for genetic optimization
    pub current_phase: String, // Current execution phase (initializing, genetic_optimization, final_analysis, completed, failed)
    pub phase_details: serde_json::Value, // Phase-specific details as JSON
    pub error_message: Option<String>,
}

/// Monte Carlo results summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloSummary {
    pub num_runs: usize,
    pub mean_sharpe: f64,
    pub stddev_sharpe: f64,
    pub var_95: f64,       // Value at Risk (95th percentile loss) in USD
    pub cvar_95: f64,      // Conditional VaR (expected loss beyond VaR) in USD
    pub confidence_lower: f64,  // 5th percentile Sharpe
    pub confidence_upper: f64,  // 95th percentile Sharpe
    /// Number of data points used for Monte Carlo simulation
    #[serde(default)]
    pub data_points: usize,
    /// True if VaR/CVaR are unreliable due to insufficient data (< 30 points)
    #[serde(default)]
    pub var_unreliable: bool,
    // Percentile distributions for key metrics
    #[serde(default)]
    pub p5_total_pnl: Option<f64>,
    #[serde(default)]
    pub p50_total_pnl: Option<f64>,
    #[serde(default)]
    pub p95_total_pnl: Option<f64>,
    #[serde(default)]
    pub p5_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p50_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p95_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p50_sharpe: Option<f64>,
    #[serde(default)]
    pub p5_win_rate: Option<f64>,
    #[serde(default)]
    pub p50_win_rate: Option<f64>,
    #[serde(default)]
    pub p95_win_rate: Option<f64>,
}

/// Walk-Forward results summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardSummary {
    pub periods_tested: usize,
    pub average_performance: f64,
    pub consistency_score: f64,
    pub out_of_sample_degradation: f64,
}

/// Inventory metrics summary for market making strategies
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventorySummary {
    /// Total volume traded (buy + sell) in quote currency (USD)
    pub total_volume: f64,
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
}

/// Order lifecycle and execution quality metrics for API
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Total orders placed (quotes submitted)
    pub orders_placed: usize,
    /// Orders that received at least one fill
    pub orders_filled: usize,
    /// Orders cancelled before any fill
    pub orders_cancelled: usize,
    /// Total individual fills received
    pub total_fills: usize,
    /// Fill rate: orders_filled / orders_placed
    pub fill_rate: f64,
    /// Cancellation rate: orders_cancelled / orders_placed
    pub cancellation_rate: f64,
    /// Average fills per filled order
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
    /// A round-trip is a buy fill matched with a sell fill
    #[serde(default)]
    pub round_trip_win_rate: Option<f64>,
    /// Total round-trips completed (buy+sell pairs)
    #[serde(default)]
    pub total_round_trips: usize,
    /// Profitable round-trips (spread captured > fees)
    #[serde(default)]
    pub profitable_round_trips: usize,
    /// Spread capture rate: actual_spread / theoretical_spread
    /// How much of the quoted spread was actually captured after adverse selection
    #[serde(default)]
    pub spread_capture_rate: Option<f64>,
    /// Adverse selection rate: % of fills where price moved against us
    /// Higher = more informed flow, worse execution quality
    #[serde(default)]
    pub adverse_selection_rate: Option<f64>,
    /// Average P&L per round-trip in quote currency
    #[serde(default)]
    pub avg_pnl_per_round_trip: Option<f64>,
}

/// Backtest results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub job_id: String,
    /// Strategy type: "market_making", "mean_reversion", "momentum", "arbitrage"
    #[serde(default)]
    pub strategy_type: Option<String>,
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub total_trades: usize,
    pub profit_factor: f64,
    pub equity_curve: Vec<f64>,
    pub completed_at: DateTime<Utc>,
    
    // Trade breakdown - helps explain PnL vs win_rate discrepancies
    /// Number of closed (completed) trades - these contribute to win_rate
    #[serde(default)]
    pub closed_trades: Option<usize>,
    /// Number of positions still open at end of backtest
    #[serde(default)]
    pub open_positions: Option<usize>,
    /// Realized P&L from closed trades only
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    /// Unrealized P&L from open positions (mark-to-market)
    #[serde(default)]
    pub unrealized_pnl: Option<f64>,
    
    // Additional performance metrics
    #[serde(default)]
    pub annualized_return: Option<f64>,
    #[serde(default)]
    pub volatility: Option<f64>,
    #[serde(default)]
    pub sortino_ratio: Option<f64>,
    #[serde(default)]
    pub calmar_ratio: Option<f64>,
    #[serde(default)]
    pub avg_trade_return: Option<f64>,
    #[serde(default)]
    pub best_trade: Option<f64>,
    #[serde(default)]
    pub worst_trade: Option<f64>,
    /// Average time in trade in SECONDS (convert to minutes for display: value / 60.0)
    #[serde(default)]
    pub avg_time_in_trade: Option<f64>,
    
    // Risk metrics
    /// Value at Risk (95% confidence) - POSITIVE value representing potential loss in USD
    #[serde(default)]
    pub value_at_risk_95: Option<f64>,
    /// Expected Shortfall / CVaR (95%) - POSITIVE value representing expected loss beyond VaR in USD
    #[serde(default)]
    pub expected_shortfall_95: Option<f64>,
    #[serde(default)]
    pub max_drawdown_duration_days: Option<i32>,
    #[serde(default)]
    pub current_drawdown: Option<f64>,
    #[serde(default)]
    pub avg_drawdown: Option<f64>,
    
    // Execution metrics
    #[serde(default)]
    pub total_orders: Option<i32>,
    #[serde(default)]
    pub filled_orders: Option<i32>,
    #[serde(default)]
    pub cancelled_orders: Option<i32>,
    #[serde(default)]
    pub avg_slippage: Option<f64>,
    #[serde(default)]
    pub total_commission_paid: Option<f64>,
    
    // Strategy parameters used
    #[serde(default)]
    pub strategy_params: Option<serde_json::Value>,
    
    // Date range for the backtest (for equity curve x-axis display)
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    
    // Advanced analysis results
    #[serde(default)]
    pub monte_carlo: Option<MonteCarloSummary>,
    #[serde(default)]
    pub walk_forward: Option<WalkForwardSummary>,
    #[serde(default)]
    pub inventory_metrics: Option<InventorySummary>,
    #[serde(default)]
    pub execution_metrics: Option<ExecutionSummary>,
    
    // Warning/halt reason (e.g., "NOT_VIABLE: Commission costs exceeded 80%")
    #[serde(default)]
    pub risk_halt_reason: Option<String>,
}

/// Job queue interface
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Submit a new backtest job
    async fn submit_job(&self, params: BacktestJobParams) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get next job from queue
    async fn get_next_job(&self) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get job by ID
    async fn get_job(&self, job_id: &str) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job status
    async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job progress
    async fn update_progress(&self, job_id: &str, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update generation progress for genetic optimization
    async fn update_generation(&self, job_id: &str, current_generation: i32, total_generations: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update current phase of job execution
    async fn update_phase(&self, job_id: &str, phase: &str, phase_details: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job heartbeat timestamp
    async fn update_heartbeat(&self, job_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Store backtest results
    async fn store_result(&self, result: &BacktestResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get backtest results
    async fn get_result(&self, job_id: &str) -> Result<Option<BacktestResult>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get queue length
    async fn queue_length(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get database connection pool (if available)
    fn get_pool(&self) -> Option<Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>> {
        None
    }
    
    /// Downcast to Any for type-specific features
    fn as_any(&self) -> &dyn std::any::Any;
}

// Re-export implementations
pub mod postgres;

pub use postgres::PostgresQueue;


