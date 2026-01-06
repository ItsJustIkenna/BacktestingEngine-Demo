//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
#[cfg(feature = "postgres")]
use std::path::PathBuf;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};

use crate::job_queue::{BacktestJob, BacktestResult, JobQueue, JobStatus, MonteCarloSummary, WalkForwardSummary, InventorySummary, ExecutionSummary};
use crate::distributed::ga_coordinator::{DistributedMode, DistributedGARunner, DistributedGAConfig};

// Logging infrastructure from the library crate
use crate::logging_facade::{MAIN_LOGGER, GENETIC_LOGGER, WALKFORWARD_LOGGER, BACKTEST_LOGGER};
use crate::{log_info, log_error, log_debug, log_warn};

// Import existing backtest infrastructure
use backtest::BacktestEngine;
use strategy::{StrategyManager, StrategyConfig, StrategyRiskLimits};
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use riskmanager::RiskManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use config::BacktestConfig;
use dataloader::{MarketData, TradeData};

// ============================================================================
// COMPREHENSIVE FITNESS FUNCTION
// ============================================================================

/// Calculate comprehensive fitness score for genetic optimization
/// 
/// This function evaluates market-making strategies holistically to find genuinely 
/// profitable ones. It penalizes strategies that look good on paper but fail after 
/// transaction costs, or that don't exhibit normal market-making behavior.
/// 
/// # Weights (sum to 1.0)
/// - Sortino Ratio: 0.20 (risk-adjusted returns, downside only)
/// - Sharpe Ratio: 0.15 (risk-adjusted returns, total volatility)
/// - Profit Factor: 0.10 (gross profit / gross loss)
/// - Win Rate: 0.05 (consistency of winning)
/// - Net Profit %: 0.35 (absolute profitability)
/// - Calmar Ratio: 0.15 (recovery ability)
/// 
/// # Hard Rejects (return near-zero fitness)
/// - Commission ratio ≥ 60%: Strategy can't overcome costs
/// - Fill rate < 1%: Strategy isn't actually trading
/// 
/// # General Penalties
/// - Commission ratio 30-60%: Scaled penalty
/// - Drawdown > 10%: 0.8x, > 20%: 0.5x, > 35%: 0.2x
/// - < 30 trades: Linear penalty
/// - Avg trade < 4x commission: 0.4x (marginal trades)
/// - Fill rate < 5%: Scaled penalty (quotes too wide)
/// 
/// # Market-Making Specific Penalties
/// - Round-trip win rate < 40%: Adverse selection eating profits
/// - Adverse selection rate > 50%: Getting picked off
/// - Bid/Ask fill imbalance > 5:1: Extreme one-sidedness (some imbalance OK from inventory skewing)
/// - Inventory turnover < 1x/day: Holding too long
/// - Inventory imbalance > 50%: Never reverting (moderate imbalance OK from skewing)
/// - Trades < 0.5/hour: Too inactive
/// - Avg trade > 2% of capital: Directional betting, not MM
/// 
/// # Bonuses
/// - Well-rounded strategy (profit >5%, PF >1.5, WR >50%, DD <10%): 1.3x
/// - Commission ratio < 20%: 1.2x, < 30%: 1.1x
/// - Fill rate ≥ 10%: 1.15x, ≥ 5%: 1.05x
/// - Round-trip win rate ≥ 60%: 1.10x
/// - Inventory turnover ≥ 3x/day: 1.10x
pub fn calculate_comprehensive_fitness(result: &backtest::types::BacktestResult, initial_capital: f64) -> f64 {
    use bigdecimal::ToPrimitive;
    
    // === HARD CONSTRAINT: Reject commission-inefficient strategies early ===
    // This prevents the optimizer from wasting time on strategies that can't be profitable
    if let (Some(gross_profit), Some(transaction_costs)) = 
        (&result.gross_profit, &result.transaction_costs) 
    {
        let gross = *gross_profit;
        let commission = transaction_costs.total_commission;
        
        // Hard reject: If commissions exceed 60% of gross profit, this is unviable (was 80%)
        if gross <= 0.0 {
            log_debug!(GENETIC_LOGGER, "FITNESS: gross_profit={:.2} <= 0, returning 0.001", gross);
            return 0.001; // No gross profit = essentially zero fitness
        }
        let commission_ratio = commission / gross;
        log_debug!(GENETIC_LOGGER, "FITNESS: gross={:.2} commission={:.2} ratio={:.2}% trades={}", 
            gross, commission, commission_ratio * 100.0, result.num_trades);
        if commission_ratio >= 0.60 {
            // Return a tiny but non-zero value that's proportional to how bad it is
            // This ensures even the "best" commission-heavy strategy loses to any viable one
            let tiny_fitness = 0.001 * (1.0 - (commission_ratio - 0.60).min(0.4));
            log_warn!(GENETIC_LOGGER, "FITNESS REJECTED: commission_ratio={:.2}% >= 60%, fitness={:.6}", 
                commission_ratio * 100.0, tiny_fitness);
            return tiny_fitness;
        }
    } else {
        log_warn!(GENETIC_LOGGER, "FITNESS: gross_profit or transaction_costs is None!");
    }
    
    // === HARD CONSTRAINT: Reject strategies with extremely low fill rates ===
    // A strategy that can't get filled is useless regardless of theoretical returns
    if let Some(execution_metrics) = &result.execution_metrics {
        let fill_rate = execution_metrics.fill_rate;
        if fill_rate < 0.01 {
            // Less than 1% fill rate is essentially not trading
            log_warn!(GENETIC_LOGGER, "FITNESS REJECTED: fill_rate={:.2}% < 1%, returning 0.002", fill_rate * 100.0);
            return 0.002;
        }
    }
    
    // === HARD CONSTRAINT: Reject strategies with too few trades ===
    // Statistical significance requires minimum trades - otherwise Sharpe/metrics are meaningless
    if result.num_trades < 10 {
        // Less than 10 trades makes metrics statistically unreliable
        let tiny_fitness = 0.003 + (result.num_trades as f64 * 0.0001);
        log_warn!(GENETIC_LOGGER, "FITNESS REJECTED: num_trades={} < 10, returning {:.6}", 
            result.num_trades, tiny_fitness);
        return tiny_fitness;
    }
    
    // === Primary Metrics ===
    let sharpe = result.sharpe_ratio.unwrap_or(0.0);
    let sortino = result.sortino_ratio.unwrap_or(sharpe);
    let pf = result.profit_factor.unwrap_or(1.0);
    let win_rate = result.win_rate.unwrap_or(0.0);
    let net_profit_pct = result.net_profit.unwrap_or(0.0) / initial_capital;
    let calmar = result.calmar_ratio.unwrap_or(0.0);
    
    // === Base Fitness (weights sum to 1.0) ===
    // Emphasize NET profitability and risk-adjusted returns
    let mut fitness = 
        sortino * 0.20 +           // Increased weight on downside risk
        sharpe * 0.15 +            // Increased weight on risk-adjusted return
        (pf - 1.0).max(0.0) * 0.10 +
        win_rate * 0.05 +          // Reduced - win rate matters less than avg profit
        net_profit_pct * 0.35 +    // Net profit is what matters!
        calmar.min(3.0) * 0.15;    // Return vs max drawdown
    
    // === Penalty: Insufficient Trades ===
    if result.num_trades < 30 {
        fitness *= (result.num_trades as f64 / 30.0).max(0.1);
    }
    
    // === Penalty: Excessive Drawdown (Stricter) ===
    let dd_value = result.max_drawdown.to_f64().unwrap_or(0.0);
    let dd_pct = (dd_value / initial_capital).abs();
    if dd_pct > 0.10 {
        let dd_penalty = if dd_pct > 0.35 { 0.2 } else if dd_pct > 0.20 { 0.5 } else { 0.8 };
        fitness *= dd_penalty;
    }
    
    // === Soft Penalty for Moderate Commission Ratio (30-60%) - Stricter ===
    if let (Some(gross_profit), Some(transaction_costs)) = 
        (&result.gross_profit, &result.transaction_costs) 
    {
        let gross = *gross_profit;
        let commission = transaction_costs.total_commission;
        
        if gross > 0.0 {
            let commission_ratio = commission / gross;
            if commission_ratio > 0.30 && commission_ratio < 0.60 {
                // Linear penalty: 30% = no penalty, 60% = 0.2x (before hard cutoff kicks in)
                let penalty = 1.0 - (commission_ratio - 0.3) * 2.667;
                fitness *= penalty.max(0.2);
            }
        }
    }
    
    // === Penalty: Poor Average Trade (Stricter - must be 4x commission) ===
    if let (Some(avg_trade), Some(transaction_costs)) = 
        (&result.avg_trade_return, &result.transaction_costs)
    {
        let avg_commission = if result.num_trades > 0 {
            transaction_costs.total_commission / result.num_trades as f64
        } else { 0.0 };
        
        // Each trade should profit more than 4x the commission to be worthwhile (raised from 3x)
        if *avg_trade > 0.0 && *avg_trade < avg_commission * 4.0 {
            fitness *= 0.4; // Stronger penalty for marginal trades
        }
    }
    
    // === Penalty: Low Fill Rate (market making strategies need fills!) ===
    // A strategy with low fill rate can't capture spreads effectively
    if let Some(execution_metrics) = &result.execution_metrics {
        let fill_rate = execution_metrics.fill_rate;
        if fill_rate < 0.05 {
            // Less than 5% fill rate = quotes too wide or market too fast
            // Scale penalty: 5% = 0.5x, 2% = 0.2x
            let fill_penalty = (fill_rate / 0.05).max(0.2);
            fitness *= fill_penalty;
            log_debug!(GENETIC_LOGGER, "FITNESS: fill_rate={:.2}% < 5%, penalty={:.2}x", 
                fill_rate * 100.0, fill_penalty);
        }
    }
    
    // === Bonus: Well-Rounded Strategy ===
    if net_profit_pct > 0.05 && pf > 1.5 && win_rate > 0.50 && dd_pct < 0.10 {
        fitness *= 1.3; // Increased bonus for truly good strategies
    }
    
    // === Bonus: Low Commission Ratio (reward efficient strategies) ===
    if let (Some(gross_profit), Some(transaction_costs)) = 
        (&result.gross_profit, &result.transaction_costs) 
    {
        let gross = *gross_profit;
        let commission = transaction_costs.total_commission;
        if gross > 0.0 {
            let commission_ratio = commission / gross;
            if commission_ratio < 0.20 {
                // Reward very efficient strategies: <20% commission ratio = 1.2x bonus
                fitness *= 1.2;
            } else if commission_ratio < 0.30 {
                // Moderate reward: 20-30% commission ratio = 1.1x bonus
                fitness *= 1.1;
            }
        }
    }
    
    // === Bonus: Good Fill Rate (reward executable strategies) ===
    if let Some(execution_metrics) = &result.execution_metrics {
        let fill_rate = execution_metrics.fill_rate;
        if fill_rate >= 0.10 {
            // 10%+ fill rate is good for market making with realistic fills
            fitness *= 1.15;
        } else if fill_rate >= 0.05 {
            // 5-10% is acceptable
            fitness *= 1.05;
        }
    }
    
    // =========================================================================
    // MARKET MAKING SPECIFIC CONSTRAINTS
    // These penalize strategies that don't exhibit normal MM behavior
    // =========================================================================
    
    // === Penalty: Poor Round-Trip Win Rate ===
    // MM strategies should capture spreads consistently (55-70% win rate typical)
    if let Some(execution_metrics) = &result.execution_metrics {
        if let Some(rt_win_rate) = execution_metrics.round_trip_win_rate {
            if rt_win_rate < 0.40 {
                // Win rate < 40% indicates adverse selection eating profits
                let penalty = (rt_win_rate / 0.40).max(0.3);
                fitness *= penalty;
                log_debug!(GENETIC_LOGGER, "FITNESS: round_trip_win_rate={:.1}% < 40%, penalty={:.2}x", 
                    rt_win_rate * 100.0, penalty);
            } else if rt_win_rate >= 0.60 {
                // Bonus for strong win rate
                fitness *= 1.10;
            }
        }
        
        // === Penalty: High Adverse Selection Rate ===
        // Indicates strategy is getting picked off by informed traders
        if let Some(adverse_rate) = execution_metrics.adverse_selection_rate {
            if adverse_rate > 0.50 {
                // More than 50% adverse selection is a major red flag
                let penalty = (0.50 / adverse_rate).max(0.4);
                fitness *= penalty;
                log_debug!(GENETIC_LOGGER, "FITNESS: adverse_selection_rate={:.1}% > 50%, penalty={:.2}x", 
                    adverse_rate * 100.0, penalty);
            }
        }
        
        // === Penalty: Imbalanced Bid/Ask Fill Rates ===
        // Some imbalance is expected due to inventory skewing - only penalize extreme cases
        // (e.g., 5:1 ratio suggests strategy is effectively one-sided)
        if execution_metrics.bid_fills > 10 && execution_metrics.ask_fills > 10 {
            let bid_rate = execution_metrics.bid_fill_rate;
            let ask_rate = execution_metrics.ask_fill_rate;
            if bid_rate > 0.0 && ask_rate > 0.0 {
                let imbalance = (bid_rate / ask_rate).max(ask_rate / bid_rate);
                if imbalance > 5.0 {
                    // More than 5:1 imbalance suggests one-sided market or broken skewing
                    fitness *= 0.85;
                    log_debug!(GENETIC_LOGGER, "FITNESS: bid/ask imbalance {:.1}:1 > 5:1, penalty=0.85x", imbalance);
                }
            }
        }
    }
    
    // === Penalty: Poor Inventory Turnover ===
    // MM should cycle inventory, not hold forever
    if let Some(inventory_metrics) = &result.inventory_metrics {
        let turnover = inventory_metrics.turnover_per_day;
        if turnover < 1.0 && inventory_metrics.total_volume_traded > 0.0 {
            // Less than 1x daily turnover = holding inventory too long
            let penalty = (turnover / 1.0).max(0.5);
            fitness *= penalty;
            log_debug!(GENETIC_LOGGER, "FITNESS: turnover_per_day={:.2}x < 1.0, penalty={:.2}x", 
                turnover, penalty);
        } else if turnover >= 3.0 {
            // Good turnover = actively market making
            fitness *= 1.10;
        }
        
        // === Penalty: Extreme Inventory Imbalance (Buy/Sell Volume) ===
        // Some imbalance is fine (inventory skewing), but >50% net over entire backtest
        // suggests strategy never reverts and is just accumulating directionally
        let imbalance = inventory_metrics.imbalance_ratio.abs();
        if imbalance > 0.50 {
            // More than 50% net directional bias = not reverting properly
            let penalty = ((1.0 - imbalance) / 0.50).max(0.6);
            fitness *= penalty;
            log_debug!(GENETIC_LOGGER, "FITNESS: inventory_imbalance={:.1}% > 50%, penalty={:.2}x", 
                imbalance * 100.0, penalty);
        }
        
        // === Penalty: Large Ending Inventory Exposure ===
        // MM should end close to flat - large ending positions indicate failed reversion
        // Penalize strategies that end with >20% of capital in unrealized inventory exposure
        let ending_exposure = inventory_metrics.ending_inventory_value.abs();
        let initial_capital = result.initial_capital;
        if initial_capital > 0.0 {
            let exposure_pct = ending_exposure / initial_capital;
            if exposure_pct > 0.20 {
                // >20% ending exposure = directional betting, not MM
                // Scale penalty: 20% = 0.8x, 40% = 0.6x, 60%+ = 0.4x
                let penalty = (0.20 / exposure_pct).max(0.4);
                fitness *= penalty;
                log_debug!(GENETIC_LOGGER, "FITNESS: ending_inventory_exposure={:.1}% > 20%, penalty={:.2}x", 
                    exposure_pct * 100.0, penalty);
            } else if exposure_pct < 0.05 {
                // Reward near-flat ending positions
                fitness *= 1.10;
            }
        }
    }
    
    // === Penalty: Trades Too Infrequent ===
    // MM should be active, not just sitting with stale quotes
    if result.num_trades > 0 {
        if let (Some(first_ts), Some(last_ts)) = (&result.first_trade_timestamp, &result.last_trade_timestamp) {
            let duration_hours = (last_ts.signed_duration_since(*first_ts).num_seconds() as f64) / 3600.0;
            if duration_hours > 0.0 {
                let trades_per_hour = result.num_trades as f64 / duration_hours;
                if trades_per_hour < 0.5 {
                    // Less than 1 trade per 2 hours is too inactive for MM
                    let penalty = (trades_per_hour / 0.5).max(0.3);
                    fitness *= penalty;
                    log_debug!(GENETIC_LOGGER, "FITNESS: trades_per_hour={:.2} < 0.5, penalty={:.2}x", 
                        trades_per_hour, penalty);
                }
            }
        }
    }
    
    // === Penalty: Average Trade Too Large Relative to Position ===
    // MM should make many small spread captures, not few large directional bets
    if let Some(avg_trade) = result.avg_trade_return {
        let initial = result.initial_capital;
        if initial > 0.0 {
            let avg_trade_pct = avg_trade.abs() / initial;
            if avg_trade_pct > 0.02 {
                // Average trade > 2% of capital suggests directional betting, not MM
                let penalty = (0.02 / avg_trade_pct).max(0.5);
                fitness *= penalty;
                log_debug!(GENETIC_LOGGER, "FITNESS: avg_trade_pct={:.2}% > 2%, penalty={:.2}x", 
                    avg_trade_pct * 100.0, penalty);
            }
        }
    }
    
    fitness.max(0.0)
}

// ============================================================================
// EXCHANGE NAME UTILITIES
// ============================================================================

/// Extract the base exchange name, stripping co-location and configuration suffixes.
/// 
/// This is used when looking up market data in the database - the data is stored
/// under the base exchange name (e.g., "kraken"), but the user may have configured
/// the job with a variant like "kraken_colocated" or "kraken_established".
/// 
/// Examples:
/// - "kraken" -> "kraken"
/// - "kraken_colocated" -> "kraken"
/// - "kraken_established" -> "kraken"
/// - "kraken_colocated_established" -> "kraken"
/// - "binance" -> "binance"
fn base_exchange_name(exchange: &str) -> &str {
    // Strip known suffixes in order (most specific first)
    let suffixes = [
        "_colocated_established",
        "_colocated",
        "_established",
        "-colocated-established",
        "-colocated",
        "-established",
    ];
    
    for suffix in &suffixes {
        if exchange.ends_with(suffix) {
            return &exchange[..exchange.len() - suffix.len()];
        }
    }
    
    exchange
}

// ============================================================================
// EFFICIENT PROGRESS TRACKING
// ============================================================================

// Import the progress broadcaster
use crate::progress_broadcaster::{get_progress_broadcaster, ProgressUpdate};

/// Efficient progress tracker using broadcast channels for real-time updates
/// 
/// # Architecture
/// - Real-time updates go to broadcast channel (non-blocking, ~176ns)
/// - DB writes only on phase changes (start, complete, fail)
/// - No RwLock contention in hot path
/// - No deadlock possible
/// 
/// # Usage
/// ```ignore
/// let tracker = ProgressTracker::new(job_id, job_queue);
/// 
/// // Fast broadcast updates (never blocks)
/// tracker.broadcast_generation(5, 20);
/// tracker.broadcast_phase("genetic_optimization", json!({...}));
/// 
/// // DB writes only on lifecycle events
/// tracker.persist_phase_change("completed").await;
/// ```
pub struct ProgressTracker {
    job_id: String,
    job_queue: Arc<dyn JobQueue>,
}

impl ProgressTracker {
    pub fn new(job_id: String, job_queue: Arc<dyn JobQueue>) -> Self {
        Self {
            job_id,
            job_queue,
        }
    }
    
    /// Broadcast progress update (fire-and-forget, never blocks)
    /// This is safe to call from the genetic optimizer's hot loop
    #[inline]
    pub fn broadcast_progress(&self, progress: f64) {
        get_progress_broadcaster().publish(ProgressUpdate {
            job_id: self.job_id.clone(),
            progress,
            current_generation: 0,
            total_generations: 0,
            phase: "running".to_string(),
            phase_details: serde_json::json!({}),
            timestamp_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        });
    }
    
    /// Broadcast generation progress (fire-and-forget, never blocks)
    #[inline]
    pub fn broadcast_generation(&self, current: i32, total: i32) {
        get_progress_broadcaster().publish_generation(&self.job_id, current as usize, total as usize);
    }
    
    /// Broadcast phase change (fire-and-forget, never blocks)
    #[inline]
    pub fn broadcast_phase(&self, phase: &str, details: serde_json::Value) {
        get_progress_broadcaster().publish_phase(&self.job_id, phase, details);
    }
    
    /// Persist phase change to database (only for lifecycle events)
    /// Call this for: job start, phase transitions, job complete, job failed
    pub async fn persist_phase_change(&self, phase: &str, details: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.job_queue.update_phase(&self.job_id, phase, details).await
    }
    
    /// Persist progress to database (sparingly - only for major milestones)
    pub async fn persist_progress(&self, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.job_queue.update_progress(&self.job_id, progress).await
    }
    
    /// Persist generation to database (only at completion)
    pub async fn persist_generation(&self, current: i32, total: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.job_queue.update_generation(&self.job_id, current, total).await
    }
    
    /// Create a generation callback for genetic optimizer
    /// Returns a callback that broadcasts updates via channel (never blocks)
    /// 
    /// CRITICAL: This callback is completely non-blocking. It:
    /// - Does NOT hold any locks
    /// - Does NOT write to database
    /// - Does NOT await anything
    /// - Uses fire-and-forget broadcast (~176ns latency)
    pub fn generation_callback(self: &Arc<Self>) -> Arc<dyn Fn(usize, usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync> {
        let job_id = self.job_id.clone();
        let job_queue = Arc::clone(&self.job_queue);
        Arc::new(move |current: usize, total: usize| {
            let job_id = job_id.clone();
            let job_queue = Arc::clone(&job_queue);
            Box::pin(async move {
                let progress_pct = (current as f64 / total as f64) * 80.0 + 5.0; // 5% to 85% for genetic phase
                
                // Log every generation for complete visibility
                log_info_structured!(GENETIC_LOGGER, "GA_GENERATION",
                    "event" => "GA_GENERATION",
                    "job_id" => &job_id,
                    "current_generation" => current,
                    "total_generations" => total,
                    "progress_pct" => format!("{:.1}", (current as f64 / total as f64) * 100.0)
                );
                
                // Broadcast to WebSocket clients (fire-and-forget, ~176ns)
                get_progress_broadcaster().publish_generation(&job_id, current, total);
                
                // Persist progress to database for UI polling
                // The UI may poll the database, so we need to persist progress
                if let Err(e) = job_queue.update_progress(&job_id, progress_pct).await {
                    log_warn!(MAIN_LOGGER, "Job {} failed to persist generation progress: {}", job_id, e);
                }
                if let Err(e) = job_queue.update_generation(&job_id, current as i32, total as i32).await {
                    log_warn!(MAIN_LOGGER, "Job {} failed to persist generation info: {}", job_id, e);
                }
            })
        })
    }
    
    // Legacy methods for backward compatibility - these now just broadcast
    #[deprecated(note = "Use broadcast_progress instead")]
    pub async fn set_progress(&self, progress: f64) {
        self.broadcast_progress(progress);
    }
    
    #[deprecated(note = "Use broadcast_generation instead")]
    pub async fn set_generation(&self, current: i32, total: i32) {
        self.broadcast_generation(current, total);
    }
    
    #[deprecated(note = "Use broadcast_phase instead")]
    pub async fn set_phase(&self, phase: &str, details: serde_json::Value) {
        self.broadcast_phase(phase, details);
    }
    
    #[deprecated(note = "No longer needed - broadcasts are instant")]
    pub async fn maybe_flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    
    #[deprecated(note = "No longer needed - broadcasts are instant")]
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    
    #[deprecated(note = "Use persist_phase_change for final state")]
    pub async fn flush_final(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

// ============================================================================
// END PROGRESS TRACKING
// ============================================================================

/// Walk-forward window definition
#[derive(Debug, Clone)]
struct WalkForwardWindow {
    opt_start: chrono::DateTime<Utc>,
    opt_end: chrono::DateTime<Utc>,
    test_start: chrono::DateTime<Utc>,
    test_end: chrono::DateTime<Utc>,
}

/// Optimal parameters found during walk-forward optimization
#[derive(Debug, Clone)]
struct OptimalParams {
    risk_aversion: f64,
    inventory_target: f64,
    order_size_pct: f64,  // Order size as percentage of capital
}

/// Result from a single walk-forward window
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields retained for serialization and future reporting
struct WalkForwardWindowResult {
    window_id: u32,
    opt_start: chrono::DateTime<Utc>,
    opt_end: chrono::DateTime<Utc>,
    test_start: chrono::DateTime<Utc>,
    test_end: chrono::DateTime<Utc>,
    optimal_params: OptimalParams,
    optimization_fitness: f64,
    oos_pnl: f64,
    oos_sharpe: f64,
    oos_max_drawdown: f64,
    oos_win_rate: f64,
    oos_trades: usize,
}

/// Backtest worker
pub struct Worker {
    id: usize,
    job_queue: Arc<dyn JobQueue>,
    /// Database connection pool for saving profitable strategies
    db_pool: Option<Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>>,
    /// Profitability criteria for auto-saving strategies
    profitability_criteria: crate::profitability::ProfitabilityCriteria,
}

impl Worker {
    pub fn new(id: usize, job_queue: Arc<dyn JobQueue>) -> Self {
        Self { 
            id, 
            job_queue,
            db_pool: None,
            profitability_criteria: crate::profitability::ProfitabilityCriteria::default(),
        }
    }
    
    /// Create a worker with database connection pool for saving profitable strategies
    pub fn with_db_pool(
        id: usize, 
        job_queue: Arc<dyn JobQueue>,
        db_pool: Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>,
    ) -> Self {
        Self { 
            id, 
            job_queue,
            db_pool: Some(db_pool),
            profitability_criteria: crate::profitability::ProfitabilityCriteria::default(),
        }
    }
    
    /// Set custom profitability criteria
    pub fn with_profitability_criteria(mut self, criteria: crate::profitability::ProfitabilityCriteria) -> Self {
        self.profitability_criteria = criteria;
        self
    }
    
    /// Start the worker loop
    pub async fn run(self) {
        log_info_structured!(MAIN_LOGGER, "WORKER_STARTED",
            "event" => "WORKER_STARTED",
            "worker_id" => &self.id
        );
        log_debug!(MAIN_LOGGER, "🚀 Worker {} started and polling for jobs", self.id);
        
        loop {
            match self.process_next_job().await {
                Ok(true) => {
                    // Job processed successfully
                }
                Ok(false) => {
                    // No job available, wait a bit
                    sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    log_error!(MAIN_LOGGER, "Worker {} error processing job: {}", self.id, e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    
    /// Process the next job in queue
    async fn process_next_job(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Get next job from queue
        let job = match self.job_queue.get_next_job().await? {
            Some(j) => j,
            None => return Ok(false), // No job available
        };
        
        log_info_structured!(MAIN_LOGGER, "JOB_CLAIMED",
            "event" => "JOB_CLAIMED",
            "worker_id" => &self.id,
            "job_id" => &job.id,
            "symbol" => &job.params.symbol,
            "exchange" => &job.params.exchange,
            "method" => &job.params.optimization_method
        );
        log_debug!(MAIN_LOGGER, "🎯 Worker {} claimed job: {} (symbol={}, exchange={})", 
            self.id, job.id, job.params.symbol, job.params.exchange);
        
        // Note: get_next_job() already set status to "running" atomically in the database
        // Verify status was updated by reading it back
        eprintln!("✅ JOB_STATUS_CHECK | job_id={} | expected=running | job.status={:?}", job.id, job.status);
        
        // Spawn heartbeat task to update job every 30 seconds
        let heartbeat_queue = self.job_queue.clone();
        let heartbeat_job_id = job.id.clone();
        let (heartbeat_tx, mut heartbeat_rx) = tokio::sync::mpsc::channel::<()>(1);
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut tick_count: u32 = 0;
            loop {
                tokio::select! {
                    // Prioritize shutdown signal over heartbeat tick
                    biased;
                    
                    _ = heartbeat_rx.recv() => {
                        break; // Graceful shutdown
                    }
                    _ = interval.tick() => {
                        tick_count += 1;
                        // Only log every 10th heartbeat to reduce noise (every 5 minutes)
                        if tick_count % 10 == 1 {
                            log_debug!(MAIN_LOGGER, "💓 Heartbeat #{} for job {}", tick_count, heartbeat_job_id);
                        }
                        if let Err(e) = heartbeat_queue.update_heartbeat(&heartbeat_job_id).await {
                            log_warn!(MAIN_LOGGER, "⚠️ Failed to update heartbeat for job {}: {}", heartbeat_job_id, e);
                            break; // Exit loop on persistent errors
                        }
                    }
                }
            }
        });
        
        // Execute backtest (genetic, walkforward, or manual)
        log_info!(MAIN_LOGGER, "Job {} starting optimization_method={}", job.id, job.params.optimization_method);
        
        // Set initial progress so UI shows job is running
        log_info!(MAIN_LOGGER, "Job {} setting initial progress to 0%", job.id);
        if let Err(e) = self.job_queue.update_progress(&job.id, 0.0).await {
            log_error!(MAIN_LOGGER, "Job {} failed to set initial progress: {}", job.id, e);
        }
        
        // CRITICAL: Set total_generations upfront for genetic optimization jobs
        // This prevents the UI from showing "Gen null/null" during initialization
        if job.params.optimization_method == "genetic" && job.params.generations > 0 {
            log_info!(MAIN_LOGGER, "Job {} setting total_generations={} at initialization", job.id, job.params.generations);
            if let Err(e) = self.job_queue.update_generation(&job.id, 0, job.params.generations as i32).await {
                log_error!(MAIN_LOGGER, "Job {} failed to set initial generation info: {}", job.id, e);
            }
        }
        
        if let Err(e) = self.job_queue.update_phase(&job.id, "initializing", serde_json::json!({
            "optimization_method": job.params.optimization_method,
            "symbol": job.params.symbol,
            "exchange": job.params.exchange,
            "total_generations": job.params.generations,
        })).await {
            log_error!(MAIN_LOGGER, "Job {} failed to set initial phase: {}", job.id, e);
        }
        
        // ============================================================================
        // AUTO DATA PREPARATION
        // ============================================================================
        // Automatically prepare binary data files if they don't exist.
        // This eliminates the manual `prepare-data` CLI step.
        #[cfg(feature = "postgres")]
        {
            if let Err(e) = self.ensure_binary_data(&job).await {
                log_error!(MAIN_LOGGER, "Job {} failed to prepare binary data: {}", job.id, e);
                // Continue anyway - the job will fall back to database streaming
            }
        }
        
        // === COMPREHENSIVE PORTFOLIO ROUTING LOGGING ===
        eprintln!("\n============================================================");
        eprintln!("🔀 [WORKER] JOB_ROUTING_DECISION job_id={}", job.id);
        eprintln!("   portfolio_assets: {:?}", job.params.portfolio_assets.as_ref().map(|a| 
            a.iter().map(|x| format!("{}@{}", x.symbol, x.exchange)).collect::<Vec<_>>()
        ));
        eprintln!("   portfolio_assets_count: {}", job.params.portfolio_assets.as_ref().map(|a| a.len()).unwrap_or(0));
        eprintln!("   is_portfolio(): {}", job.params.is_portfolio());
        eprintln!("   all_symbols(): {:?}", job.params.all_symbols());
        eprintln!("   optimization_method: {}", job.params.optimization_method);
        eprintln!("   start_date: {:?}", job.params.start_date);
        eprintln!("   end_date: {:?}", job.params.end_date);
        eprintln!("   ROUTING TO: {}", if job.params.is_portfolio() { "PORTFOLIO_BACKTEST" } else { "SINGLE_SYMBOL_BACKTEST" });
        eprintln!("============================================================\n");
        
        let result = if job.params.is_portfolio() {
            eprintln!("🏦 [WORKER] Entering PORTFOLIO path for job {}", job.id);
            log_info_structured!(BACKTEST_LOGGER, "PORTFOLIO_BACKTEST_START",
                "event" => "PORTFOLIO_BACKTEST_START",
                "job_id" => &job.id,
                "symbols" => format!("{:?}", job.params.all_symbols()),
                "num_symbols" => job.params.all_symbols().len()
            );
            log_info!(MAIN_LOGGER, "🏦 Starting portfolio backtest for job {} with {} symbols", 
                job.id, job.params.all_symbols().len());
            
            // Route portfolio backtests through GA optimization
            match job.params.optimization_method.as_str() {
                "genetic" => {
                    log_info!(MAIN_LOGGER, "🧬 Portfolio GA optimization for job {}", job.id);
                    self.execute_portfolio_backtest_with_ga(&job).await
                }
                _ => {
                    // Non-GA portfolio backtest (use default params)
                    self.execute_portfolio_backtest(&job).await
                }
            }
        } else {
            // Single-symbol backtest - use existing logic
            eprintln!("📊 [WORKER] Entering SINGLE-SYMBOL path for job {} (symbol={})", job.id, job.params.symbol);
            eprintln!("   ⚠️ If this should be portfolio mode, check is_portfolio() logic!");
            match job.params.optimization_method.as_str() {
                "genetic" => {
                    eprintln!("   -> Executing single-symbol GA optimization");
                    log_info_structured!(GENETIC_LOGGER, "GENETIC_OPTIMIZATION_START",
                        "event" => "GENETIC_OPTIMIZATION_START",
                        "job_id" => &job.id,
                        "population_size" => job.params.population_size,
                        "generations" => job.params.generations
                    );
                    self.execute_genetic_optimization(&job).await
                },
                "walkforward" => {
                    log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_ANALYSIS_START",
                        "event" => "WALKFORWARD_ANALYSIS_START",
                        "job_id" => &job.id,
                        "opt_days" => format!("{:?}", job.params.walkforward_optimization_days),
                        "test_days" => format!("{:?}", job.params.walkforward_test_days)
                    );
                    log_debug!(MAIN_LOGGER, "🔄 Starting walk-forward analysis for job {}", job.id);
                    self.execute_walkforward_analysis(&job).await
                },
                _ => {
                    log_info_structured!(BACKTEST_LOGGER, "BACKTEST_START",
                        "event" => "BACKTEST_START",
                        "job_id" => &job.id,
                        "risk_aversion" => job.params.risk_aversion,
                        "order_size" => job.params.order_size
                    );
                    self.execute_backtest(&job).await
                },
            }
        };
        
        let result = match result {
            Ok(result) => {
                // IMPORTANT: Keep heartbeat running during result storage to prevent
                // other workers from "recovering" this job while we're still processing
                
                // Log comprehensive results in organized format
                self.log_backtest_results(&job.id, &result);
                
                // Store results with timeout to detect spawn_blocking exhaustion
                let store_result = tokio::time::timeout(
                    tokio::time::Duration::from_secs(30),
                    self.job_queue.store_result(&result)
                ).await;
                
                // Update status to completed BEFORE stopping heartbeat
                // This prevents race condition where another worker sees stale heartbeat
                let status_result = self.job_queue
                    .update_job_status(&job.id, JobStatus::Completed, None)
                    .await;
                
                // NOW stop heartbeat - job is already marked complete
                log_debug!(MAIN_LOGGER, "Job {} stopping heartbeat task", job.id);
                let _ = heartbeat_tx.send(()).await;
                let _ = tokio::time::timeout(tokio::time::Duration::from_secs(2), heartbeat_handle).await;
                
                // Check results after heartbeat is stopped
                match store_result {
                    Ok(Ok(_)) => {
                        log_info!(MAIN_LOGGER, "Job {} results stored successfully", job.id);
                    }
                    Ok(Err(e)) => {
                        log_error!(MAIN_LOGGER, "Job {} failed to store results: {}", job.id, e);
                        // Job is already marked complete, but results may be missing
                    }
                    Err(_) => {
                        log_error!(MAIN_LOGGER, "Job {} TIMEOUT storing results", job.id);
                    }
                }
                
                status_result?;
                
                // Check profitability and save strategy if criteria are met
                if let Some(ref _pool) = self.db_pool {
                    self.save_if_profitable(&result, &job).await;
                }
                
                log_info!(MAIN_LOGGER, "Job {} completed successfully ✓", job.id);
                
                Ok(true)
            }
            Err(e) => {
                // Gracefully stop heartbeat task
                let _ = heartbeat_tx.send(()).await;
                let _ = tokio::time::timeout(tokio::time::Duration::from_secs(2), heartbeat_handle).await;
                
                let error_msg = format!("Backtest failed: {}", e);
                log_error!(MAIN_LOGGER, "Job {} failed: {}", job.id, error_msg);
                
                // Update status to failed
                self.job_queue
                    .update_job_status(&job.id, JobStatus::Failed, Some(error_msg))
                    .await?;
                
                Ok(true)
            }
        };
        
        result
    }
    
    /// Check if the strategy is profitable and save it to the database
    /// Uses profitability criteria from job parameters (UI-configurable)
    async fn save_if_profitable(&self, result: &BacktestResult, job: &BacktestJob) {
        use crate::profitability::{self, ProfitabilityCriteria};
        
        // Check if auto-save is enabled for this job
        if !job.params.auto_save_profitable {
            log_debug!(BACKTEST_LOGGER, 
                "Job {} auto-save disabled, skipping profitability check", 
                job.id
            );
            return;
        }
        
        // Use criteria from job parameters (UI-configurable)
        let criteria = ProfitabilityCriteria::from_job_params(&job.params);
        
        // Check profitability criteria
        let profitability_result = criteria.is_profitable(result);
        
        if profitability_result.is_profitable {
            log_info!(BACKTEST_LOGGER, 
                "Job {} strategy meets profitability criteria (score: {:.2}). Saving to database...", 
                job.id, profitability_result.score
            );
            
            // Get the database pool
            if let Some(ref pool) = self.db_pool {
                // Determine strategy name and type from job params
                let strategy_name = format!("{}_{}", job.params.strategy_type, job.params.symbol);
                let strategy_type = &job.params.strategy_type;
                
                match profitability::save_profitable_strategy(
                    pool.clone(),
                    result,
                    &strategy_name,
                    strategy_type,
                    &criteria,
                ).await {
                    Ok(Some(instance_id)) => {
                        log_info!(BACKTEST_LOGGER, 
                            "Job {} saved profitable strategy instance {} to database", 
                            job.id, instance_id
                        );
                    }
                    Ok(None) => {
                        // This shouldn't happen since we already checked profitability
                        log_warn!(BACKTEST_LOGGER, 
                            "Job {} strategy not saved (profitability check failed in save function)", 
                            job.id
                        );
                    }
                    Err(e) => {
                        log_error!(BACKTEST_LOGGER, 
                            "Job {} failed to save profitable strategy: {}", 
                            job.id, e
                        );
                    }
                }
            }
        } else {
            log_debug!(BACKTEST_LOGGER, 
                "Job {} strategy does not meet profitability criteria: {:?}", 
                job.id, profitability_result.failures
            );
        }
    }
    
    /// Execute backtest using existing CLI logic
    async fn execute_backtest(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let params = &job.params;
        log_debug!(BACKTEST_LOGGER, "Job {} loading market data for {} on {}", job.id, params.symbol, params.exchange);
        
        // Update phase: loading data
        let _ = self.job_queue.update_phase(&job.id, "data_loading", serde_json::json!({
            "symbol": params.symbol,
            "exchange": params.exchange,
            "data_source": params.data_source,
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 5.0).await;
        
        // Load market data based on data source configuration
        let data = if params.data_source == "tardis" {
            // Check if specific file paths are provided (from file upload)
            if let (Some(trades_path), Some(orderbook_path)) = (&params.tardis_trades_path, &params.tardis_orderbook_path) {
                self.load_tardis_data_from_files(trades_path, orderbook_path).await?
            } else {
                // Use default path pattern
                let data_path = params.tardis_data_path.as_deref().unwrap_or("/data/tardis");
                self.load_tardis_data(&params.symbol, data_path).await?
            }
        } else {
            // Default: load from database
            self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?
        };
        
        if data.is_empty() {
            return Err("No market data available".into());
        }
        
        // Update phase: preparing backtest
        let _ = self.job_queue.update_phase(&job.id, "preparing", serde_json::json!({
            "data_points": data.len(),
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 15.0).await;
        
        // Create configuration
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = params.initial_capital;
        backtest_config.trading.commission_rate = params.commission_rate;
        
        // Create engine
        let engine = BacktestEngine::new(backtest_config);
        
        // Get initial price for inventory calculation
        let initial_price = Self::get_reference_price(&data);
        
        // Initialize portfolio with BOTH quote and base assets for proper market making
        // Split capital based on initial_inventory_pct (default 50% as quote, 50% as base)
        let inventory_pct = params.initial_inventory_pct;
        let quote_balance = params.initial_capital * (1.0 - inventory_pct);
        let base_value = params.initial_capital * inventory_pct;
        let base_quantity = if initial_price > 0.0 { base_value / initial_price } else { 0.0 };
        let first_timestamp = Self::get_first_timestamp(&data);
        
        let mut portfolio = PortfolioState::with_inventory(
            params.symbol.clone(),
            BigDecimal::from_f64(quote_balance).unwrap_or_default(),
            base_quantity,
            BigDecimal::from_f64(initial_price).unwrap_or_default(),
            first_timestamp,
        );
        
        log_info!(BACKTEST_LOGGER, "Portfolio initialized: quote=${:.2} base={:.6} units @ ${:.2} (total=${:.2})",
            quote_balance, base_quantity, initial_price, params.initial_capital);
        
        // Create orderbook
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(params.symbol.clone(), params.exchange.clone(), orderbook_config);
        
        // Create risk manager
        let mut risk_manager = RiskManager::new();
        
        // Create strategy
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: params.risk_aversion,
            inventory_target: params.inventory_target,
            order_size: params.order_size,
            order_size_pct: params.order_size_pct,
            initial_capital: params.initial_capital,
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
        strategy_params_map.insert("order_size_pct".to_string(), config::ParameterValue::Float(params.order_size_pct));
        strategy_params_map.insert("initial_capital".to_string(), config::ParameterValue::Float(params.initial_capital));
        
        let strategy_config = StrategyConfig {
            id: format!("backtest-{}", job.id),
            name: "Market Making".to_string(),
            strategy_type: "MarketMaking".to_string(),
            symbol: params.symbol.clone(),
            exchange: params.exchange.clone(),
            parameters: strategy_params_map,
            risk_limits: StrategyRiskLimits::default(),
            enabled: true,
        };
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        
        // log_info!(...)
        
        let mut strategy_manager = StrategyManager::new();
        strategy_manager
            .register_strategy(strategy_config.clone(), strategy)
            .map_err(|e| {
                // log_error!(...)
                format!("Failed to register strategy: {}", e)
            })?;
        
        // Update phase: running backtest
        let _ = self.job_queue.update_phase(&job.id, "running_backtest", serde_json::json!({
            "strategy": "AvellanedaStoikov",
            "data_points": data.len(),
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 25.0).await;
        
        // Run backtest
        let start_time = std::time::Instant::now();
        
        let result = engine
            .run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &data,
            )
            .await?;
        
        let execution_time = start_time.elapsed();
        
        // Update phase: processing results
        let _ = self.job_queue.update_phase(&job.id, "processing_results", serde_json::json!({
            "execution_time_ms": execution_time.as_millis(),
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 90.0).await;
        
        // Convert to API result format
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
        // Convert Monte Carlo results if available
        let monte_carlo = result.monte_carlo.as_ref().map(|mc| MonteCarloSummary {
            num_runs: mc.num_runs,
            mean_sharpe: mc.mean_metrics.sharpe_ratio,
            stddev_sharpe: mc.stddev_metrics.sharpe_ratio,
            var_95: mc.var_95,
            cvar_95: mc.cvar_95,
            confidence_lower: mc.percentile_metrics.get(&5).map(|m| m.sharpe_ratio).unwrap_or(0.0),
            confidence_upper: mc.percentile_metrics.get(&95).map(|m| m.sharpe_ratio).unwrap_or(0.0),
            data_points: mc.data_points,
            var_unreliable: mc.var_unreliable,
            // Percentile distributions
            p5_total_pnl: mc.percentile_metrics.get(&5).map(|m| m.net_profit),
            p50_total_pnl: mc.percentile_metrics.get(&50).map(|m| m.net_profit),
            p95_total_pnl: mc.percentile_metrics.get(&95).map(|m| m.net_profit),
            p5_max_drawdown: mc.percentile_metrics.get(&5).map(|m| m.max_drawdown),
            p50_max_drawdown: mc.percentile_metrics.get(&50).map(|m| m.max_drawdown),
            p95_max_drawdown: mc.percentile_metrics.get(&95).map(|m| m.max_drawdown),
            p50_sharpe: mc.percentile_metrics.get(&50).map(|m| m.sharpe_ratio),
            p5_win_rate: mc.percentile_metrics.get(&5).map(|m| m.win_rate),
            p50_win_rate: mc.percentile_metrics.get(&50).map(|m| m.win_rate),
            p95_win_rate: mc.percentile_metrics.get(&95).map(|m| m.win_rate),
        });
        
        // Convert Walk-Forward results if available
        let walk_forward = result.walk_forward.as_ref().map(|wf| WalkForwardSummary {
            periods_tested: wf.periods_tested,
            average_performance: wf.average_performance,
            consistency_score: wf.consistency_score,
            out_of_sample_degradation: wf.out_of_sample_degradation,
        });
        
        // Calculate annualized return from equity curve
        let annualized_return = if result.core.equity_curve.len() >= 2 {
            let initial = result.core.equity_curve.first().copied().unwrap_or(1.0);
            let final_val = result.core.equity_curve.last().copied().unwrap_or(1.0);
            if initial > 0.0 {
                let total_return = (final_val - initial) / initial;
                // Assuming each equity point is ~1 minute, calculate trading days
                let minutes = result.core.equity_curve.len() as f64;
                let trading_days = minutes / (6.5 * 60.0); // ~6.5 hours of trading per day
                if trading_days > 0.0 {
                    // Annualize: (1 + total_return)^(252/trading_days) - 1
                    let annualized = (1.0 + total_return).powf(252.0 / trading_days) - 1.0;
                    Some(annualized)
                } else {
                    Some(total_return)
                }
            } else {
                None
            }
        } else {
            None
        };
        
        let backtest_result = BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some(job.params.strategy_type.clone()),
            total_pnl,
            sharpe_ratio: result.core.sharpe_ratio.unwrap_or(0.0),
            max_drawdown,
            win_rate: result.core.win_rate.unwrap_or(0.0),
            total_trades: result.core.num_trades,
            profit_factor: result.core.profit_factor.unwrap_or(1.0),
            equity_curve: result.core.equity_curve,
            completed_at: Utc::now(),
            // Trade breakdown for transparency
            closed_trades: Some(result.core.closed_trades),
            open_positions: Some(result.core.open_positions),
            realized_pnl: result.core.realized_pnl,
            unrealized_pnl: result.core.unrealized_pnl,
            // Additional metrics
            annualized_return,
            volatility: result.core.volatility,
            sortino_ratio: result.core.sortino_ratio,
            calmar_ratio: result.core.calmar_ratio,
            avg_trade_return: result.core.avg_trade_return,
            best_trade: result.core.gross_profit, // Approximate
            worst_trade: result.core.gross_loss, // Approximate
            avg_time_in_trade: result.core.avg_trade_duration,
            // Risk metrics from Monte Carlo if available
            value_at_risk_95: monte_carlo.as_ref().map(|mc| mc.var_95),
            expected_shortfall_95: monte_carlo.as_ref().map(|mc| mc.cvar_95),
            max_drawdown_duration_days: None,
            current_drawdown: None,
            avg_drawdown: None,
            // Execution metrics from transaction costs
            total_orders: result.core.transaction_costs.as_ref().map(|_| result.core.num_trades as i32),
            filled_orders: Some(result.core.num_trades as i32),
            cancelled_orders: Some(0),
            avg_slippage: result.core.transaction_costs.as_ref().map(|tc| tc.total_slippage / result.core.num_trades.max(1) as f64),
            total_commission_paid: result.core.transaction_costs.as_ref().map(|tc| tc.total_commission),
            // Comprehensive strategy params for live trading deployment
            strategy_params: Some(Self::build_strategy_params_for_live(&job.params)),
            // Date range for equity curve x-axis - use actual trade timestamps
            start_date: result.core.first_trade_timestamp.map(|ts| ts.to_rfc3339())
                .or_else(|| job.params.start_date.clone()),
            end_date: result.core.last_trade_timestamp.map(|ts| ts.to_rfc3339())
                .or_else(|| job.params.end_date.clone()),
            monte_carlo,
            walk_forward,
            inventory_metrics: result.core.inventory_metrics.as_ref().map(|inv| InventorySummary {
                total_volume: inv.total_volume_traded,
                avg_inventory_value: inv.avg_inventory_value,
                turnover_per_day: inv.turnover_per_day,
                buy_fills: inv.buy_fills,
                sell_fills: inv.sell_fills,
                buy_volume: inv.buy_volume,
                sell_volume: inv.sell_volume,
                imbalance_ratio: inv.imbalance_ratio,
                ..Default::default()
            }),
            execution_metrics: result.core.execution_metrics.as_ref().map(|ex| ExecutionSummary {
                orders_placed: ex.orders_placed,
                orders_filled: ex.orders_filled,
                orders_cancelled: ex.orders_cancelled,
                total_fills: ex.total_fills,
                fill_rate: ex.fill_rate,
                cancellation_rate: ex.cancellation_rate,
                avg_fills_per_order: ex.avg_fills_per_order,
                avg_time_to_fill_seconds: ex.avg_time_to_fill_seconds,
                median_time_to_fill_seconds: ex.median_time_to_fill_seconds,
                bid_orders: ex.bid_orders,
                ask_orders: ex.ask_orders,
                bid_fills: ex.bid_fills,
                ask_fills: ex.ask_fills,
                bid_fill_rate: ex.bid_fill_rate,
                ask_fill_rate: ex.ask_fill_rate,
                avg_quote_spread_bps: ex.avg_quote_spread_bps,
                round_trip_win_rate: ex.round_trip_win_rate,
                total_round_trips: ex.total_round_trips,
                profitable_round_trips: ex.profitable_round_trips,
                spread_capture_rate: ex.spread_capture_rate,
                adverse_selection_rate: ex.adverse_selection_rate,
                avg_pnl_per_round_trip: ex.avg_pnl_per_round_trip,
            }),
            risk_halt_reason: None,
            metrics_profile: Some(Self::build_metrics_profile(&job.params.strategy_type)),
            is_portfolio: false,
            portfolio_metrics: None,
            routing_metrics: None,
        };
        
        log_info_structured!(BACKTEST_LOGGER, "BACKTEST_COMPLETE",
            "event" => "BACKTEST_COMPLETE",
            "job_id" => &job.id,
            "total_pnl" => format!("{:.2}", backtest_result.total_pnl),
            "sharpe_ratio" => format!("{:.3}", backtest_result.sharpe_ratio),
            "total_trades" => backtest_result.total_trades
        );
        
        // Final progress update
        let _ = self.job_queue.update_progress(&job.id, 100.0).await;
        let _ = self.job_queue.update_phase(&job.id, "completed", serde_json::json!({
            "total_pnl": backtest_result.total_pnl,
            "sharpe_ratio": backtest_result.sharpe_ratio,
            "total_trades": backtest_result.total_trades,
        })).await;
        
        Ok(backtest_result)
    }
    
    /// Execute portfolio/multi-symbol backtest
    /// 
    /// Runs simulation across multiple symbols with portfolio-level aggregation.
    /// Each symbol gets allocated capital based on weights (equal if not specified).
    #[allow(dead_code)]
    async fn execute_portfolio_backtest(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use backtest::portfolio_simulation::PortfolioSimulator;
        use backtest::realistic_simulation::RealisticASParams;
        
        let params = &job.params;
        let all_symbols = params.all_symbols();
        
        log_info!(BACKTEST_LOGGER, "Job {} starting portfolio backtest for {} symbols: {:?}", 
            job.id, all_symbols.len(), all_symbols);
        
        // Update phase
        let _ = self.job_queue.update_phase(&job.id, "portfolio_loading", serde_json::json!({
            "symbols": all_symbols,
            "exchange": params.exchange,
        })).await;
        
        // Load market data for all assets (symbol+exchange pairs)
        // Use asset_key (symbol@exchange) to allow same symbol on different exchanges
        let mut symbol_events = std::collections::HashMap::new();
        let mut asset_exchanges = std::collections::HashMap::new(); // Track exchange per asset key
        let symbol_exchange_pairs = params.all_symbol_exchange_pairs();
        
        for (idx, (symbol, exchange, weight)) in symbol_exchange_pairs.iter().enumerate() {
            let _ = self.job_queue.update_progress(&job.id, 5.0 + (idx as f64 * 10.0 / symbol_exchange_pairs.len() as f64)).await;
            
            // Use asset key to support same symbol on multiple exchanges
            let asset_key = crate::job_queue::BacktestJobParams::asset_key(symbol, exchange);
            
            log_debug!(BACKTEST_LOGGER, "Job {} loading data for asset {}/{}: {} (weight: {:.2}%)", 
                job.id, idx + 1, symbol_exchange_pairs.len(), asset_key, weight * 100.0);
            
            match self.load_market_data_for_strategy(symbol, exchange, &params.strategy_type).await {
                Ok(data) => {
                    if data.is_empty() {
                        log_warn!(BACKTEST_LOGGER, "Job {} no data for {}, skipping", job.id, asset_key);
                        continue;
                    }
                    
                    // Convert MarketData to OrderBookEvents
                    let events = backtest::ticks_to_trade_events(&data);
                    log_info!(BACKTEST_LOGGER, "Job {} loaded {} events for {}", job.id, events.len(), asset_key);
                    symbol_events.insert(asset_key.clone(), events);
                    asset_exchanges.insert(asset_key, exchange.clone());
                }
                Err(e) => {
                    log_error!(BACKTEST_LOGGER, "Job {} failed to load data for {}: {}", job.id, asset_key, e);
                    // Continue with other assets
                }
            }
        }
        
        if symbol_events.is_empty() {
            return Err("No market data available for any asset".into());
        }
        
        // Update phase
        let _ = self.job_queue.update_phase(&job.id, "portfolio_simulation", serde_json::json!({
            "assets_loaded": symbol_events.len(),
            "total_assets": symbol_exchange_pairs.len(),
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 30.0).await;
        
        // Get exchange fee config for fee rates
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&params.exchange);
        
        // Build simulation params from job params
        let sim_params = RealisticASParams {
            risk_aversion: params.risk_aversion,
            inventory_target: params.inventory_target,
            order_size_pct: params.order_size_pct,
            initial_inventory_pct: params.initial_inventory_pct,
            volatility_cap: params.volatility_cap,
            time_horizon_minutes: params.time_horizon_minutes as f64,
            min_spread_bps: params.min_spread_bps,
            max_inventory_multiple: 1.0,
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            fee_spread_multiplier: params.fee_spread_multiplier,
            adverse_selection_sensitivity: params.adverse_selection_sensitivity,
            take_profit_pct: params.take_profit_pct,
            stop_loss_pct: params.stop_loss_pct,
            max_trades_per_minute: params.max_trades_per_minute,
            max_orders_per_second: 10.0,
            rate_limit_cooldown_ms: 1000,
            window_ms: params.window_ms as i64,
            min_quote_lifetime_ms: params.min_quote_lifetime_ms as i64,
            fill_model: backtest::FillModelType::from_str_or_exchange(&params.fill_model, &params.exchange),
            ..Default::default()
        };
        
        // Create portfolio simulator
        let simulator = PortfolioSimulator::new(params.initial_capital, exchange_fee_config);
        
        // Run portfolio simulation
        let portfolio_result = simulator.run(
            &symbol_events,
            &sim_params,
            params.allocation_weights.as_ref(),
        );
        
        log_info!(BACKTEST_LOGGER, "Job {} portfolio simulation complete: {} symbols, total_pnl=${:.2}, sharpe={:.3}", 
            job.id, portfolio_result.symbol_results.len(), portfolio_result.total_pnl, portfolio_result.portfolio_sharpe);
        
        // Log per-symbol breakdown
        for sym_result in &portfolio_result.symbol_results {
            log_info!(BACKTEST_LOGGER, "  {} | PnL: ${:.2} | Sharpe: {:.3} | Trades: {} | Alloc: {:.1}%",
                sym_result.symbol, sym_result.total_pnl, sym_result.sharpe_ratio, 
                sym_result.num_trades, sym_result.allocation_weight * 100.0);
        }
        
        // Update progress
        let _ = self.job_queue.update_progress(&job.id, 100.0).await;
        let _ = self.job_queue.update_phase(&job.id, "completed", serde_json::json!({
            "symbols_traded": portfolio_result.symbol_results.len(),
            "total_pnl": portfolio_result.total_pnl,
            "portfolio_sharpe": portfolio_result.portfolio_sharpe,
            "total_trades": portfolio_result.total_trades,
        })).await;
        
        // Build per-symbol breakdown for inventory metrics
        let symbol_breakdown: Vec<serde_json::Value> = portfolio_result.symbol_results.iter()
            .map(|r| serde_json::json!({
                "symbol": r.symbol,
                "allocation_weight": r.allocation_weight,
                "allocated_capital": r.allocated_capital,
                "total_pnl": r.total_pnl,
                "sharpe_ratio": r.sharpe_ratio,
                "max_drawdown": r.max_drawdown,
                "num_trades": r.num_trades,
                "win_rate": r.win_rate,
                "ending_inventory": r.ending_inventory,
                "ending_inventory_value": r.ending_inventory_value,
                "ending_capital": r.ending_capital,
                "final_equity": r.final_equity,
            }))
            .collect();
        
        // Build PortfolioMetrics for the result
        // The symbol field in results is the asset_key (symbol@exchange)
        let portfolio_metrics = crate::job_queue::PortfolioMetrics {
            total_symbols: portfolio_result.symbol_results.len(),
            symbol_results: portfolio_result.symbol_results.iter().map(|r| {
                // Parse exchange from asset_key if it contains @
                let (symbol, exchange) = if r.symbol.contains('@') {
                    let parts: Vec<&str> = r.symbol.splitn(2, '@').collect();
                    (parts[0].to_string(), Some(parts[1].to_string()))
                } else {
                    // Legacy: look up exchange
                    let exch = params.get_exchange_for_symbol(&r.symbol).to_string();
                    (r.symbol.clone(), Some(exch))
                };
                
                crate::job_queue::SymbolResult {
                    symbol,
                    exchange,
                    sharpe_ratio: r.sharpe_ratio,
                    max_drawdown: r.max_drawdown,
                    total_pnl: r.total_pnl,
                    win_rate: r.win_rate,
                    profit_factor: r.profit_factor,
                    total_trades: r.num_trades,
                    allocation_weight: r.allocation_weight,
                    // Per-symbol position tracking
                    starting_capital: Some(r.starting_capital),
                    ending_capital: Some(r.ending_capital),
                    starting_inventory: Some(r.starting_inventory),
                    ending_inventory: Some(r.ending_inventory),
                    starting_inventory_value: Some(r.starting_inventory_value),
                    ending_inventory_value: Some(r.ending_inventory_value),
                    starting_mark_price: Some(r.starting_mark_price),
                    ending_mark_price: Some(r.ending_mark_price),
                }
            }).collect(),
            correlation_matrix: portfolio_result.correlation_matrix.as_ref().map(|corrs| {
                corrs.iter().map(|c| {
                    crate::job_queue::CorrelationPair {
                        symbol_a: c.symbol1.clone(),
                        symbol_b: c.symbol2.clone(),
                        correlation: c.correlation,
                    }
                }).collect()
            }).unwrap_or_default(),
            portfolio_sharpe: portfolio_result.portfolio_sharpe,
            portfolio_max_drawdown: portfolio_result.portfolio_max_drawdown,
            diversification_ratio: None, // TODO: Calculate based on individual vs portfolio volatility
        };
        
        // Calculate additional metrics from portfolio equity curve and trade returns
        let equity_curve = &portfolio_result.equity_curve;
        
        // Calculate annualized return from equity curve
        let annualized_return = if equity_curve.len() >= 2 {
            let initial = equity_curve.first().copied().unwrap_or(1.0);
            let final_val = equity_curve.last().copied().unwrap_or(1.0);
            if initial > 0.0 {
                let total_return = (final_val - initial) / initial;
                // Assuming each equity point is ~1 minute, calculate trading days
                let minutes = equity_curve.len() as f64;
                let trading_days = minutes / (6.5 * 60.0); // ~6.5 hours of trading per day
                if trading_days > 0.0 {
                    Some((1.0 + total_return).powf(252.0 / trading_days) - 1.0)
                } else {
                    Some(total_return)
                }
            } else {
                None
            }
        } else {
            None
        };
        
        // Calculate volatility from equity curve returns
        let volatility = if equity_curve.len() >= 2 {
            let mut returns = Vec::with_capacity(equity_curve.len() - 1);
            for i in 1..equity_curve.len() {
                if equity_curve[i - 1] > 0.0 {
                    returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
                }
            }
            if !returns.is_empty() {
                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
                let std = variance.sqrt();
                // Annualize: assume ~1 minute per sample, 252 trading days * 6.5 hours * 60 min
                let samples_per_year: f64 = 252.0 * 6.5 * 60.0;
                Some(std * samples_per_year.sqrt())
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        };
        
        // Calculate Sortino ratio (only penalizes downside volatility)
        let sortino_ratio = if equity_curve.len() >= 2 {
            let mut returns = Vec::with_capacity(equity_curve.len() - 1);
            for i in 1..equity_curve.len() {
                if equity_curve[i - 1] > 0.0 {
                    returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
                }
            }
            if !returns.is_empty() {
                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                let downside_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).cloned().collect();
                if downside_returns.is_empty() {
                    if mean > 0.0 { Some(10.0) } else { Some(0.0) } // All positive returns
                } else {
                    let downside_variance: f64 = downside_returns.iter().map(|r| r.powi(2)).sum::<f64>() / returns.len() as f64;
                    let downside_std = downside_variance.sqrt();
                    if downside_std > 1e-10 {
                        let samples_per_year: f64 = 252.0 * 6.5 * 60.0;
                        Some((mean / downside_std) * samples_per_year.sqrt())
                    } else {
                        Some(0.0)
                    }
                }
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        };
        
        // Calculate Calmar ratio (annualized return / max drawdown)
        let calmar_ratio = match (annualized_return, portfolio_result.portfolio_max_drawdown) {
            (Some(ret), dd) if dd > 0.001 => Some(ret / dd),
            (Some(ret), _) if ret > 0.0 => Some(10.0), // No drawdown, positive return
            _ => Some(0.0),
        };
        
        // Get best and worst trade from trade returns
        let best_trade = portfolio_result.trade_returns.iter().cloned().fold(None, |max: Option<f64>, r| {
            Some(max.map_or(r, |m| m.max(r)))
        });
        let worst_trade = portfolio_result.trade_returns.iter().cloned().fold(None, |min: Option<f64>, r| {
            Some(min.map_or(r, |m| m.min(r)))
        });
        
        // Calculate average trade return
        let avg_trade_return = if !portfolio_result.trade_returns.is_empty() {
            Some(portfolio_result.trade_returns.iter().sum::<f64>() / portfolio_result.trade_returns.len() as f64)
        } else {
            None
        };
        
        // VaR and ES from portfolio risk metrics if available
        let value_at_risk_95 = portfolio_result.risk_metrics.as_ref().map(|rm| rm.portfolio_var_95);
        let expected_shortfall_95 = portfolio_result.risk_metrics.as_ref().map(|rm| rm.portfolio_es_95);
        
        // Convert PortfolioSimResult to job_queue::BacktestResult
        let backtest_result = BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some("portfolio".to_string()),
            total_pnl: portfolio_result.total_pnl,
            sharpe_ratio: portfolio_result.portfolio_sharpe,
            max_drawdown: portfolio_result.portfolio_max_drawdown,
            win_rate: portfolio_result.portfolio_win_rate,
            total_trades: portfolio_result.total_trades,
            profit_factor: portfolio_result.portfolio_profit_factor,
            equity_curve: portfolio_result.equity_curve,
            completed_at: Utc::now(),
            closed_trades: Some(portfolio_result.total_trades),
            open_positions: None,
            realized_pnl: Some(portfolio_result.total_pnl),
            unrealized_pnl: Some(0.0),
            // Additional calculated metrics
            annualized_return,
            volatility,
            sortino_ratio,
            calmar_ratio,
            avg_trade_return,
            best_trade,
            worst_trade,
            avg_time_in_trade: Some(portfolio_result.avg_time_in_trade_ms / 1000.0), // Convert ms to seconds
            value_at_risk_95,
            expected_shortfall_95,
            // Execution metrics - use actual tracked values
            total_orders: Some(portfolio_result.total_orders_placed as i32),
            filled_orders: Some(portfolio_result.total_orders_filled as i32),
            cancelled_orders: Some((portfolio_result.total_orders_placed - portfolio_result.total_orders_filled) as i32),
            avg_slippage: None,
            total_commission_paid: Some(portfolio_result.total_fees),
            // Portfolio specific
            is_portfolio: true,
            portfolio_metrics: Some(portfolio_metrics),
            strategy_params: Some(serde_json::json!({
                "portfolio_mode": true,
                "symbols": all_symbols,
                "symbol_results": symbol_breakdown,
                "initial_capital": params.initial_capital,
                "correlation_matrix": portfolio_result.correlation_matrix,
            })),
            // Date range - use actual data timestamps with fallback to job params
            start_date: portfolio_result.first_event_timestamp_ms
                .map(|ms| chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default())
                .filter(|s| !s.is_empty())
                .or_else(|| job.params.start_date.clone()),
            end_date: portfolio_result.last_event_timestamp_ms
                .map(|ms| chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default())
                .filter(|s| !s.is_empty())
                .or_else(|| job.params.end_date.clone()),
            ..Default::default()
        };
        
        Ok(backtest_result)
    }
    
    /// Execute portfolio backtest with GA optimization
    /// 
    /// Phase 1: Per-symbol GA optimization (parallel)
    /// Phase 2: Allocation weight optimization  
    /// Phase 3: Final portfolio simulation with optimized params
    async fn execute_portfolio_backtest_with_ga(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        // Check if this is a momentum strategy - route to momentum portfolio GA
        let strategy_type = job.params.strategy_type.to_lowercase();
        let is_momentum = strategy_type.contains("momentum") || strategy_type.contains("trend");
        
        if is_momentum {
            eprintln!("🚀 [PORTFOLIO_GA] Dispatching to MOMENTUM portfolio optimization (strategy_type={})", job.params.strategy_type);
            return self.execute_momentum_portfolio_ga_optimization(job).await;
        }
        
        // Default: Avellaneda-Stoikov market making portfolio optimization
        use genetic::{
            PortfolioGAOptimizer, PortfolioFitnessConfig, 
            GeneticOptimizer, FitnessResult, AsyncFitnessFn,
        };
        use genetic::chromosome::AvellanedaStoikovChromosome;
        use backtest::portfolio_simulation::PortfolioSimulator;
        use backtest::realistic_simulation::RealisticASParams;
        use config::GeneticConfig;
        
        // === PORTFOLIO GA EXECUTION LOGGING ===
        eprintln!("\n============================================================");
        eprintln!("🧬 [PORTFOLIO_GA] STARTING job_id={}", job.id);
        eprintln!("============================================================");
        
        let params = &job.params;
        let symbol_exchange_pairs = params.all_symbol_exchange_pairs();
        let num_symbols = symbol_exchange_pairs.len();
        
        eprintln!("   symbol_exchange_pairs: {:?}", symbol_exchange_pairs);
        eprintln!("   num_symbols: {}", num_symbols);
        eprintln!("   population_size: {}", params.population_size);
        eprintln!("   generations: {}", params.generations);
        
        log_info!(MAIN_LOGGER, "🧬 [{}] Portfolio GA: Starting optimization for {} symbols", 
            job.id, num_symbols);
        
        // Update phase
        let _ = self.job_queue.update_phase(&job.id, "portfolio_ga_loading", serde_json::json!({
            "phase": "data_loading",
            "symbols": num_symbols,
        })).await;
        
        // ========== PHASE 0: Load market data for all symbols ==========
        let mut symbol_data: std::collections::HashMap<String, Vec<dataloader::MarketData>> = std::collections::HashMap::new();
        let mut symbol_events: std::collections::HashMap<String, Vec<backtest::realistic_simulation::OrderBookEvent>> = std::collections::HashMap::new();
        
        for (idx, (symbol, exchange, _weight)) in symbol_exchange_pairs.iter().enumerate() {
            let asset_key = crate::job_queue::BacktestJobParams::asset_key(symbol, exchange);
            let _ = self.job_queue.update_progress(&job.id, 5.0 + (idx as f64 * 10.0 / num_symbols as f64)).await;
            
            log_info!(MAIN_LOGGER, "📊 [{}] Loading data for {}", job.id, asset_key);
            
            match self.load_market_data_for_strategy(symbol, exchange, &params.strategy_type).await {
                Ok(data) => {
                    if data.is_empty() {
                        log_warn!(MAIN_LOGGER, "[{}] No data for {}, skipping", job.id, asset_key);
                        continue;
                    }
                    let events = backtest::ticks_to_trade_events(&data);
                    log_info!(MAIN_LOGGER, "[{}] Loaded {} events for {}", job.id, events.len(), asset_key);
                    symbol_data.insert(asset_key.clone(), data);
                    symbol_events.insert(asset_key, events);
                }
                Err(e) => {
                    log_error!(MAIN_LOGGER, "[{}] Failed to load {}: {}", job.id, asset_key, e);
                }
            }
        }
        
        if symbol_events.is_empty() {
            return Err("No market data available for any symbol".into());
        }
        
        let symbols: Vec<String> = symbol_events.keys().cloned().collect();
        let actual_num_symbols = symbols.len();
        
        // ========== TRAIN/TEST SPLIT FOR PORTFOLIO MODE ==========
        // Apply the same train/test split logic as single-symbol mode
        // Train data is used for GA optimization, test data for OOS validation
        let train_ratio = params.train_ratio;
        let test_ratio = params.test_ratio;
        
        let mut symbol_train_data: std::collections::HashMap<String, Vec<dataloader::MarketData>> = std::collections::HashMap::new();
        let mut symbol_test_data: std::collections::HashMap<String, Vec<dataloader::MarketData>> = std::collections::HashMap::new();
        let mut symbol_train_events: std::collections::HashMap<String, Vec<backtest::realistic_simulation::OrderBookEvent>> = std::collections::HashMap::new();
        let mut symbol_test_events: std::collections::HashMap<String, Vec<backtest::realistic_simulation::OrderBookEvent>> = std::collections::HashMap::new();
        
        for (asset_key, data) in &symbol_data {
            let n = data.len();
            let train_end = ((n as f64) * train_ratio).ceil() as usize;
            let test_end = ((n as f64) * (train_ratio + test_ratio)).ceil() as usize;
            
            let train_data: Vec<dataloader::MarketData> = data[..train_end].to_vec();
            let test_data: Vec<dataloader::MarketData> = data[train_end..test_end.min(n)].to_vec();
            
            let train_events = backtest::ticks_to_trade_events(&train_data);
            let test_events = backtest::ticks_to_trade_events(&test_data);
            
            log_info!(MAIN_LOGGER, "📊 [{}] {} split: train={} ({:.0}%), test={} ({:.0}%)", 
                job.id, asset_key, train_data.len(), train_ratio * 100.0, 
                test_data.len(), test_ratio * 100.0);
            
            symbol_train_data.insert(asset_key.clone(), train_data);
            symbol_test_data.insert(asset_key.clone(), test_data);
            symbol_train_events.insert(asset_key.clone(), train_events);
            symbol_test_events.insert(asset_key.clone(), test_events);
        }
        
        // ========== PHASE 1: Per-symbol GA optimization (parallel) ==========
        let _ = self.job_queue.update_phase(&job.id, "portfolio_ga_symbol_optimization", serde_json::json!({
            "phase": "per_symbol_ga",
            "total_symbols": actual_num_symbols,
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 15.0).await;
        
        log_info!(MAIN_LOGGER, "🧬 [{}] Phase 1: Starting parallel per-symbol GA optimization (using training data only)", job.id);
        
        // GA config for per-symbol optimization
        let ga_config = GeneticConfig {
            population_size: params.population_size,
            generations: params.generations,
            mutation_rate: 0.1,
            crossover_rate: 0.7,
            ..Default::default()
        };
        
        // Create fitness config
        let fitness_config = PortfolioFitnessConfig::default();
        
        // Create portfolio optimizer
        let _portfolio_optimizer = PortfolioGAOptimizer::new(
            ga_config.clone(),
            fitness_config.clone(),
            job.id.clone(),
        );
        
        // Create fitness function factory for each symbol - use TRAINING data only
        let symbol_train_data_arc = std::sync::Arc::new(symbol_train_data.clone());
        let initial_capital = params.initial_capital;
        let _commission_rate = params.commission_rate;
        let exchange = params.exchange.clone();
        
        // Run parallel per-symbol optimization
        let symbol_results = {
            use tokio::task::JoinSet;
            
            let mut join_set: JoinSet<Result<genetic::SymbolOptimizationResult, Box<dyn std::error::Error + Send + Sync>>> = JoinSet::new();
            
            for asset_key in symbols.iter() {
                let asset_key = asset_key.clone();
                let symbol_train_data_arc = std::sync::Arc::clone(&symbol_train_data_arc);
                let ga_config = ga_config.clone();
                let job_id = job.id.clone();
                let exchange = exchange.clone();
                
                join_set.spawn(async move {
                    // Use TRAINING data only for GA optimization
                    let data = symbol_train_data_arc.get(&asset_key)
                        .ok_or_else(|| format!("No training data for {}", asset_key))?;
                    
                    // Convert to events for realistic simulation
                    let events = backtest::ticks_to_trade_events(data);
                    let events_arc = std::sync::Arc::new(events);
                    
                    eprintln!("🧬 [{}] GA for {} with {} TRAINING events (using RealisticSimulator)", job_id, asset_key, events_arc.len());
                    
                    // Get exchange fee config
                    let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&exchange);
                    
                    // Create fitness function using RealisticSimulator (same as final evaluation)
                    let fitness_fn: AsyncFitnessFn<AvellanedaStoikovChromosome> = {
                        let events = std::sync::Arc::clone(&events_arc);
                        let fee_config = exchange_fee_config.clone();
                        
                        std::sync::Arc::new(move |chr: &AvellanedaStoikovChromosome| {
                            let events = std::sync::Arc::clone(&events);
                            let chr = chr.clone();
                            let fee_config = fee_config.clone();
                            
                            Box::pin(async move {
                                // Use RealisticSimulator for GA fitness (same as final evaluation)
                                let sim_params = RealisticASParams {
                                    risk_aversion: chr.risk_aversion,
                                    inventory_target: chr.inventory_target,
                                    order_size_pct: chr.order_size_pct,
                                    initial_inventory_pct: 0.0,
                                    volatility_cap: chr.volatility_cap,
                                    time_horizon_minutes: chr.time_horizon_minutes,
                                    min_spread_bps: chr.min_spread_bps,
                                    max_inventory_multiple: 1.0,
                                    maker_fee_rate: fee_config.maker_fee,
                                    taker_fee_rate: fee_config.taker_fee,
                                    fee_spread_multiplier: chr.fee_spread_multiplier,
                                    adverse_selection_sensitivity: chr.adverse_selection_sensitivity,
                                    take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
                                    stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
                                    max_trades_per_minute: chr.max_trades_per_minute,
                                    max_orders_per_second: 10.0,
                                    rate_limit_cooldown_ms: 1000,
                                    window_ms: chr.window_ms as i64,
                                    min_quote_lifetime_ms: chr.min_quote_lifetime_ms as i64,
                                    ..Default::default()
                                };
                                
                                let simulator = backtest::RealisticSimulator::with_exchange_config(
                                    initial_capital,
                                    fee_config,
                                );
                                let sim_result = simulator.run(&events, &sim_params);
                                
                                // ==============================================
                                // MARKET MAKING FITNESS FUNCTION
                                // ==============================================
                                // Prioritizes what makes MM strategies actually profitable:
                                // 1. Net profit after ALL costs (commissions are the enemy)
                                // 2. Consistency (many small wins > few big wins)
                                // 3. Inventory control (don't take directional bets)
                                // 4. Risk-adjusted returns (survive bad periods)
                                
                                let num_trades = sim_result.num_trades as f64;
                                
                                // ==============================================
                                // AGGRESSIVE HARD REJECT - MARKET MAKING VIABILITY
                                // ==============================================
                                // Only allow chromosomes that meet ALL viable MM criteria.
                                // Fail fast = faster evolution toward profitable strategies.
                                
                                let gross_profit = sim_result.trade_returns.iter()
                                    .filter(|&&r| r > 0.0).sum::<f64>();
                                let commission_ratio = if gross_profit > 0.0 {
                                    sim_result.total_fees / gross_profit
                                } else {
                                    1.0
                                };
                                let ending_inv_exposure = sim_result.inventory_metrics.ending_inventory_value.abs() / initial_capital;
                                
                                // 1. Minimum trades for statistical significance (MM should trade frequently)
                                if sim_result.num_trades < 1000 {
                                    return FitnessResult::new(-10.0, sim_result.equity_curve);
                                }
                                
                                // 2. Win rate >= 51% (MM MUST have edge, not get picked off)
                                if sim_result.win_rate < 0.51 {
                                    return FitnessResult::new(-8.0, sim_result.equity_curve);
                                }
                                
                                // 3. Profit factor >= 1.2 (must have clear edge after costs)
                                if sim_result.profit_factor < 1.2 {
                                    return FitnessResult::new(-7.0, sim_result.equity_curve);
                                }
                                
                                // 4. Sharpe ratio >= 1.0 (MM should have excellent risk-adjusted returns)
                                if sim_result.sharpe_ratio < 1.0 {
                                    return FitnessResult::new(-6.5, sim_result.equity_curve);
                                }
                                
                                // 5. Max drawdown <= 20% (risk control is non-negotiable)
                                if sim_result.max_drawdown > 0.20 {
                                    return FitnessResult::new(-6.0, sim_result.equity_curve);
                                }
                                
                                // 6. Commission ratio <= 60% (fees can't eat most of profit)
                                if commission_ratio > 0.60 {
                                    return FitnessResult::new(-5.0, sim_result.equity_curve);
                                }
                                
                                // 7. Inventory exposure <= 10% (MM must stay flat, not take directional bets)
                                if ending_inv_exposure > 0.10 {
                                    return FitnessResult::new(-4.0, sim_result.equity_curve);
                                }
                                
                                // 8. Must be profitable (this is the whole point)
                                if sim_result.total_pnl <= 0.0 {
                                    return FitnessResult::new(-3.0, sim_result.equity_curve);
                                }
                                
                                // 9. Avg profit per trade >= 2× avg commission (edge must be robust)
                                let avg_profit_per_trade = sim_result.total_pnl / sim_result.num_trades as f64;
                                let avg_commission_per_trade = sim_result.total_fees / sim_result.num_trades as f64;
                                if avg_profit_per_trade < 2.0 * avg_commission_per_trade {
                                    return FitnessResult::new(-2.5, sim_result.equity_curve);
                                }
                                
                                // ==============================================
                                // PASSED ALL GATES - Now rank by quality
                                // ==============================================
                                // At this point we have a VIABLE strategy. Rank by how good it is.
                                
                                let num_trades = sim_result.num_trades as f64;
                                let net_profit_pct = sim_result.total_pnl / initial_capital;
                                
                                // Profit score (primary driver - we want maximum profit)
                                let profit_score = (net_profit_pct * 10.0).tanh();  // 10% return = 0.76, 20% = 0.96
                                
                                // Sharpe bonus (risk-adjusted, capped at 3)
                                let sharpe_score = (sim_result.sharpe_ratio.max(0.0) / 3.0).min(1.0);
                                
                                // Win rate bonus (baseline at 51%)
                                let win_rate_bonus = (sim_result.win_rate - 0.51) * 2.0;  // 61% WR = 0.2 bonus
                                
                                // Profit factor bonus (baseline at 1.2)
                                let pf_bonus = ((sim_result.profit_factor - 1.2) / 1.8).min(1.0);  // PF 2.0 = 0.44 bonus
                                
                                // Trade count bonus (more trades = more confidence, log scale)
                                let trade_bonus = (num_trades.ln() - 3.4) / 4.0;  // 30 trades = 0, 200 = 0.43
                                
                                // Low drawdown bonus
                                let dd_bonus = (0.20 - sim_result.max_drawdown) * 2.0;  // 5% DD = 0.30 bonus
                                
                                // Commission efficiency bonus
                                let comm_bonus = (0.60 - commission_ratio) / 0.60;  // 30% ratio = 0.5 bonus
                                
                                // Inventory management bonus (tighter = better)
                                let inv_bonus = (0.10 - ending_inv_exposure) / 0.10;  // 10% exposure = 0.67 bonus
                                
                                // === FINAL FITNESS ===
                                // Heavily weight profit, then consistency, then risk metrics
                                let fitness = 
                                    profit_score * 0.40 +       // 40% raw profitability
                                    sharpe_score * 0.15 +       // 15% risk-adjusted return
                                    pf_bonus * 0.15 +           // 15% profit factor
                                    win_rate_bonus * 0.10 +     // 10% win rate
                                    trade_bonus * 0.05 +        // 5% statistical confidence
                                    dd_bonus * 0.05 +           // 5% drawdown control
                                    comm_bonus * 0.05 +         // 5% commission efficiency
                                    inv_bonus * 0.05;           // 5% inventory management
                                
                                FitnessResult::with_metrics(
                                    fitness,
                                    sim_result.equity_curve,
                                    sim_result.total_pnl,
                                    sim_result.sharpe_ratio,
                                    sim_result.num_trades as i32,
                                    sim_result.max_drawdown,
                                    sim_result.win_rate,
                                    sim_result.profit_factor,
                                )
                            })
                        })
                    };
                    
                    // Run GA for this symbol
                    let optimizer = GeneticOptimizer::<AvellanedaStoikovChromosome> {
                        config: ga_config,
                        fitness_fn,
                        progress_callback: None,
                        job_id: Some(format!("{}-{}", job_id, asset_key)),
                    };
                    
                    let (best_chromosome, best_fitness) = optimizer.run().await;
                    
                    // Get final metrics
                    let final_result = (optimizer.fitness_fn)(&best_chromosome).await;
                    
                    eprintln!("✅ [{}] {} complete: fitness={:.4}, sharpe={:.3}, pnl=${:.2}", 
                        job_id, asset_key, best_fitness, final_result.sharpe_ratio, final_result.net_pnl);
                    
                    Ok(genetic::SymbolOptimizationResult {
                        asset_key,
                        chromosome: best_chromosome,
                        fitness: best_fitness,
                        sharpe_ratio: final_result.sharpe_ratio,
                        net_pnl: final_result.net_pnl,
                        num_trades: final_result.num_trades,
                        max_drawdown: final_result.max_drawdown,
                    })
                });
            }
            
            // Collect results
            let mut results = Vec::with_capacity(actual_num_symbols);
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok(opt_result)) => results.push(opt_result),
                    Ok(Err(e)) => {
                        log_error!(MAIN_LOGGER, "[{}] Symbol optimization error: {}", job.id, e);
                        return Err(e);
                    }
                    Err(e) => {
                        log_error!(MAIN_LOGGER, "[{}] Join error: {}", job.id, e);
                        return Err(format!("Task join error: {}", e).into());
                    }
                }
            }
            results
        };
        
        let _ = self.job_queue.update_progress(&job.id, 60.0).await;
        
        // Build chromosome map
        let mut symbol_chromosomes: std::collections::HashMap<String, AvellanedaStoikovChromosome> = 
            std::collections::HashMap::new();
        for result in &symbol_results {
            symbol_chromosomes.insert(result.asset_key.clone(), result.chromosome.clone());
        }
        
        // Guard: Ensure we have at least one optimized chromosome
        if symbol_chromosomes.is_empty() {
            return Err("No symbols were successfully optimized".into());
        }
        
        // ========== PHASE 2: Allocation weight optimization ==========
        let _ = self.job_queue.update_phase(&job.id, "portfolio_ga_weight_optimization", serde_json::json!({
            "phase": "weight_optimization",
            "symbols": actual_num_symbols,
        })).await;
        
        log_info!(MAIN_LOGGER, "🧬 [{}] Phase 2: Optimizing allocation weights", job.id);
        
        // Get exchange fee config
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&params.exchange);
        
        // Run weight optimization GA
        let optimized_weights = {
            use genetic::AllocationChromosomeFactory;
            use rand::prelude::*;
            
            let factory = AllocationChromosomeFactory::new(actual_num_symbols);
            let mut rng = rand::rngs::StdRng::from_entropy();
            
            let weight_pop_size = 30;
            let weight_generations = 15;
            
            let mut population: Vec<genetic::AllocationChromosome> = (0..weight_pop_size)
                .map(|_| factory.random(&mut rng))
                .collect();
            
            let mut best_weights = vec![1.0 / actual_num_symbols as f64; actual_num_symbols];
            let mut best_fitness = f64::NEG_INFINITY;
            
            let symbols_ordered: Vec<String> = symbol_results.iter().map(|r| r.asset_key.clone()).collect();
            
            for gen in 0..weight_generations {
                let mut fitness_results: Vec<(genetic::AllocationChromosome, f64)> = Vec::new();
                
                for chr in &population {
                    let weights = chr.normalized_weights();
                    
                    // Run portfolio simulation with these weights and optimized params
                    // Use TRAINING data only for weight optimization
                    let mut symbol_events_for_sim: std::collections::HashMap<String, Vec<backtest::realistic_simulation::OrderBookEvent>> = 
                        std::collections::HashMap::new();
                    
                    for asset_key in &symbols_ordered {
                        if let Some(data) = symbol_train_data.get(asset_key) {
                            symbol_events_for_sim.insert(asset_key.clone(), backtest::ticks_to_trade_events(data));
                        }
                    }
                    
                    // Build sim params from best chromosome (use first symbol's as base)
                    // Safe: guarded by symbol_chromosomes.is_empty() check above
                    let base_chr = symbol_chromosomes.values().next()
                        .expect("symbol_chromosomes checked non-empty above");
                    let sim_params = RealisticASParams {
                        risk_aversion: base_chr.risk_aversion,
                        inventory_target: base_chr.inventory_target,
                        order_size_pct: base_chr.order_size_pct,
                        initial_inventory_pct: params.initial_inventory_pct,
                        volatility_cap: base_chr.volatility_cap,
                        time_horizon_minutes: base_chr.time_horizon_minutes,
                        min_spread_bps: base_chr.min_spread_bps,
                        max_inventory_multiple: 1.0,
                        maker_fee_rate: exchange_fee_config.maker_fee,
                        taker_fee_rate: exchange_fee_config.taker_fee,
                        fee_spread_multiplier: base_chr.fee_spread_multiplier,
                        adverse_selection_sensitivity: base_chr.adverse_selection_sensitivity,
                        take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
                        stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
                        max_trades_per_minute: base_chr.max_trades_per_minute,
                        max_orders_per_second: 10.0,
                        rate_limit_cooldown_ms: 1000,
                        window_ms: base_chr.window_ms as i64,
                        min_quote_lifetime_ms: base_chr.min_quote_lifetime_ms as i64,
                        fill_model: backtest::FillModelType::from_str_or_exchange(&params.fill_model, &params.exchange),
                        ..Default::default()
                    };
                    
                    let simulator = PortfolioSimulator::new(initial_capital, exchange_fee_config.clone());
                    let portfolio_result = simulator.run(
                        &symbol_events_for_sim,
                        &sim_params,
                        Some(&weights),
                    );
                    
                    // Calculate ending inventory exposure for fitness penalty
                    let ending_inventory_exposure: f64 = portfolio_result.symbol_results.iter()
                        .map(|r| r.ending_inventory_value.abs())
                        .sum();
                    
                    // Calculate fitness
                    let fitness = fitness_config.calculate_fitness(
                        portfolio_result.portfolio_sharpe,
                        None, // sortino
                        portfolio_result.total_pnl,
                        portfolio_result.portfolio_profit_factor,
                        portfolio_result.portfolio_win_rate,
                        portfolio_result.portfolio_max_drawdown,
                        None, // diversification
                        None, // calmar
                        portfolio_result.total_trades,
                        actual_num_symbols,
                        Some(portfolio_result.avg_fill_rate),
                        Some(ending_inventory_exposure),
                        initial_capital,
                    );
                    
                    fitness_results.push((chr.clone(), fitness));
                }
                
                // Sort by fitness
                fitness_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                
                // Update best
                if let Some((best_chr, fitness)) = fitness_results.first() {
                    if *fitness > best_fitness {
                        best_fitness = *fitness;
                        best_weights = best_chr.normalized_weights();
                        eprintln!("📊 [{}] Weight Gen {}: fitness={:.4}, weights={:?}", 
                            job.id, gen + 1, best_fitness, best_weights);
                    }
                }
                
                // Next generation
                let elite_size = 3;
                let mut next_pop: Vec<genetic::AllocationChromosome> = fitness_results.iter()
                    .take(elite_size)
                    .map(|(chr, _)| chr.clone())
                    .collect();
                
                while next_pop.len() < weight_pop_size {
                    let idx1 = rng.gen_range(0..fitness_results.len().min(10));
                    let idx2 = rng.gen_range(0..fitness_results.len().min(10));
                    let mut child = genetic::Chromosome::crossover(&fitness_results[idx1].0, &fitness_results[idx2].0, &mut rng);
                    genetic::Chromosome::mutate(&mut child, &mut rng, 0.15);
                    child.enforce_min_weight(0.05);
                    next_pop.push(child);
                }
                
                population = next_pop;
            }
            
            best_weights
        };
        
        log_info!(MAIN_LOGGER, "✅ [{}] Optimized weights: {:?}", job.id, optimized_weights);
        
        let _ = self.job_queue.update_progress(&job.id, 80.0).await;
        
        // ========== PHASE 3: Final portfolio simulation on OUT-OF-SAMPLE test data ==========
        let _ = self.job_queue.update_phase(&job.id, "portfolio_final_simulation", serde_json::json!({
            "phase": "final_simulation_oos",
            "optimized_weights": &optimized_weights,
            "data_type": "test_data_oos",
        })).await;
        
        log_info!(MAIN_LOGGER, "🧬 [{}] Phase 3: Final portfolio simulation on OUT-OF-SAMPLE test data", job.id);
        
        // Run final simulation with optimized params and weights
        // Safe: guarded by symbol_chromosomes.is_empty() check above
        let base_chr = symbol_chromosomes.values().next()
            .expect("symbol_chromosomes checked non-empty above");
        let sim_params = RealisticASParams {
            risk_aversion: base_chr.risk_aversion,
            inventory_target: base_chr.inventory_target,
            order_size_pct: base_chr.order_size_pct,
            initial_inventory_pct: params.initial_inventory_pct,
            volatility_cap: base_chr.volatility_cap,
            time_horizon_minutes: base_chr.time_horizon_minutes,
            min_spread_bps: base_chr.min_spread_bps,
            max_inventory_multiple: 1.0,
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            fee_spread_multiplier: base_chr.fee_spread_multiplier,
            adverse_selection_sensitivity: base_chr.adverse_selection_sensitivity,
            take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
            stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
            max_trades_per_minute: base_chr.max_trades_per_minute,
            max_orders_per_second: 10.0,
            rate_limit_cooldown_ms: 1000,
            window_ms: base_chr.window_ms as i64,
            min_quote_lifetime_ms: base_chr.min_quote_lifetime_ms as i64,
            fill_model: backtest::FillModelType::from_str_or_exchange(&params.fill_model, &params.exchange),
            ..Default::default()
        };
        
        let simulator = PortfolioSimulator::new(initial_capital, exchange_fee_config);
        
        // Run final simulation on OUT-OF-SAMPLE test data
        let portfolio_result = simulator.run(
            &symbol_test_events,
            &sim_params,
            Some(&optimized_weights),
        );
        
        log_info!(MAIN_LOGGER, "✅ [{}] Portfolio GA complete (OOS): pnl=${:.2}, sharpe={:.3}, trades={}", 
            job.id, portfolio_result.total_pnl, portfolio_result.portfolio_sharpe, portfolio_result.total_trades);
        
        // Build portfolio metrics
        let symbols_ordered: Vec<String> = symbol_results.iter().map(|r| r.asset_key.clone()).collect();
        let portfolio_metrics = crate::job_queue::PortfolioMetrics {
            total_symbols: actual_num_symbols,
            symbol_results: portfolio_result.symbol_results.iter().enumerate().map(|(idx, r)| {
                let (symbol, exchange) = if r.symbol.contains('@') {
                    let parts: Vec<&str> = r.symbol.splitn(2, '@').collect();
                    (parts[0].to_string(), Some(parts[1].to_string()))
                } else {
                    (r.symbol.clone(), Some(params.exchange.clone()))
                };
                
                crate::job_queue::SymbolResult {
                    symbol,
                    exchange,
                    sharpe_ratio: r.sharpe_ratio,
                    max_drawdown: r.max_drawdown,
                    total_pnl: r.total_pnl,
                    win_rate: r.win_rate,
                    profit_factor: r.profit_factor,
                    total_trades: r.num_trades,
                    allocation_weight: optimized_weights.get(idx).copied().unwrap_or(1.0 / actual_num_symbols as f64),
                    // Per-symbol position tracking
                    starting_capital: Some(r.starting_capital),
                    ending_capital: Some(r.ending_capital),
                    starting_inventory: Some(r.starting_inventory),
                    ending_inventory: Some(r.ending_inventory),
                    starting_inventory_value: Some(r.starting_inventory_value),
                    ending_inventory_value: Some(r.ending_inventory_value),
                    starting_mark_price: Some(r.starting_mark_price),
                    ending_mark_price: Some(r.ending_mark_price),
                }
            }).collect(),
            correlation_matrix: portfolio_result.correlation_matrix.as_ref().map(|corrs| {
                corrs.iter().map(|c| {
                    crate::job_queue::CorrelationPair {
                        symbol_a: c.symbol1.clone(),
                        symbol_b: c.symbol2.clone(),
                        correlation: c.correlation,
                    }
                }).collect()
            }).unwrap_or_default(),
            portfolio_sharpe: portfolio_result.portfolio_sharpe,
            portfolio_max_drawdown: portfolio_result.portfolio_max_drawdown,
            diversification_ratio: None,
        };
        
        // Build per-symbol optimization details
        let symbol_optimization_details: Vec<serde_json::Value> = symbol_results.iter()
            .enumerate()
            .map(|(idx, r)| {
                let chr = &r.chromosome;
                serde_json::json!({
                    "asset_key": r.asset_key,
                    "fitness": r.fitness,
                    "sharpe_ratio": r.sharpe_ratio,
                    "net_pnl": r.net_pnl,
                    "num_trades": r.num_trades,
                    "max_drawdown": r.max_drawdown,
                    "allocation_weight": optimized_weights.get(idx).copied().unwrap_or(0.0),
                    "optimized_params": {
                        "risk_aversion": chr.risk_aversion,
                        "order_size_pct": chr.order_size_pct,
                        "min_spread_bps": chr.min_spread_bps,
                        "volatility_cap": chr.volatility_cap,
                        "time_horizon_minutes": chr.time_horizon_minutes,
                        "inventory_target": chr.inventory_target,
                        "adverse_selection_sensitivity": chr.adverse_selection_sensitivity,
                        "fee_spread_multiplier": chr.fee_spread_multiplier,
                    }
                })
            })
            .collect();
        
        // Calculate additional metrics from portfolio equity curve and trade returns
        let equity_curve = &portfolio_result.equity_curve;
        
        // Calculate annualized return from equity curve
        let annualized_return = if equity_curve.len() >= 2 {
            let initial = equity_curve.first().copied().unwrap_or(1.0);
            let final_val = equity_curve.last().copied().unwrap_or(1.0);
            if initial > 0.0 {
                let total_return = (final_val - initial) / initial;
                let minutes = equity_curve.len() as f64;
                let trading_days = minutes / (6.5 * 60.0);
                if trading_days > 0.0 {
                    Some((1.0 + total_return).powf(252.0 / trading_days) - 1.0)
                } else {
                    Some(total_return)
                }
            } else {
                None
            }
        } else {
            None
        };
        
        // Calculate volatility from equity curve returns
        let volatility = if equity_curve.len() >= 2 {
            let mut returns = Vec::with_capacity(equity_curve.len() - 1);
            for i in 1..equity_curve.len() {
                if equity_curve[i - 1] > 0.0 {
                    returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
                }
            }
            if !returns.is_empty() {
                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
                let std = variance.sqrt();
                let samples_per_year: f64 = 252.0 * 6.5 * 60.0;
                Some(std * samples_per_year.sqrt())
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        };
        
        // Calculate Sortino ratio (only penalizes downside volatility)
        let sortino_ratio = if equity_curve.len() >= 2 {
            let mut returns = Vec::with_capacity(equity_curve.len() - 1);
            for i in 1..equity_curve.len() {
                if equity_curve[i - 1] > 0.0 {
                    returns.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
                }
            }
            if !returns.is_empty() {
                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                let downside_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).cloned().collect();
                if downside_returns.is_empty() {
                    if mean > 0.0 { Some(10.0) } else { Some(0.0) }
                } else {
                    let downside_variance: f64 = downside_returns.iter().map(|r| r.powi(2)).sum::<f64>() / returns.len() as f64;
                    let downside_std = downside_variance.sqrt();
                    if downside_std > 1e-10 {
                        let samples_per_year: f64 = 252.0 * 6.5 * 60.0;
                        Some((mean / downside_std) * samples_per_year.sqrt())
                    } else {
                        Some(0.0)
                    }
                }
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        };
        
        // Calculate Calmar ratio (annualized return / max drawdown)
        let calmar_ratio = match (annualized_return, portfolio_result.portfolio_max_drawdown) {
            (Some(ret), dd) if dd > 0.001 => Some(ret / dd),
            (Some(ret), _) if ret > 0.0 => Some(10.0),
            _ => Some(0.0),
        };
        
        // Get best and worst trade from trade returns
        let best_trade = portfolio_result.trade_returns.iter().cloned().fold(None, |max: Option<f64>, r| {
            Some(max.map_or(r, |m| m.max(r)))
        });
        let worst_trade = portfolio_result.trade_returns.iter().cloned().fold(None, |min: Option<f64>, r| {
            Some(min.map_or(r, |m| m.min(r)))
        });
        
        // Calculate average trade return
        let avg_trade_return = if !portfolio_result.trade_returns.is_empty() {
            Some(portfolio_result.trade_returns.iter().sum::<f64>() / portfolio_result.trade_returns.len() as f64)
        } else {
            None
        };
        
        // VaR and ES will be populated from Monte Carlo if available (see below)
        // Initialize from portfolio risk metrics as fallback
        let mut value_at_risk_95 = portfolio_result.risk_metrics.as_ref().map(|rm| rm.portfolio_var_95);
        let mut expected_shortfall_95 = portfolio_result.risk_metrics.as_ref().map(|rm| rm.portfolio_es_95);
        
        // Calculate drawdown metrics from equity curve
        let (max_drawdown_duration_days, current_drawdown, avg_drawdown) = {
            let equity = &portfolio_result.equity_curve;
            if equity.len() >= 2 {
                let mut peak = equity[0];
                let mut drawdowns: Vec<f64> = Vec::new();
                let mut current_dd_start: Option<usize> = None;
                let mut max_dd_duration: usize = 0;
                let mut current_dd: f64 = 0.0;
                
                for (i, &val) in equity.iter().enumerate() {
                    if val > peak {
                        // New peak, end any drawdown period
                        if let Some(start) = current_dd_start {
                            max_dd_duration = max_dd_duration.max(i - start);
                        }
                        peak = val;
                        current_dd_start = None;
                    } else if peak > 0.0 {
                        let dd = (peak - val) / peak;
                        drawdowns.push(dd);
                        if current_dd_start.is_none() && dd > 0.001 {
                            current_dd_start = Some(i);
                        }
                        if i == equity.len() - 1 {
                            current_dd = dd;
                        }
                    }
                }
                
                // Convert duration from data points to days (assuming ~1 point per minute)
                let minutes_per_day = 6.5 * 60.0; // Trading hours
                let duration_days = (max_dd_duration as f64 / minutes_per_day).ceil() as i32;
                
                let avg_dd = if !drawdowns.is_empty() {
                    drawdowns.iter().sum::<f64>() / drawdowns.len() as f64
                } else {
                    0.0
                };
                
                (Some(duration_days), Some(current_dd), Some(avg_dd))
            } else {
                (None, None, None)
            }
        };
        
        // ========== MONTE CARLO SIMULATION FOR PORTFOLIO ==========
        let monte_carlo_enabled = params.monte_carlo_enabled;
        let monte_carlo_runs = params.monte_carlo_runs;
        
        let monte_carlo_summary = if monte_carlo_enabled && portfolio_result.total_trades >= 30 {
            log_info!(MAIN_LOGGER, "🎲 [{}] Running Monte Carlo simulation with {} trades", job.id, portfolio_result.total_trades);
            
            // Build a BacktestResult compatible with monte_carlo module
            use bigdecimal::FromPrimitive;
            let core_result = backtest::types::BacktestResult {
                total_pnl: bigdecimal::BigDecimal::from_f64(portfolio_result.total_pnl).unwrap_or_default(),
                num_trades: portfolio_result.total_trades,
                closed_trades: portfolio_result.total_trades,
                open_positions: 0,
                first_trade_timestamp: None,
                last_trade_timestamp: None,
                first_trade_price: None,
                last_trade_price: None,
                realized_pnl: Some(portfolio_result.total_pnl),
                unrealized_pnl: Some(0.0),
                max_drawdown: bigdecimal::BigDecimal::from_f64(portfolio_result.portfolio_max_drawdown).unwrap_or_default(),
                sharpe_ratio: Some(portfolio_result.portfolio_sharpe),
                sortino_ratio,
                calmar_ratio,
                profit_factor: Some(portfolio_result.portfolio_profit_factor),
                win_rate: Some(portfolio_result.portfolio_win_rate),
                avg_trade_return,
                median_trade_return: None,
                avg_trade_duration: Some(portfolio_result.avg_time_in_trade_ms / 1000.0 / 60.0), // Convert to minutes
                volatility,
                gross_profit: None,
                gross_loss: None,
                net_profit: Some(portfolio_result.total_pnl),
                equity_curve: portfolio_result.equity_curve.clone(),
                trade_returns: portfolio_result.trade_returns.clone(),
                initial_capital,
                data_quality: None,
                transaction_costs: None,
                market_impact: None,
                inventory_metrics: None,
                execution_metrics: None,
                risk_halt_reason: None,
            };
            
            let mc_start = std::time::Instant::now();
            match backtest::monte_carlo::run_monte_carlo_simulation(&core_result, monte_carlo_runs) {
                Ok(mc) => {
                    let mc_elapsed = mc_start.elapsed();
                    log_info!(MAIN_LOGGER, "✅ [{}] Monte Carlo complete in {:.2}s | mean_sharpe={:.3} VaR95=${:.2}",
                        job.id, mc_elapsed.as_secs_f64(), mc.mean_metrics.sharpe_ratio, mc.var_95);
                    
                    Some(MonteCarloSummary {
                        num_runs: mc.num_runs,
                        mean_sharpe: mc.mean_metrics.sharpe_ratio,
                        stddev_sharpe: mc.stddev_metrics.sharpe_ratio,
                        var_95: mc.var_95,
                        cvar_95: mc.cvar_95,
                        confidence_lower: mc.percentile_metrics.get(&5).map(|m| m.sharpe_ratio).unwrap_or(0.0),
                        confidence_upper: mc.percentile_metrics.get(&95).map(|m| m.sharpe_ratio).unwrap_or(0.0),
                        data_points: mc.data_points,
                        var_unreliable: mc.var_unreliable,
                        p5_total_pnl: mc.percentile_metrics.get(&5).map(|m| m.net_profit),
                        p50_total_pnl: mc.percentile_metrics.get(&50).map(|m| m.net_profit),
                        p95_total_pnl: mc.percentile_metrics.get(&95).map(|m| m.net_profit),
                        p5_max_drawdown: mc.percentile_metrics.get(&5).map(|m| m.max_drawdown),
                        p50_max_drawdown: mc.percentile_metrics.get(&50).map(|m| m.max_drawdown),
                        p95_max_drawdown: mc.percentile_metrics.get(&95).map(|m| m.max_drawdown),
                        p50_sharpe: mc.percentile_metrics.get(&50).map(|m| m.sharpe_ratio),
                        p5_win_rate: mc.percentile_metrics.get(&5).map(|m| m.win_rate),
                        p50_win_rate: mc.percentile_metrics.get(&50).map(|m| m.win_rate),
                        p95_win_rate: mc.percentile_metrics.get(&95).map(|m| m.win_rate),
                    })
                }
                Err(e) => {
                    log_warn!(MAIN_LOGGER, "[{}] Monte Carlo failed: {} - continuing without MC", job.id, e);
                    None
                }
            }
        } else {
            if monte_carlo_enabled {
                log_info!(MAIN_LOGGER, "[{}] Skipping Monte Carlo: only {} trades (min 30 required)", 
                    job.id, portfolio_result.total_trades);
            }
            None
        };
        
        // Update VaR/ES from Monte Carlo results if available (takes precedence over risk_metrics)
        if let Some(ref mc) = monte_carlo_summary {
            value_at_risk_95 = Some(mc.var_95);
            expected_shortfall_95 = Some(mc.cvar_95);
        }
        
        // Build aggregated inventory metrics for the portfolio from symbol results
        let total_starting_inv_value: f64 = portfolio_result.symbol_results.iter()
            .map(|r| r.starting_inventory_value)
            .sum();
        let total_ending_inv_value: f64 = portfolio_result.symbol_results.iter()
            .map(|r| r.ending_inventory_value)
            .sum();
        let avg_inventory_value = (total_starting_inv_value + total_ending_inv_value) / 2.0;
        
        // Calculate turnover: total volume / average inventory value
        let turnover_per_day = if avg_inventory_value.abs() > 0.001 && portfolio_result.total_volume > 0.0 {
            // Estimate based on simulation duration
            // For now, use a simple ratio (could be improved with actual duration tracking)
            portfolio_result.total_volume / avg_inventory_value.abs()
        } else {
            0.0
        };
        
        // Estimate buy/sell fills from ending inventory changes
        // If ending_inventory > starting_inventory, we're net long (more buy fills)
        // This is an approximation since we don't track individual fill directions
        let total_fills = portfolio_result.total_orders_filled;
        let net_inventory_change: f64 = portfolio_result.symbol_results.iter()
            .map(|r| r.ending_inventory - r.starting_inventory)
            .sum();
        
        // For market making, fills should be roughly balanced
        // Positive net_inventory means more buys than sells
        let (buy_fills, sell_fills) = if total_fills > 0 {
            // Estimate based on net inventory direction
            // If perfectly balanced, buy_fills = sell_fills = total_fills / 2
            let base = total_fills / 2;
            if net_inventory_change > 0.0 {
                // More buys - assign extra to buy side
                (total_fills - base, base)
            } else if net_inventory_change < 0.0 {
                // More sells
                (base, total_fills - base)
            } else {
                // Perfectly balanced
                (base, base)
            }
        } else {
            (0, 0)
        };
        
        let imbalance_ratio = if portfolio_result.total_volume > 0.0 && avg_inventory_value.abs() > 0.001 {
            net_inventory_change.abs() / (portfolio_result.total_volume / 2.0).max(0.001)
        } else {
            0.0
        };
        
        let inventory_metrics = Some(InventorySummary {
            total_volume: portfolio_result.total_volume,
            avg_inventory_value,
            turnover_per_day,
            buy_fills,
            sell_fills,
            buy_volume: portfolio_result.total_volume / 2.0, // Estimated
            sell_volume: portfolio_result.total_volume / 2.0, // Estimated
            imbalance_ratio,
            // Aggregate starting/ending values from symbols
            starting_inventory: Some(portfolio_result.symbol_results.iter().map(|r| r.starting_inventory).sum()),
            starting_inventory_value: Some(total_starting_inv_value),
            starting_mark_price: portfolio_result.symbol_results.first().map(|r| r.starting_mark_price),
            ending_inventory: Some(portfolio_result.symbol_results.iter().map(|r| r.ending_inventory).sum()),
            ending_inventory_value: Some(total_ending_inv_value),
            ending_mark_price: portfolio_result.symbol_results.first().map(|r| r.ending_mark_price),
            starting_capital: Some(portfolio_result.initial_capital),
            ending_capital: Some(portfolio_result.symbol_results.iter().map(|r| r.ending_capital).sum()),
            final_equity: Some(portfolio_result.final_equity),
        });
        
        // Build execution metrics summary for the portfolio
        let fill_rate = if portfolio_result.total_orders_placed > 0 {
            portfolio_result.total_orders_filled as f64 / portfolio_result.total_orders_placed as f64
        } else {
            0.0
        };
        let cancellation_rate = if portfolio_result.total_orders_placed > 0 {
            (portfolio_result.total_orders_placed - portfolio_result.total_orders_filled) as f64 / portfolio_result.total_orders_placed as f64
        } else {
            0.0
        };
        
        let execution_metrics = Some(ExecutionSummary {
            orders_placed: portfolio_result.total_orders_placed,
            orders_filled: portfolio_result.total_orders_filled,
            orders_cancelled: portfolio_result.total_orders_placed.saturating_sub(portfolio_result.total_orders_filled),
            total_fills: portfolio_result.total_orders_filled,
            fill_rate,
            cancellation_rate,
            avg_fills_per_order: if portfolio_result.total_orders_filled > 0 { 1.0 } else { 0.0 },
            avg_time_to_fill_seconds: portfolio_result.avg_time_in_trade_ms / 1000.0,
            median_time_to_fill_seconds: portfolio_result.avg_time_in_trade_ms / 1000.0, // Approximate
            bid_orders: portfolio_result.total_orders_placed / 2, // Approximate 50/50 split
            ask_orders: portfolio_result.total_orders_placed / 2,
            bid_fills: portfolio_result.total_orders_filled / 2,
            ask_fills: portfolio_result.total_orders_filled / 2,
            bid_fill_rate: fill_rate,
            ask_fill_rate: fill_rate,
            avg_quote_spread_bps: 0.0, // Not tracked at portfolio level
            round_trip_win_rate: Some(portfolio_result.portfolio_win_rate),
            total_round_trips: portfolio_result.total_trades,
            profitable_round_trips: (portfolio_result.total_trades as f64 * portfolio_result.portfolio_win_rate) as usize,
            spread_capture_rate: None,
            adverse_selection_rate: None,
            avg_pnl_per_round_trip: if portfolio_result.total_trades > 0 {
                Some(portfolio_result.total_pnl / portfolio_result.total_trades as f64)
            } else {
                None
            },
        });
        
        // Build result
        let backtest_result = BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some("portfolio_ga".to_string()),
            total_pnl: portfolio_result.total_pnl,
            sharpe_ratio: portfolio_result.portfolio_sharpe,
            max_drawdown: portfolio_result.portfolio_max_drawdown,
            win_rate: portfolio_result.portfolio_win_rate,
            total_trades: portfolio_result.total_trades,
            profit_factor: portfolio_result.portfolio_profit_factor,
            equity_curve: portfolio_result.equity_curve,
            completed_at: Utc::now(),
            closed_trades: Some(portfolio_result.total_trades),
            open_positions: Some(0),
            realized_pnl: Some(portfolio_result.total_pnl),
            unrealized_pnl: Some(0.0),
            // Additional calculated metrics
            annualized_return,
            volatility,
            sortino_ratio,
            calmar_ratio,
            avg_trade_return,
            best_trade,
            worst_trade,
            avg_time_in_trade: Some(portfolio_result.avg_time_in_trade_ms / 1000.0), // Convert ms to seconds
            // Risk metrics
            value_at_risk_95,
            expected_shortfall_95,
            max_drawdown_duration_days,
            current_drawdown,
            avg_drawdown,
            // Execution metrics - individual fields
            total_orders: Some(portfolio_result.total_orders_placed as i32),
            filled_orders: Some(portfolio_result.total_orders_filled as i32),
            cancelled_orders: Some((portfolio_result.total_orders_placed - portfolio_result.total_orders_filled) as i32),
            avg_slippage: None,
            total_commission_paid: Some(portfolio_result.total_fees),
            // Full execution metrics struct
            execution_metrics,
            // Monte Carlo results
            monte_carlo: monte_carlo_summary,
            // Walk-forward not yet implemented for portfolio
            walk_forward: None,
            // Portfolio specific
            is_portfolio: true,
            portfolio_metrics: Some(portfolio_metrics),
            inventory_metrics,
            // No routing metrics for portfolio GA
            routing_metrics: None,
            // Metrics profile for UI display
            metrics_profile: Some(Self::build_metrics_profile("portfolio_ga")),
            // Strategy params with full optimization details
            strategy_params: Some(serde_json::json!({
                "portfolio_mode": true,
                "ga_optimized": true,
                "symbols": symbols_ordered,
                "optimized_weights": optimized_weights,
                "symbol_optimization": symbol_optimization_details,
                "initial_capital": params.initial_capital,
                "population_size": params.population_size,
                "generations": params.generations,
                "correlation_matrix": portfolio_result.correlation_matrix,
            })),
            // Date range - use actual data timestamps with fallback to job params
            start_date: portfolio_result.first_event_timestamp_ms
                .map(|ms| chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default())
                .filter(|s| !s.is_empty())
                .or_else(|| job.params.start_date.clone()),
            end_date: portfolio_result.last_event_timestamp_ms
                .map(|ms| chrono::DateTime::from_timestamp_millis(ms)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default())
                .filter(|s| !s.is_empty())
                .or_else(|| job.params.end_date.clone()),
            risk_halt_reason: None,
        };
        
        let _ = self.job_queue.update_progress(&job.id, 100.0).await;
        
        Ok(backtest_result)
    }
    
    /// Execute genetic optimization to find best strategy parameters
    async fn execute_genetic_optimization(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        // Dispatch to strategy-specific optimization
        let strategy_type = job.params.strategy_type.to_lowercase();
        let is_momentum = strategy_type.contains("momentum") || strategy_type.contains("trend");
        
        if is_momentum {
            // Check if portfolio mode (multiple assets)
            if job.params.is_portfolio() {
                eprintln!("🚀 [MOMENTUM_PORTFOLIO_GA] Dispatching to portfolio momentum optimization");
                return self.execute_momentum_portfolio_ga_optimization(job).await;
            } else {
                return self.execute_momentum_ga_optimization(job).await;
            }
        }
        
        // Default: Avellaneda-Stoikov market making optimization
        use genetic::GeneticOptimizer;
        use genetic::chromosome::AvellanedaStoikovChromosome;
        use config::GeneticConfig;
        
        let params = &job.params;
        
        // === SINGLE-SYMBOL GA EXECUTION LOGGING ===
        eprintln!("\n============================================================");
        eprintln!("🧬 [SINGLE_GA] STARTING job_id={}", job.id);
        eprintln!("   symbol: {}", params.symbol);
        eprintln!("   exchange: {}", params.exchange);
        eprintln!("   strategy_type: {}", params.strategy_type);
        eprintln!("   population_size: {}", params.population_size);
        eprintln!("   generations: {}", params.generations);
        eprintln!("   portfolio_assets: {:?}", params.portfolio_assets.as_ref().map(|a| a.len()));
        eprintln!("   ⚠️ NOTE: This is SINGLE-SYMBOL mode. Portfolio assets will be IGNORED!");
        eprintln!("============================================================\n");
        
        // ============ TIMING: Total Job ============
        let timing_total_start = std::time::Instant::now();
        
        // Send structured timing event to Kibana
        log_info_structured!(MAIN_LOGGER, "TIMING_JOB_START",
            "event" => "TIMING_JOB_START",
            "job_id" => &job.id,
            "symbol" => &params.symbol,
            "exchange" => &params.exchange,
            "population_size" => params.population_size,
            "generations" => params.generations
        );
        
        log_info_structured!(GENETIC_LOGGER, "GA_OPTIMIZATION_STARTING",
            "event" => "GA_OPTIMIZATION_STARTING",
            "job_id" => &job.id,
            "population_size" => params.population_size,
            "generations" => params.generations,
            "symbol" => &params.symbol,
            "strategy_type" => &params.strategy_type
        );
        
        // Create efficient progress tracker (broadcasts via channel, no DB in hot path)
        let progress_tracker = Arc::new(ProgressTracker::new(job.id.clone(), Arc::clone(&self.job_queue)));
        
        // Persist phase change to DB (lifecycle event) + broadcast with granular details
        progress_tracker.broadcast_phase("data_loading", serde_json::json!({
            "status": "starting",
            "symbol": params.symbol,
            "exchange": params.exchange,
        }));
        let _ = progress_tracker.persist_phase_change("data_loading", serde_json::json!({
            "status": "starting",
            "population_size": params.population_size,
            "generations": params.generations,
        })).await;
        let _ = progress_tracker.persist_progress(2.0).await; // 2% - starting data load
        
        // ============ TIMING: Data Loading ============
        let timing_data_load_start = std::time::Instant::now();
        
        // Load market data
        log_info_structured!(GENETIC_LOGGER, "GA_LOADING_DATA",
            "event" => "GA_LOADING_DATA",
            "job_id" => &job.id,
            "symbol" => &params.symbol,
            "exchange" => &params.exchange
        );
        
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
        if data.is_empty() {
            log_error!(GENETIC_LOGGER, "Job {} no market data available", job.id);
            return Err("No market data available".into());
        }
        
        let timing_data_load_elapsed = timing_data_load_start.elapsed();
        let data_load_rate = data.len() as f64 / timing_data_load_elapsed.as_secs_f64();
        
        // Send timing to Kibana with structured metadata
        log_info_structured!(MAIN_LOGGER, "TIMING_DATA_LOAD",
            "event" => "TIMING_DATA_LOAD",
            "job_id" => &job.id,
            "data_points" => data.len(),
            "elapsed_sec" => format!("{:.2}", timing_data_load_elapsed.as_secs_f64()),
            "rate_per_sec" => format!("{:.0}", data_load_rate)
        );
        
        // Update UI with data load complete
        progress_tracker.broadcast_phase("data_loading", serde_json::json!({
            "status": "complete",
            "data_points": data.len(),
            "elapsed_sec": timing_data_load_elapsed.as_secs_f64(),
        }));
        let _ = progress_tracker.persist_progress(5.0).await; // 5% - data loaded
        eprintln!("⏱️ TIMING | DATA_LOAD | {} points in {:.2}s", data.len(), timing_data_load_elapsed.as_secs_f64());
        
        // ============================================================================
        // TRAIN/TEST/VALIDATE SPLIT for Walk-Forward Validation
        // ============================================================================
        // train_ratio: GA optimization + backtest (default 60%)
        // test_ratio: Walk-forward out-of-sample validation (default 20%)
        // validate_ratio: Final held-out validation (default 20%, reserved for future)
        //
        // This prevents overfitting by testing the strategy on unseen data
        let total_len = data.len();
        let train_end = (total_len as f64 * params.train_ratio) as usize;
        let test_end = train_end + (total_len as f64 * params.test_ratio) as usize;
        
        // Split data chronologically (time-series data must preserve order)
        let train_data: Vec<MarketData> = data[..train_end].to_vec();
        let test_data: Vec<MarketData> = data[train_end..test_end].to_vec();
        // validate_data would be data[test_end..] - reserved for future use
        
        // Wrap splits in Arc for efficient sharing
        let shared_train_data = Arc::new(train_data);
        let shared_test_data = Arc::new(test_data);
        
        log_info_structured!(GENETIC_LOGGER, "GA_DATA_SPLIT",
            "event" => "GA_DATA_SPLIT",
            "job_id" => &job.id,
            "total_data_points" => total_len,
            "train_data_points" => shared_train_data.len(),
            "test_data_points" => shared_test_data.len(),
            "train_ratio" => format!("{:.0}%", params.train_ratio * 100.0),
            "test_ratio" => format!("{:.0}%", params.test_ratio * 100.0)
        );
        eprintln!("📊 Data split: train={} ({:.0}%) | test/walkforward={} ({:.0}%) | total={}", 
            shared_train_data.len(), params.train_ratio * 100.0,
            shared_test_data.len(), params.test_ratio * 100.0,
            total_len);
        
        // GA optimization and backtest use training data only
        let data_for_backtest = Arc::clone(&shared_train_data);
        // Walk-forward uses test data (unseen during optimization)
        let data_for_walkforward = Arc::clone(&shared_test_data);
        
        // Convert training data to events for GA fitness evaluation
        let events_start = std::time::Instant::now();
        eprintln!("📊 Converting training data to trade events for RealisticSimulator...");
        let events = backtest::ticks_to_trade_events(&shared_train_data);
        let events_arc = std::sync::Arc::new(events);
        eprintln!("📊 Events converted in {:.2}s ({} events from {} train points)", 
            events_start.elapsed().as_secs_f64(), events_arc.len(), shared_train_data.len());
        log_info!(GENETIC_LOGGER, "Converted training data to trade events in {:.2}ms | {} events",
            events_start.elapsed().as_millis(),
            events_arc.len());
        
        // Get exchange fee config
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&params.exchange);

        eprintln!("📊 About to create GeneticConfig...");
        
        // Create genetic config
        let config = GeneticConfig {
            population_size: params.population_size,
            generations: params.generations,
            mutation_rate: 0.1,
            crossover_rate: 0.7,
            ..Default::default() // Use defaults for MC lazy eval (disabled)
        };
        
        // Use efficient progress callback that batches DB writes
        let generation_callback = progress_tracker.generation_callback();
        
        // NOTE: data_for_backtest and data_for_walkforward are already defined above
        // from the train/test split. Here we just set up the remaining references.
        let events_for_fitness = std::sync::Arc::clone(&events_arc);
        let fee_config_for_fitness = exchange_fee_config.clone();
        // Create separate clones for distributed mode fallback (local GA uses these too)
        let data_for_distributed = Arc::clone(&shared_train_data);  // Use training data for distributed GA
        let _symbol = params.symbol.clone();
        let exchange = params.exchange.clone();
        let symbol_for_distributed = params.symbol.clone();
        let exchange_for_distributed = params.exchange.clone();
        let exchange_for_logging = exchange.clone(); // Clone for use after the closure
        let initial_capital = params.initial_capital;
        let initial_capital_for_distributed = params.initial_capital;
        let _initial_inventory_pct = params.initial_inventory_pct;
        let initial_inventory_pct_for_distributed = params.initial_inventory_pct;
        let _commission_rate = params.commission_rate;
        let commission_rate_for_distributed = params.commission_rate;
        
        // Create fitness function that evaluates chromosomes and returns equity curve for robustness analysis
        // THREAD SAFETY: We clone Arc references and create fresh state inside each async block
        // Use static counter to track fitness evaluation progress
        static FITNESS_EVAL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        
        // === FITNESS FUNCTION ===
        // Uses RealisticSimulator for accurate GA optimization matching final evaluation
        
        let fitness_fn: genetic::AsyncFitnessFn<AvellanedaStoikovChromosome> = Arc::new(move |chromosome: &AvellanedaStoikovChromosome| {
            let events = std::sync::Arc::clone(&events_for_fitness);
            let fee_config = fee_config_for_fitness.clone();
            let chr = chromosome.clone();
            
            Box::pin(async move {
                let eval_id = FITNESS_EVAL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let eval_start = std::time::Instant::now();
                
                // Use RealisticSimulator for GA fitness (same as final evaluation)
                let sim_params = backtest::RealisticASParams {
                    risk_aversion: chr.risk_aversion,
                    inventory_target: chr.inventory_target,
                    order_size_pct: chr.order_size_pct,
                    initial_inventory_pct: 0.0,
                    volatility_cap: chr.volatility_cap,
                    time_horizon_minutes: chr.time_horizon_minutes,
                    min_spread_bps: chr.min_spread_bps,
                    max_inventory_multiple: 1.0,
                    maker_fee_rate: fee_config.maker_fee,
                    taker_fee_rate: fee_config.taker_fee,
                    fee_spread_multiplier: chr.fee_spread_multiplier,
                    adverse_selection_sensitivity: chr.adverse_selection_sensitivity,
                    take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
                    stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
                    max_trades_per_minute: chr.max_trades_per_minute,
                    max_orders_per_second: 10.0,
                    rate_limit_cooldown_ms: 1000,
                    window_ms: chr.window_ms as i64,
                    min_quote_lifetime_ms: chr.min_quote_lifetime_ms as i64,
                    ..Default::default()
                };
                
                let simulator = backtest::RealisticSimulator::with_exchange_config(
                    initial_capital,
                    fee_config,
                );
                let sim_result = simulator.run(&events, &sim_params);
                
                // ==============================================
                // MARKET MAKING FITNESS FUNCTION
                // ==============================================
                // AGGRESSIVE HARD REJECT - MARKET MAKING VIABILITY
                // ==============================================
                // Only allow chromosomes that meet ALL viable MM criteria.
                // Fail fast = faster evolution toward profitable strategies.
                
                let gross_profit = sim_result.trade_returns.iter()
                    .filter(|&&r| r > 0.0).sum::<f64>();
                let commission_ratio = if gross_profit > 0.0 {
                    sim_result.total_fees / gross_profit
                } else {
                    1.0
                };
                let ending_inv_exposure = sim_result.inventory_metrics.ending_inventory_value.abs() / initial_capital;
                
                // 1. Minimum trades for statistical significance (MM should trade frequently)
                if sim_result.num_trades < 1000 {
                    return genetic::FitnessResult::new(-10.0, sim_result.equity_curve);
                }
                
                // 2. Win rate >= 51% (MM MUST have edge, not get picked off)
                if sim_result.win_rate < 0.51 {
                    return genetic::FitnessResult::new(-8.0, sim_result.equity_curve);
                }
                
                // 3. Profit factor >= 1.2 (must have clear edge after costs)
                if sim_result.profit_factor < 1.2 {
                    return genetic::FitnessResult::new(-7.0, sim_result.equity_curve);
                }
                
                // 4. Sharpe ratio >= 1.0 (MM should have excellent risk-adjusted returns)
                if sim_result.sharpe_ratio < 1.0 {
                    return genetic::FitnessResult::new(-6.5, sim_result.equity_curve);
                }
                
                // 5. Max drawdown <= 20% (risk control is non-negotiable)
                if sim_result.max_drawdown > 0.20 {
                    return genetic::FitnessResult::new(-6.0, sim_result.equity_curve);
                }
                
                // 6. Commission ratio <= 60% (fees can't eat most of profit)
                if commission_ratio > 0.60 {
                    return genetic::FitnessResult::new(-5.0, sim_result.equity_curve);
                }
                
                // 7. Inventory exposure <= 10% (MM must stay flat, not take directional bets)
                if ending_inv_exposure > 0.10 {
                    return genetic::FitnessResult::new(-4.0, sim_result.equity_curve);
                }
                
                // 8. Must be profitable (this is the whole point)
                if sim_result.total_pnl <= 0.0 {
                    return genetic::FitnessResult::new(-3.0, sim_result.equity_curve);
                }
                
                // 9. Avg profit per trade >= 2× avg commission (edge must be robust)
                let avg_profit_per_trade = sim_result.total_pnl / sim_result.num_trades as f64;
                let avg_commission_per_trade = sim_result.total_fees / sim_result.num_trades as f64;
                if avg_profit_per_trade < 2.0 * avg_commission_per_trade {
                    return genetic::FitnessResult::new(-2.5, sim_result.equity_curve);
                }
                
                // ==============================================
                // PASSED ALL GATES - Now rank by quality
                // ==============================================
                let num_trades = sim_result.num_trades as f64;
                let net_profit_pct = sim_result.total_pnl / initial_capital;
                
                // Profit score (primary driver)
                let profit_score = (net_profit_pct * 10.0).tanh();
                
                // Sharpe bonus
                let sharpe_score = (sim_result.sharpe_ratio.max(0.0) / 3.0).min(1.0);
                
                // Win rate bonus (baseline at 51%)
                let win_rate_bonus = (sim_result.win_rate - 0.51) * 2.0;
                
                // Profit factor bonus (baseline at 1.2)
                let pf_bonus = ((sim_result.profit_factor - 1.2) / 1.8).min(1.0);
                
                // Trade count bonus (log scale, baseline at 1000)
                let trade_bonus = (num_trades.ln() - 6.9) / 3.0;  // 1000 trades = 0, 5000 = 0.54
                
                // Low drawdown bonus
                let dd_bonus = (0.20 - sim_result.max_drawdown) * 2.0;
                
                // Commission efficiency bonus
                let comm_bonus = (0.60 - commission_ratio) / 0.60;
                
                // Inventory management bonus (tighter = better)
                let inv_bonus = (0.10 - ending_inv_exposure) / 0.10;
                
                let fitness = 
                    profit_score * 0.40 +
                    sharpe_score * 0.15 +
                    pf_bonus * 0.15 +
                    win_rate_bonus * 0.10 +
                    trade_bonus * 0.05 +
                    dd_bonus * 0.05 +
                    comm_bonus * 0.05 +
                    inv_bonus * 0.05;
                
                log_info_structured!(GENETIC_LOGGER, "REALISTIC_FITNESS_EVAL",
                    "event" => "REALISTIC_FITNESS_EVAL",
                    "eval_id" => eval_id,
                    "fitness" => format!("{:.6}", fitness),
                    "pnl" => format!("{:.2}", sim_result.total_pnl),
                    "sharpe" => format!("{:.3}", sim_result.sharpe_ratio),
                    "trades" => sim_result.num_trades,
                    "elapsed_ms" => eval_start.elapsed().as_millis()
                );
                
                genetic::FitnessResult::new(fitness, sim_result.equity_curve)
            })
        });
        
        // Run genetic optimization with robustness-weighted fitness
        log_info!(GENETIC_LOGGER, "Job {} creating GeneticOptimizer", job.id);
        
        
        // Clone config before creating optimizer since it will be moved
        let config_for_distributed = config.clone();
        
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: Some(generation_callback),
            job_id: Some(job.id.clone()),
        };
        
        // Update UI for GA start
        progress_tracker.broadcast_phase("genetic_optimization", serde_json::json!({
            "status": "running",
            "population_size": params.population_size,
            "generations": params.generations,
            "current_generation": 0,
        }));
        let _ = progress_tracker.persist_phase_change("genetic_optimization", serde_json::json!({
            "status": "running",
            "population_size": params.population_size,
            "generations": params.generations,
        })).await;
        
        // ============ TIMING: GA Optimization ============
        let timing_ga_start = std::time::Instant::now();
        
        // Check if distributed mode is enabled via environment variable
        eprintln!("🧬 Checking distributed mode configuration...");
        let distributed_mode = DistributedMode::from_env();
        eprintln!("🧬 Distributed mode: enabled={}, min_workers={}, broker_url={:?}", 
            distributed_mode.enabled, distributed_mode.min_workers, distributed_mode.broker_url);
        
        let (best_chromosome, best_fitness) = if distributed_mode.enabled {
            eprintln!("🧬 Taking DISTRIBUTED GA path...");
            log_info!(GENETIC_LOGGER, "Job {} attempting DISTRIBUTED genetic optimization", job.id);
            
            // Create distributed GA config with simplified TRAIN/VALIDATE split
            let distributed_config = DistributedGAConfig {
                genetic_config: config_for_distributed.clone(),
                symbol: params.symbol.clone(),
                exchange: params.exchange.clone(),
                initial_capital,
                initial_30d_volume: params.initial_30d_volume,
                initial_inventory_pct: params.initial_inventory_pct,
                cache_ttl_seconds: 3600, // 1 hour cache TTL
                start_time: params.start_date.clone().unwrap_or_else(|| "2024-01-01T00:00:00Z".to_string()),
                end_time: params.end_date.clone().unwrap_or_else(|| "2024-12-31T23:59:59Z".to_string()),
                train_ratio: params.train_ratio,
                validation_ratio: params.test_ratio + params.validate_ratio, // Combined for walk-forward
                fitness_weights: params.fitness_weights.clone(),
                monte_carlo_runs: params.monte_carlo_runs,
            };
            eprintln!("🧬 Created DistributedGAConfig with start_time={}, end_time={}, fitness_weights={:?}", 
                distributed_config.start_time, distributed_config.end_time, distributed_config.fitness_weights);
            
            // Clone data for the closure
            let data_for_fallback = Arc::clone(&data_for_distributed);
            let symbol_for_fallback = symbol_for_distributed.clone();
            let exchange_for_fallback = exchange_for_distributed.clone();
            let initial_capital_fallback = initial_capital_for_distributed;
            let initial_inventory_pct_fallback = initial_inventory_pct_for_distributed;
            let commission_rate_fallback = commission_rate_for_distributed;
            
            // Create progress callback for distributed GA
            // This parses generation info from the detail string and updates the database
            let job_id_for_distributed = job.id.clone();
            let job_queue_for_distributed = Arc::clone(&self.job_queue);
            let distributed_progress_callback = move |progress: f64, detail: String| {
                let job_id = job_id_for_distributed.clone();
                let job_queue = Arc::clone(&job_queue_for_distributed);
                
                // Parse generation info from detail string (e.g., "Generation 5/10: evaluating chromosomes")
                if detail.starts_with("Generation ") {
                    let gen_str = detail.trim_start_matches("Generation ");
                    let gen_part = gen_str.split(|c| c == ':' || c == ' ').next().unwrap_or("");
                    if let Some((current, total)) = gen_part.split_once('/') {
                        if let (Ok(cur), Ok(tot)) = (current.trim().parse::<usize>(), total.trim().parse::<usize>()) {
                            // Broadcast and persist progress
                            get_progress_broadcaster().publish_generation(&job_id, cur, tot);
                            
                            // Spawn async task for database updates
                            tokio::spawn(async move {
                                if let Err(e) = job_queue.update_progress(&job_id, progress).await {
                                    log_warn!(MAIN_LOGGER, "Job {} failed to persist progress: {}", job_id, e);
                                }
                                if let Err(e) = job_queue.update_generation(&job_id, cur as i32, tot as i32).await {
                                    log_warn!(MAIN_LOGGER, "Job {} failed to persist generation: {}", job_id, e);
                                }
                            });
                        }
                    }
                }
            };
            
            // Run with automatic fallback to local if distributed fails
            DistributedGARunner::run_with_fallback(
                &distributed_mode,
                &distributed_config,
                &shared_train_data,  // Pass training data slice reference
                move |chromo: &AvellanedaStoikovChromosome| {
                    // This fitness function is used for LOCAL fallback only
                    // When distributed mode works, workers evaluate chromosomes
                    let data = Arc::clone(&data_for_fallback);
                    let symbol = symbol_for_fallback.clone();
                    let exchange = exchange_for_fallback.clone();
                    let chr = chromo.clone();
                    let initial_capital = initial_capital_fallback;
                    let initial_inventory_pct = initial_inventory_pct_fallback;
                    let commission_rate = commission_rate_fallback;
                    
                    Box::pin(async move {
                        // Same fitness evaluation as the main local path
                        let mut backtest_config = BacktestConfig::default();
                        backtest_config.trading.initial_capital = initial_capital;
                        backtest_config.trading.commission_rate = commission_rate;
                        backtest_config.analysis.skip_genetic_optimization = true;
                        backtest_config.analysis.auto_monte_carlo = false;
                        backtest_config.analysis.auto_walk_forward = false;
                        backtest_config.data.gap_detection.enabled = false;
                        
                        let engine = BacktestEngine::new(backtest_config);
                        let asset_price = Self::get_reference_price(&data);
                        
                        let inventory_pct = initial_inventory_pct;
                        let quote_balance = initial_capital * (1.0 - inventory_pct);
                        let base_value = initial_capital * inventory_pct;
                        let base_quantity = if asset_price > 0.0 { base_value / asset_price } else { 0.0 };
                        let first_timestamp = Self::get_first_timestamp(&data);
                        
                        let mut portfolio = PortfolioState::with_inventory(
                            symbol.clone(),
                            BigDecimal::from_f64(quote_balance).unwrap_or_default(),
                            base_quantity,
                            BigDecimal::from_f64(asset_price).unwrap_or_default(),
                            first_timestamp,
                        );
                        
                        let orderbook_config = OrderBookConfig::default();
                        let mut orderbook = OrderBook::new(symbol.clone(), exchange.clone(), orderbook_config);
                        let mut risk_manager = RiskManager::new();
                        
                        let order_size_units = (initial_capital * chr.order_size_pct) / asset_price;
                        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&exchange);
                        let maker_fee = exchange_fee_config.maker_fee;
                        
                        let strategy_params = AvellanedaStoikovParams {
                            risk_aversion: chr.risk_aversion,
                            inventory_target: chr.inventory_target,
                            order_size: order_size_units,
                            order_size_pct: chr.order_size_pct,
                            initial_capital,
                            initial_inventory_pct: 0.5,
                            window_ms: chr.window_ms,
                            min_quote_lifetime_ms: chr.min_quote_lifetime_ms,
                            fee_rate: maker_fee,
                            volatility_cap: chr.volatility_cap,
                            time_horizon_minutes: chr.time_horizon_minutes,
                            market_depth_k: chr.market_depth_k,
                            min_spread_bps: chr.min_spread_bps,
                            max_trades_per_minute: chr.max_trades_per_minute,
                            take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
                            stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
                            volatility_adjustment_factor: chr.volatility_adjustment_factor,
                            inventory_penalty_factor: chr.inventory_penalty_factor,
                            trend_sensitivity: chr.trend_sensitivity,
                            flow_sensitivity: chr.flow_sensitivity,
                            ..Default::default()
                        };
                        
                        let mut strategy_params_map = HashMap::new();
                        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                        
                        let strategy_config = StrategyConfig {
                            id: "genetic-eval-fallback".to_string(),
                            name: "Avellaneda-Stoikov".to_string(),
                            strategy_type: "AvellanedaStoikov".to_string(),
                            symbol: symbol.clone(),
                            exchange: exchange.clone(),
                            parameters: strategy_params_map,
                            risk_limits: StrategyRiskLimits::default(),
                            enabled: true,
                        };
                        
                        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
                        let mut strategy_manager = StrategyManager::new();
                        if strategy_manager.register_strategy(strategy_config, strategy).is_err() {
                            return genetic::FitnessResult::failure();
                        }
                        
                        match engine.run_core_simulation_direct(
                            &mut strategy_manager,
                            &mut orderbook,
                            &mut portfolio,
                            &mut risk_manager,
                            &data,
                        ).await {
                            Ok(result) => {
                                let fitness = calculate_comprehensive_fitness(&result, initial_capital);
                                genetic::FitnessResult::new(fitness, result.equity_curve)
                            }
                            Err(_) => genetic::FitnessResult::failure()
                        }
                    })
                },
                Some(distributed_progress_callback),
            ).await
        } else {
            log_info!(GENETIC_LOGGER, "Job {} starting LOCAL genetic algorithm with robustness-weighted fitness", job.id);
            optimizer.run().await
        };
        
        let timing_ga_elapsed = timing_ga_start.elapsed();
        
        // Send timing to Kibana with structured metadata
        log_info_structured!(MAIN_LOGGER, "TIMING_GA_COMPLETE",
            "event" => "TIMING_GA_COMPLETE",
            "job_id" => &job.id,
            "elapsed_sec" => format!("{:.2}", timing_ga_elapsed.as_secs_f64()),
            "elapsed_min" => format!("{:.1}", timing_ga_elapsed.as_secs_f64() / 60.0),
            "best_fitness" => format!("{:.6}", best_fitness),
            "population_size" => params.population_size,
            "generations" => params.generations
        );
        
        // Update UI with GA complete
        progress_tracker.broadcast_phase("genetic_optimization", serde_json::json!({
            "status": "complete",
            "best_fitness": best_fitness,
            "elapsed_sec": timing_ga_elapsed.as_secs_f64(),
        }));
        let _ = progress_tracker.persist_progress(85.0).await; // 85% - GA complete
        
        log_info_structured!(GENETIC_LOGGER, "GA_OPTIMIZER_RUN_COMPLETE",
            "event" => "GA_OPTIMIZER_RUN_COMPLETE",
            "job_id" => &job.id,
            "best_fitness" => format!("{:.6}", best_fitness)
        );
        
        // Track if strategy is below viability threshold (but continue to show results)
        let strategy_below_threshold = best_fitness < 0.01;
        if strategy_below_threshold {
            log_warn!(GENETIC_LOGGER, "Job {} WARNING: No viable strategy found! best_fitness={:.6} < 0.01 threshold. All strategies had commission ratio >= 80%", 
                job.id, best_fitness);
            log_warn!(GENETIC_LOGGER, "Job {} best params were: risk_aversion={:.4} order_size_pct={:.2}% min_spread_bps={:.1} - continuing to show results anyway", 
                job.id, best_chromosome.risk_aversion, best_chromosome.order_size_pct * 100.0, best_chromosome.min_spread_bps);
        }
        
        // Broadcast + persist phase change after optimization completes
        progress_tracker.broadcast_phase("post_optimization", serde_json::json!({
            "best_fitness": best_fitness,
            "risk_aversion": best_chromosome.risk_aversion,
            "inventory_target": best_chromosome.inventory_target,
        }));
        
        // Log BEFORE db persist to diagnose stalls
        log_info_structured!(GENETIC_LOGGER, "GA_COMPLETE_BEFORE_DB_PERSIST",
            "event" => "GA_COMPLETE_BEFORE_DB_PERSIST",
            "job_id" => &job.id,
            "best_fitness" => format!("{:.4}", best_fitness)
        );
        
        // Persist with timeout to prevent indefinite stalls on db issues
        let persist_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            progress_tracker.persist_phase_change("post_optimization", serde_json::json!({
                "best_fitness": best_fitness,
                "risk_aversion": best_chromosome.risk_aversion,
                "inventory_target": best_chromosome.inventory_target,
            }))
        ).await;
        
        if persist_result.is_err() {
            log_warn!(GENETIC_LOGGER, "Job {} DB_PERSIST_TIMEOUT - continuing without persist", job.id);
        }
        
        log_info_structured!(GENETIC_LOGGER, "GA_COMPLETE_AFTER_DB_PERSIST",
            "event" => "GA_COMPLETE_AFTER_DB_PERSIST",
            "job_id" => &job.id
        );
        
        log_info_structured!(GENETIC_LOGGER, "GA_OPTIMIZATION_COMPLETE",
            "event" => "GA_OPTIMIZATION_COMPLETE",
            "job_id" => &job.id,
            "best_fitness" => format!("{:.4}", best_fitness),
            "risk_aversion" => format!("{:.3}", best_chromosome.risk_aversion),
            "inventory_target" => format!("{:.3}", best_chromosome.inventory_target),
            "order_size_pct" => format!("{:.2}", best_chromosome.order_size_pct * 100.0)
        );
        
        // Log ALL chromosome parameters for debugging unprofitable strategies
        // Note: fee_rate is now from exchange config, not chromosome
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&exchange_for_logging);
        log_info_structured!(MAIN_LOGGER, "STRATEGY_PARAMS",
            "event" => "STRATEGY_PARAMS",
            "job_id" => &job.id,
            "best_fitness" => format!("{:.6}", best_fitness),
            "exchange_maker_fee" => format!("{:.4}", exchange_fee_config.maker_fee * 100.0),
            "risk_aversion" => format!("{:.4}", best_chromosome.risk_aversion),
            "order_size_pct" => format!("{:.4}", best_chromosome.order_size_pct),
            "window_ms" => best_chromosome.window_ms,
            "min_quote_lifetime_ms" => best_chromosome.min_quote_lifetime_ms,
            "inventory_target" => format!("{:.4}", best_chromosome.inventory_target),
            "volatility_cap" => format!("{:.4}", best_chromosome.volatility_cap),
            "time_horizon_min" => format!("{:.2}", best_chromosome.time_horizon_minutes),
            "market_depth_k" => format!("{:.2}", best_chromosome.market_depth_k),
            "min_spread_bps" => format!("{:.1}", best_chromosome.min_spread_bps),
            "max_trades_per_min" => format!("{:.2}", best_chromosome.max_trades_per_minute),
            "vol_adj_factor" => format!("{:.2}", best_chromosome.volatility_adjustment_factor),
            "inv_penalty_factor" => format!("{:.2}", best_chromosome.inventory_penalty_factor),
            "trend_sensitivity" => format!("{:.3}", best_chromosome.trend_sensitivity),
            "flow_sensitivity" => format!("{:.3}", best_chromosome.flow_sensitivity)
        );
        
        // Log expected economics based on parameters
        // At 0.25% maker fee, expected commission per trade = 2 * order_value * 0.0025
        let expected_order_value = params.initial_capital * best_chromosome.order_size_pct;
        let expected_commission_per_trade = 2.0 * expected_order_value * 0.0025; // Both sides, maker fee
        let expected_spread_capture = expected_order_value * best_chromosome.min_spread_bps / 10000.0;
        let expected_net_per_trade = expected_spread_capture - expected_commission_per_trade;
        log_info_structured!(MAIN_LOGGER, "EXPECTED_ECONOMICS",
            "event" => "EXPECTED_ECONOMICS",
            "job_id" => &job.id,
            "order_value_usd" => format!("{:.2}", expected_order_value),
            "min_spread_bps" => format!("{:.1}", best_chromosome.min_spread_bps),
            "expected_spread_capture" => format!("{:.2}", expected_spread_capture),
            "expected_commission" => format!("{:.2}", expected_commission_per_trade),
            "expected_net_per_trade" => format!("{:.2}", expected_net_per_trade),
            "viable" => (expected_net_per_trade > 0.0)
        );
        
        // Update generation to show all generations complete before final analysis
        let total_generations = params.generations as i32;
        if let Err(e) = self.job_queue.update_generation(&job.id, total_generations, total_generations).await {
            log_warn!(MAIN_LOGGER, "Job {} failed to update generation for final analysis: {}", job.id, e);
        }
        
        // Update phase for final analysis with granular sub-phases
        progress_tracker.broadcast_phase("final_analysis", serde_json::json!({
            "status": "starting",
            "sub_phase": "backtest",
            "current_generation": total_generations,
            "total_generations": total_generations,
            "monte_carlo_enabled": params.monte_carlo_enabled,
            "walk_forward_enabled": params.walk_forward_enabled,
            "data_points": data_for_backtest.len(),
        }));
        let _ = progress_tracker.persist_phase_change("final_analysis", serde_json::json!({
            "status": "starting",
            "sub_phase": "backtest",
            "current_generation": total_generations,
            "total_generations": total_generations,
            "monte_carlo_runs": params.monte_carlo_runs,
        })).await;
        let _ = progress_tracker.persist_progress(87.0).await; // 87% - starting final analysis
        
        // Run final backtest with best parameters to get full results
        log_info_structured!(GENETIC_LOGGER, "GA_FULL_ANALYSIS_PIPELINE_START",
            "event" => "GA_FULL_ANALYSIS_PIPELINE_START",
            "job_id" => &job.id,
            "monte_carlo_enabled" => params.monte_carlo_enabled,
            "walk_forward_enabled" => params.walk_forward_enabled
        );
        log_debug!(MAIN_LOGGER, "🚀 About to run final backtest for job {}", job.id);
        
        // ============ TIMING: Final Analysis (Backtest + MC + WF) ============
        let timing_analysis_start = std::time::Instant::now();
        
        let mut final_result = self.run_full_analysis_with_chromosome(
            &job.id,
            &params.symbol,
            &params.exchange,
            params.initial_capital,
            params.initial_inventory_pct,
            params.commission_rate,
            params.initial_30d_volume,  // Pass through starting volume for fee tier
            &best_chromosome,
            &data_for_backtest,      // Full data for backtest + Monte Carlo
            &data_for_walkforward,   // Full data for walk-forward
            params.monte_carlo_enabled,
            params.monte_carlo_runs, // Use requested MC runs (no cap)
            params.walk_forward_enabled,
            &params.fill_model,  // Fill model realism level
        ).await?;
        
        let timing_analysis_elapsed = timing_analysis_start.elapsed();
        
        // Send timing to Kibana with structured metadata
        log_info_structured!(MAIN_LOGGER, "TIMING_FINAL_ANALYSIS",
            "event" => "TIMING_FINAL_ANALYSIS",
            "job_id" => &job.id,
            "elapsed_sec" => format!("{:.2}", timing_analysis_elapsed.as_secs_f64()),
            "elapsed_min" => format!("{:.1}", timing_analysis_elapsed.as_secs_f64() / 60.0),
            "data_points" => data_for_backtest.len(),
            "monte_carlo_enabled" => params.monte_carlo_enabled,
            "walk_forward_enabled" => params.walk_forward_enabled
        );
        
        // Update UI with final analysis complete
        progress_tracker.broadcast_phase("final_analysis", serde_json::json!({
            "status": "complete",
            "elapsed_sec": timing_analysis_elapsed.as_secs_f64(),
            "total_pnl": final_result.total_pnl,
        }));
        let _ = progress_tracker.persist_progress(98.0).await; // 98% - analysis complete
        
        // If strategy was below viability threshold, add warning to result
        if strategy_below_threshold {
            final_result.risk_halt_reason = Some(format!(
                "NOT_VIABLE: Best fitness {:.6} < 0.01 threshold. Commission costs exceeded 80% of gross profits. \
                This market/timeframe may not be suitable for market-making.",
                best_fitness
            ));
        }
        
        // Final broadcast + persist (lifecycle event) with comprehensive timing
        let timing_total_elapsed = timing_total_start.elapsed();
        
        // Send comprehensive timing summary to Kibana with structured metadata
        log_info_structured!(MAIN_LOGGER, "TIMING_JOB_COMPLETE",
            "event" => "TIMING_JOB_COMPLETE",
            "job_id" => &job.id,
            "total_elapsed_sec" => format!("{:.2}", timing_total_elapsed.as_secs_f64()),
            "total_elapsed_min" => format!("{:.1}", timing_total_elapsed.as_secs_f64() / 60.0),
            "data_load_sec" => format!("{:.2}", timing_data_load_elapsed.as_secs_f64()),
            "ga_sec" => format!("{:.2}", timing_ga_elapsed.as_secs_f64()),
            "analysis_sec" => format!("{:.2}", timing_analysis_elapsed.as_secs_f64()),
            "data_points" => data_for_walkforward.len(),
            "total_pnl" => format!("{:.2}", final_result.total_pnl),
            "sharpe_ratio" => format!("{:.3}", final_result.sharpe_ratio)
        );
        
        progress_tracker.broadcast_progress(100.0);
        progress_tracker.broadcast_phase("completed", serde_json::json!({
            "total_pnl": final_result.total_pnl,
            "sharpe_ratio": final_result.sharpe_ratio,
            "total_elapsed_sec": timing_total_elapsed.as_secs_f64(),
            "timing": {
                "data_load_sec": timing_data_load_elapsed.as_secs_f64(),
                "ga_sec": timing_ga_elapsed.as_secs_f64(),
                "analysis_sec": timing_analysis_elapsed.as_secs_f64(),
            }
        }));
        let _ = progress_tracker.persist_progress(100.0).await;
        let _ = progress_tracker.persist_phase_change("completed", serde_json::json!({
            "total_pnl": final_result.total_pnl,
            "sharpe_ratio": final_result.sharpe_ratio,
            "total_elapsed_sec": timing_total_elapsed.as_secs_f64(),
        })).await;
        
        log_info_structured!(GENETIC_LOGGER, "GA_FULL_ANALYSIS_COMPLETE",
            "event" => "GA_FULL_ANALYSIS_COMPLETE",
            "job_id" => &job.id,
            "total_pnl" => format!("{:.2}", final_result.total_pnl),
            "sharpe_ratio" => format!("{:.3}", final_result.sharpe_ratio)
        );
        
        Ok(final_result)
    }
    
    /// Execute walk-forward validation: test fixed parameters across rolling time windows
    /// 
    /// Walk-forward VALIDATES that optimized parameters generalize across different
    /// market conditions. Uses the SAME parameters for all windows (no re-optimization).
    /// This is much faster than per-window optimization and tests true out-of-sample robustness.
    /// 
    /// For each window:
    /// 1. Uses fixed parameters from job (from prior GA or manual input)
    /// 2. Tests on the test window data (out-of-sample validation)
    /// 3. Slides forward and repeats
    async fn execute_walkforward_analysis(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use genetic::chromosome::AvellanedaStoikovChromosome;
        
        let params = &job.params;
        
        // Walk-forward configuration with defaults
        let optimization_days = params.walkforward_optimization_days.unwrap_or(180);
        let test_days = params.walkforward_test_days.unwrap_or(30);
        let step_days = params.walkforward_step_days.unwrap_or(30);
        
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_CONFIG",
            "event" => "WALKFORWARD_CONFIG",
            "job_id" => &job.id,
            "optimization_days" => optimization_days,
            "test_days" => test_days,
            "step_days" => step_days
        );
        log_debug!(MAIN_LOGGER, "📊 Walk-forward config: opt={}d, test={}d, step={}d", 
            optimization_days, test_days, step_days);
        
        // Update phase
        let _ = self.job_queue.update_phase(
            &job.id, 
            "walkforward_initialization",
            serde_json::json!({
                "optimization_days": optimization_days,
                "test_days": test_days,
                "step_days": step_days
            })
        ).await;
        
        // Load ALL market data for the entire date range
        log_debug!(WALKFORWARD_LOGGER, "Job {} loading market data for {} on {}", job.id, params.symbol, params.exchange);
        let all_data = self.load_market_data_for_strategy(
            &params.symbol, 
            &params.exchange, 
            &params.strategy_type
        ).await?;
        
        if all_data.is_empty() {
            log_error!(WALKFORWARD_LOGGER, "Job {} no market data available", job.id);
            return Err("No market data available for walk-forward analysis".into());
        }
        
        // Determine date range from data
        let (data_start, data_end) = self.get_data_date_range(&all_data)?;
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_DATA_RANGE",
            "event" => "WALKFORWARD_DATA_RANGE",
            "job_id" => &job.id,
            "data_start" => data_start.format("%Y-%m-%d").to_string(),
            "data_end" => data_end.format("%Y-%m-%d").to_string(),
            "data_points" => all_data.len()
        );
        log_debug!(MAIN_LOGGER, "📅 Data range: {} to {}", data_start, data_end);
        
        // Generate walk-forward windows
        let windows = self.generate_walkforward_windows(
            data_start,
            data_end,
            optimization_days,
            test_days,
            step_days,
        );
        
        if windows.is_empty() {
            log_error!(WALKFORWARD_LOGGER, "Job {} not enough data for walk-forward analysis", job.id);
            return Err("Not enough data for walk-forward analysis. Need at least one complete window.".into());
        }
        
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOWS_GENERATED",
            "event" => "WALKFORWARD_WINDOWS_GENERATED",
            "job_id" => &job.id,
            "window_count" => windows.len()
        );
        log_debug!(MAIN_LOGGER, "🔢 Generated {} walk-forward windows", windows.len());
        
        // Track results for each window
        let mut window_results: Vec<WalkForwardWindowResult> = Vec::new();
        let mut aggregated_equity_curve: Vec<f64> = Vec::new();
        let total_windows = windows.len();
        
        // Use FIXED parameters from job - no per-window optimization
        // These come from prior GA run or manual input
        let fixed_chromosome = AvellanedaStoikovChromosome {
            risk_aversion: params.risk_aversion,
            inventory_target: params.inventory_target,
            order_size_pct: params.order_size_pct,
            window_ms: params.window_ms,
            min_quote_lifetime_ms: params.min_quote_lifetime_ms,
            volatility_cap: params.volatility_cap,
            time_horizon_minutes: params.time_horizon_minutes as f64,
            market_depth_k: params.market_depth_k,
            volatility_adjustment_factor: params.volatility_adjustment_factor,
            inventory_penalty_factor: params.inventory_penalty_factor,
            trend_sensitivity: params.trend_sensitivity,
            flow_sensitivity: params.flow_sensitivity,
            min_spread_bps: params.min_spread_bps,
            max_trades_per_minute: params.max_trades_per_minute,
            adverse_selection_sensitivity: params.adverse_selection_sensitivity,
            fee_spread_multiplier: params.fee_spread_multiplier,
            max_inventory_multiple: params.max_inventory_multiple,
            quote_withdrawal_threshold: 0.6,  // Default: withdraw quotes when flow is 60%+ one-sided
        };
        
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_FIXED_PARAMS",
            "event" => "WALKFORWARD_FIXED_PARAMS",
            "job_id" => &job.id,
            "risk_aversion" => format!("{:.3}", fixed_chromosome.risk_aversion),
            "inventory_target" => format!("{:.3}", fixed_chromosome.inventory_target),
            "order_size_pct" => format!("{:.2}", fixed_chromosome.order_size_pct * 100.0)
        );
        
        for (window_idx, window) in windows.iter().enumerate() {
            let window_num = window_idx + 1;
            
            log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOW_START",
                "event" => "WALKFORWARD_WINDOW_START",
                "job_id" => &job.id,
                "window_num" => window_num,
                "total_windows" => total_windows,
                "test_start" => window.test_start.format("%Y-%m-%d").to_string(),
                "test_end" => window.test_end.format("%Y-%m-%d").to_string()
            );
            
            // Update progress
            let progress = (window_idx as f64 / total_windows as f64) * 90.0;
            let _ = self.job_queue.update_progress(&job.id, progress).await;
            let _ = self.job_queue.update_phase(
                &job.id,
                "walkforward_window",
                serde_json::json!({
                    "current_window": window_num,
                    "total_windows": total_windows,
                    "phase": "validating"
                })
            ).await;
            
            // Filter data for test window only (validation is out-of-sample)
            let test_data: Vec<MarketData> = all_data.iter()
                .filter(|d| {
                    let ts = self.get_market_data_timestamp(d);
                    ts >= window.test_start && ts < window.test_end
                })
                .cloned()
                .collect();
            
            if test_data.is_empty() {
                log_warn!(WALKFORWARD_LOGGER, "Job {} window {} no test data, skipping", job.id, window_num);
                continue;
            }
            
            log_debug!(WALKFORWARD_LOGGER, "Job {} window {} validating on {} out-of-sample points", job.id, window_num, test_data.len());
            
            // Run backtest on out-of-sample data with FIXED parameters
            let test_result = self.run_backtest_with_chromosome(
                &format!("{}-window{}", job.id, window_num),
                &params.symbol,
                &params.exchange,
                params.initial_capital,
                params.initial_inventory_pct,
                params.commission_rate,
                &fixed_chromosome,
                &test_data,
            ).await?;
            
            log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOW_OOS_RESULTS",
                "event" => "WALKFORWARD_WINDOW_OOS_RESULTS",
                "job_id" => &job.id,
                "window_num" => window_num,
                "pnl" => format!("{:.2}", test_result.total_pnl),
                "sharpe_ratio" => format!("{:.3}", test_result.sharpe_ratio),
                "trade_count" => test_result.total_trades,
                "max_drawdown_pct" => format!("{:.2}", test_result.max_drawdown * 100.0)
            );
            
            // Store window result (params are same for all windows in validation mode)
            window_results.push(WalkForwardWindowResult {
                window_id: window_num as u32,
                opt_start: window.opt_start,
                opt_end: window.opt_end,
                test_start: window.test_start,
                test_end: window.test_end,
                optimal_params: OptimalParams {
                    risk_aversion: fixed_chromosome.risk_aversion,
                    inventory_target: fixed_chromosome.inventory_target,
                    order_size_pct: fixed_chromosome.order_size_pct,
                },
                optimization_fitness: 0.0, // Not applicable for validation mode
                oos_pnl: test_result.total_pnl,
                oos_sharpe: test_result.sharpe_ratio,
                oos_max_drawdown: test_result.max_drawdown,
                oos_win_rate: test_result.win_rate,
                oos_trades: test_result.total_trades,
            });
            
            // Append test period equity curve to aggregated curve
            aggregated_equity_curve.extend(test_result.equity_curve);
        }
        
        if window_results.is_empty() {
            log_error!(WALKFORWARD_LOGGER, "Job {} all walk-forward windows failed", job.id);
            return Err("All walk-forward windows failed".into());
        }
        
        // Calculate aggregated metrics across all windows
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_AGGREGATING",
            "event" => "WALKFORWARD_AGGREGATING",
            "job_id" => &job.id,
            "window_count" => window_results.len()
        );
        log_debug!(MAIN_LOGGER, "\n📊 Aggregating {} window results...", window_results.len());
        
        let total_oos_pnl: f64 = window_results.iter().map(|w| w.oos_pnl).sum();
        let avg_oos_sharpe: f64 = window_results.iter().map(|w| w.oos_sharpe).sum::<f64>() 
            / window_results.len() as f64;
        let max_oos_drawdown: f64 = window_results.iter()
            .map(|w| w.oos_max_drawdown)
            .fold(0.0, f64::max);
        let avg_oos_win_rate: f64 = window_results.iter().map(|w| w.oos_win_rate).sum::<f64>() 
            / window_results.len() as f64;
        let total_oos_trades: usize = window_results.iter().map(|w| w.oos_trades).sum();
        
        // Calculate parameter stability (how consistent are optimal params across windows)
        let param_stability = self.calculate_parameter_stability(&window_results);
        
        log_info_structured!(WALKFORWARD_LOGGER, "WALKFORWARD_COMPLETE",
            "event" => "WALKFORWARD_COMPLETE",
            "job_id" => &job.id,
            "windows" => window_results.len(),
            "total_pnl" => format!("{:.2}", total_oos_pnl),
            "avg_sharpe" => format!("{:.3}", avg_oos_sharpe),
            "max_drawdown_pct" => format!("{:.2}", max_oos_drawdown * 100.0),
            "param_stability_pct" => format!("{:.2}", param_stability * 100.0)
        );
        
        log_debug!(MAIN_LOGGER, "✅ Walk-forward complete:");
        log_debug!(MAIN_LOGGER, "   Total OOS PnL: {:.2}", total_oos_pnl);
        log_debug!(MAIN_LOGGER, "   Avg OOS Sharpe: {:.3}", avg_oos_sharpe);
        log_debug!(MAIN_LOGGER, "   Max OOS Drawdown: {:.2}%", max_oos_drawdown * 100.0);
        log_debug!(MAIN_LOGGER, "   Parameter Stability: {:.2}%", param_stability * 100.0);
        
        // Store walk-forward specific details in phase
        let _ = self.job_queue.update_phase(
            &job.id,
            "walkforward_complete",
            serde_json::json!({
                "windows_completed": window_results.len(),
                "total_windows": total_windows,
                "parameter_stability": param_stability,
                "window_details": window_results.iter().map(|w| serde_json::json!({
                    "window_id": w.window_id,
                    "oos_pnl": w.oos_pnl,
                    "oos_sharpe": w.oos_sharpe,
                    "optimal_params": {
                        "risk_aversion": w.optimal_params.risk_aversion,
                        "inventory_target": w.optimal_params.inventory_target,
                        "order_size_pct": w.optimal_params.order_size_pct,
                    }
                })).collect::<Vec<_>>()
            })
        ).await;
        
        // Calculate profit factor from aggregated results
        let (total_profit, total_loss) = window_results.iter().fold((0.0, 0.0), |(p, l), w| {
            if w.oos_pnl > 0.0 { (p + w.oos_pnl, l) } else { (p, l + w.oos_pnl.abs()) }
        });
        let profit_factor = if total_loss > 0.0 { total_profit / total_loss } else { 1.0 };
        
        let _ = self.job_queue.update_progress(&job.id, 100.0).await;
        
        Ok(BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some(job.params.strategy_type.clone()),
            total_pnl: total_oos_pnl,
            sharpe_ratio: avg_oos_sharpe,
            max_drawdown: max_oos_drawdown,
            win_rate: avg_oos_win_rate,
            total_trades: total_oos_trades,
            profit_factor,
            equity_curve: aggregated_equity_curve,
            completed_at: Utc::now(),
            // Walk-forward aggregates OOS results, breakdown not tracked
            closed_trades: None,
            open_positions: None,
            realized_pnl: None,
            unrealized_pnl: None,
            annualized_return: None,
            volatility: None,
            sortino_ratio: None,
            calmar_ratio: None,
            avg_trade_return: None,
            best_trade: None,
            worst_trade: None,
            avg_time_in_trade: None,
            value_at_risk_95: None,
            expected_shortfall_95: None,
            max_drawdown_duration_days: None,
            current_drawdown: None,
            avg_drawdown: None,
            total_orders: Some(total_oos_trades as i32),
            filled_orders: Some(total_oos_trades as i32),
            cancelled_orders: Some(0),
            avg_slippage: None,
            total_commission_paid: None,
            strategy_params: None,
            start_date: job.params.start_date.clone(),
            end_date: job.params.end_date.clone(),
            monte_carlo: None, // Walk-forward uses aggregated OOS results
            walk_forward: None, // This IS the walk-forward result
            inventory_metrics: None, // Not tracked for WF windows
            execution_metrics: None, // Not tracked for WF windows
            risk_halt_reason: None,
            metrics_profile: Some(Self::build_metrics_profile(&job.params.strategy_type)),
            is_portfolio: false,
            portfolio_metrics: None,
            routing_metrics: None,
        })
    }
    
    /// Generate walk-forward windows from start to end date
    fn generate_walkforward_windows(
        &self,
        start: chrono::DateTime<Utc>,
        end: chrono::DateTime<Utc>,
        optimization_days: i64,
        test_days: i64,
        step_days: i64,
    ) -> Vec<WalkForwardWindow> {
        let mut windows = Vec::new();
        let mut opt_start = start;
        
        loop {
            let opt_end = opt_start + chrono::Duration::days(optimization_days);
            let test_start = opt_end;
            let test_end = test_start + chrono::Duration::days(test_days);
            
            // Stop if test window extends beyond available data
            if test_end > end {
                break;
            }
            
            windows.push(WalkForwardWindow {
                opt_start,
                opt_end,
                test_start,
                test_end,
            });
            
            // Slide forward
            opt_start = opt_start + chrono::Duration::days(step_days);
        }
        
        windows
    }
    
    /// Get a reference price from market data (first available price)
    /// Used for converting order_size_pct to asset units (works for any trading pair)
    #[cfg(feature = "postgres")]
    fn get_reference_price(data: &[MarketData]) -> f64 {
        data.first().map(|d| {
            match d {
                MarketData::Candle(c) => c.close.to_f64().unwrap_or(100_000.0),
                MarketData::Order(o) => o.price_level.to_f64().unwrap_or(100_000.0),
                MarketData::Snapshot(s) => s.price_level.to_f64().unwrap_or(100_000.0),
                MarketData::Trade(t) => t.price.to_f64().unwrap_or(100_000.0),
            }
        }).unwrap_or(100_000.0)
    }
    
    /// Get a reference price from market data (first available price)
    /// Used for converting order_size_pct to asset units (works for any trading pair)
    #[cfg(not(feature = "postgres"))]
    fn get_reference_price(data: &[MarketData]) -> f64 {
        data.first().map(|d| {
            match d {
                MarketData::Candle(c) => c.close.to_f64().unwrap_or(100_000.0),
                MarketData::Trade(t) => t.price.to_f64().unwrap_or(100_000.0),
                #[allow(unreachable_patterns)]
                _ => 100_000.0,
            }
        }).unwrap_or(100_000.0)
    }
    
    /// Get the first timestamp from market data (for initializing portfolio open_time)
    #[cfg(feature = "postgres")]
    fn get_first_timestamp(data: &[MarketData]) -> chrono::DateTime<Utc> {
        data.first().map(|d| {
            match d {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Order(o) => o.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Trade(t) => t.timestamp,
            }
        }).unwrap_or_else(Utc::now)
    }
    
    /// Get the first timestamp from market data (for initializing portfolio open_time)
    #[cfg(not(feature = "postgres"))]
    fn get_first_timestamp(data: &[MarketData]) -> chrono::DateTime<Utc> {
        data.first().map(|d| {
            match d {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
                #[allow(unreachable_patterns)]
                _ => Utc::now(),
            }
        }).unwrap_or_else(Utc::now)
    }
    
    /// Get the timestamp from MarketData (candle, order, snapshot, or trade)
    #[cfg(feature = "postgres")]
    fn get_market_data_timestamp(&self, data: &MarketData) -> chrono::DateTime<Utc> {
        match data {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Order(o) => o.timestamp,
            MarketData::Snapshot(s) => s.timestamp,
            MarketData::Trade(t) => t.timestamp,
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    fn get_market_data_timestamp(&self, data: &MarketData) -> chrono::DateTime<Utc> {
        match data {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Trade(t) => t.timestamp,
            #[allow(unreachable_patterns)]
            _ => Utc::now(),
        }
    }
    
    /// Log comprehensive backtest results as structured JSON for Kibana
    fn log_backtest_results(&self, job_id: &str, result: &BacktestResult) {
        // Determine assessment status
        // Note: win_rate may be None if no closed trades (all positions still open)
        let win_rate = result.win_rate;
        let has_meaningful_trades = result.total_trades >= 10;
        
        // Check if strategy was flagged as not viable (commission costs too high)
        let is_not_viable = result.risk_halt_reason.as_ref()
            .map(|r| r.starts_with("NOT_VIABLE"))
            .unwrap_or(false);
        
        // Check if Sharpe confidence interval crosses zero (statistically unreliable)
        let sharpe_unreliable = result.monte_carlo.as_ref()
            .map(|mc| mc.confidence_lower < 0.0 && mc.confidence_upper > 0.0)
            .unwrap_or(false);
        
        // Check commission ratio for status determination
        let commission = result.total_commission_paid.unwrap_or(0.0);
        let gross_pnl_for_status = result.total_pnl + commission;
        let high_commission_ratio = if gross_pnl_for_status > 0.0 { 
            commission / gross_pnl_for_status > 0.50 
        } else { 
            false 
        };
        
        let status = if is_not_viable {
            "NOT_VIABLE"
        } else if high_commission_ratio {
            "HIGH_COMMISSION"
        } else if result.total_pnl > 0.0 && result.sharpe_ratio > 0.5 && win_rate > 0.3 && has_meaningful_trades && !sharpe_unreliable {
            "PROFITABLE"
        } else if result.total_pnl > 0.0 && sharpe_unreliable {
            "STATISTICALLY_UNRELIABLE"
        } else if result.total_pnl > 0.0 && !has_meaningful_trades {
            // Profitable but with too few trades - may be unrealized gains
            "INSUFFICIENT_TRADES"
        } else if result.total_pnl > 0.0 {
            "MARGINALLY_PROFITABLE"
        } else if result.sharpe_ratio > 0.0 {
            "LOSING_POSITIVE_SHARPE"
        } else {
            "UNPROFITABLE"
        };

        // Calculate derived metrics
        let gross_pnl = result.total_pnl + commission;
        let cost_ratio = if gross_pnl.abs() > 0.0 { commission / gross_pnl.abs() * 100.0 } else { 0.0 };
        let fill_rate = if result.total_orders.unwrap_or(0) > 0 {
            result.filled_orders.unwrap_or(0) as f64 / result.total_orders.unwrap_or(1) as f64 * 100.0
        } else { 0.0 };
        
        // Initial capital is the first value in equity curve
        let initial_capital = result.equity_curve.first().copied().unwrap_or(100_000.0);

        // Build Monte Carlo JSON section (VaR values are in USD, also show as % of capital)
        let mc_json = if let Some(ref mc) = result.monte_carlo {
            let var_pct = if initial_capital > 0.0 { mc.var_95 / initial_capital * 100.0 } else { 0.0 };
            let cvar_pct = if initial_capital > 0.0 { mc.cvar_95 / initial_capital * 100.0 } else { 0.0 };
            let var_warning = if mc.var_unreliable { r#", "var_unreliable": true, "var_warning": "VaR calculated from <30 data points - statistically unreliable""# } else { "" };
            format!(r#", "monte_carlo": {{"num_runs": {}, "data_points": {}, "mean_sharpe": {:.4}, "stddev_sharpe": {:.4}, "var_95_usd": {:.2}, "var_95_pct": {:.2}, "cvar_95_usd": {:.2}, "cvar_95_pct": {:.2}{}, "confidence_lower": {:.2}, "confidence_upper": {:.2}, "p5_pnl": {:.2}, "p50_pnl": {:.2}, "p95_pnl": {:.2}, "p5_drawdown_pct": {:.2}, "p50_drawdown_pct": {:.2}, "p95_drawdown_pct": {:.2}, "p5_win_rate_pct": {:.2}, "p50_win_rate_pct": {:.2}, "p95_win_rate_pct": {:.2}, "p50_sharpe": {:.4}}}"#,
                mc.num_runs,
                mc.data_points,
                mc.mean_sharpe,
                mc.stddev_sharpe,
                mc.var_95,   // Absolute $ value
                var_pct,     // As percentage of capital
                mc.cvar_95,  // Absolute $ value
                cvar_pct,    // As percentage of capital
                var_warning, // Warning if unreliable
                mc.confidence_lower,
                mc.confidence_upper,
                mc.p5_total_pnl.unwrap_or(0.0),
                mc.p50_total_pnl.unwrap_or(0.0),
                mc.p95_total_pnl.unwrap_or(0.0),
                mc.p5_max_drawdown.unwrap_or(0.0) * 100.0,
                mc.p50_max_drawdown.unwrap_or(0.0) * 100.0,
                mc.p95_max_drawdown.unwrap_or(0.0) * 100.0,
                mc.p5_win_rate.unwrap_or(0.0) * 100.0,
                mc.p50_win_rate.unwrap_or(0.0) * 100.0,
                mc.p95_win_rate.unwrap_or(0.0) * 100.0,
                mc.p50_sharpe.unwrap_or(0.0)
            )
        } else {
            String::new()
        };

        // Build Walk-Forward JSON section
        let wf_json = if let Some(ref wf) = result.walk_forward {
            format!(r#", "walk_forward": {{"periods_tested": {}, "avg_performance_pct": {:.2}, "consistency_pct": {:.2}, "oos_degradation_pct": {:.2}}}"#,
                wf.periods_tested,
                wf.average_performance * 100.0,
                wf.consistency_score * 100.0,
                wf.out_of_sample_degradation * 100.0
            )
        } else {
            String::new()
        };

        // Build Inventory Metrics JSON section (for market making strategies)
        let inv_json = if let Some(ref inv) = result.inventory_metrics {
            format!(r#", "inventory": {{"turnover_per_day": {:.2}, "buy_volume": {:.2}, "sell_volume": {:.2}, "avg_value": {:.2}, "buy_fills": {}, "sell_fills": {}, "imbalance": {:.4}}}"#,
                inv.turnover_per_day,
                inv.buy_volume,
                inv.sell_volume,
                inv.avg_inventory_value,
                inv.buy_fills,
                inv.sell_fills,
                inv.imbalance_ratio
            )
        } else {
            String::new()
        };

        // Build Execution Metrics JSON section (order lifecycle tracking)
        let exec_json = if let Some(ref ex) = result.execution_metrics {
            format!(r#", "execution_detail": {{"orders_placed": {}, "orders_filled": {}, "orders_cancelled": {}, "total_fills": {}, "fill_rate_pct": {:.2}, "cancel_rate_pct": {:.2}, "avg_fills_per_order": {:.2}, "avg_time_to_fill_sec": {:.3}, "median_time_to_fill_sec": {:.3}, "bid_orders": {}, "ask_orders": {}, "bid_fills": {}, "ask_fills": {}, "bid_fill_rate_pct": {:.2}, "ask_fill_rate_pct": {:.2}, "avg_quote_spread_bps": {:.2}}}"#,
                ex.orders_placed,
                ex.orders_filled,
                ex.orders_cancelled,
                ex.total_fills,
                ex.fill_rate * 100.0,
                ex.cancellation_rate * 100.0,
                ex.avg_fills_per_order,
                ex.avg_time_to_fill_seconds,
                ex.median_time_to_fill_seconds,
                ex.bid_orders,
                ex.ask_orders,
                ex.bid_fills,
                ex.ask_fills,
                ex.bid_fill_rate * 100.0,
                ex.ask_fill_rate * 100.0,
                ex.avg_quote_spread_bps
            )
        } else {
            String::new()
        };

        // Log single structured JSON entry - Kibana can parse and index all fields
        // VaR values are in USD, calculate percentages for easier interpretation
        let var_95_usd = result.value_at_risk_95.unwrap_or(0.0);
        let cvar_95_usd = result.expected_shortfall_95.unwrap_or(0.0);
        let var_95_pct = if initial_capital > 0.0 { var_95_usd / initial_capital * 100.0 } else { 0.0 };
        let cvar_95_pct = if initial_capital > 0.0 { cvar_95_usd / initial_capital * 100.0 } else { 0.0 };
        
        log_info!(MAIN_LOGGER, 
            r#"{{"event": "BACKTEST_RESULT", "job_id": "{}", "status": "{}", "initial_capital": {:.2}, "performance": {{"total_pnl": {:.2}, "gross_pnl": {:.2}, "sharpe_ratio": {:.4}, "max_drawdown_pct": {:.2}, "win_rate_pct": {:.2}, "profit_factor": {:.4}, "total_trades": {}}}, "returns": {{"annualized_pct": {:.2}, "volatility_pct": {:.2}, "sortino_ratio": {:.4}, "calmar_ratio": {:.4}}}, "trades": {{"avg_return": {:.4}, "avg_time_min": {:.1}, "best_trade": {:.2}, "worst_trade": {:.2}}}, "risk": {{"var_95_usd": {:.2}, "var_95_pct": {:.2}, "cvar_95_usd": {:.2}, "cvar_95_pct": {:.2}, "current_drawdown_pct": {:.2}, "avg_drawdown_pct": {:.2}, "max_dd_days": {}}}, "execution": {{"commission": {:.2}, "commission_ratio_pct": {:.1}, "avg_slippage_pct": {:.4}, "total_orders": {}, "filled_orders": {}, "cancelled_orders": {}, "fill_rate_pct": {:.1}}}{}{}{}{}}}"#,
            job_id,
            status,
            initial_capital,
            // Performance
            result.total_pnl,
            gross_pnl,
            result.sharpe_ratio,
            result.max_drawdown * 100.0,
            result.win_rate * 100.0,
            result.profit_factor,
            result.total_trades,
            // Returns
            result.annualized_return.unwrap_or(0.0) * 100.0,
            result.volatility.unwrap_or(0.0) * 100.0,
            result.sortino_ratio.unwrap_or(0.0),
            result.calmar_ratio.unwrap_or(0.0),
            // Trades (avg_time_in_trade is stored in seconds, convert to minutes for display)
            result.avg_trade_return.unwrap_or(0.0),
            result.avg_time_in_trade.unwrap_or(0.0) / 60.0,  // Convert seconds to minutes
            result.best_trade.unwrap_or(0.0),
            result.worst_trade.unwrap_or(0.0),
            // Risk (VaR/CVaR are positive values representing potential loss in USD and %)
            var_95_usd,
            var_95_pct,
            cvar_95_usd,
            cvar_95_pct,
            result.current_drawdown.unwrap_or(0.0) * 100.0,
            result.avg_drawdown.unwrap_or(0.0) * 100.0,
            result.max_drawdown_duration_days.unwrap_or(0),
            // Execution
            commission,
            cost_ratio,
            result.avg_slippage.unwrap_or(0.0) * 100.0,
            result.total_orders.unwrap_or(0),
            result.filled_orders.unwrap_or(0),
            result.cancelled_orders.unwrap_or(0),
            fill_rate,
            // Optional sections
            mc_json,
            wf_json,
            inv_json,
            exec_json
        );
        
        // Log detailed per-trade economics for strategy improvement insights
        let avg_gross_per_trade = if result.total_trades > 0 { 
            gross_pnl / result.total_trades as f64 
        } else { 0.0 };
        let avg_commission_per_trade = if result.total_trades > 0 { 
            commission / result.total_trades as f64 
        } else { 0.0 };
        let avg_net_per_trade = if result.total_trades > 0 { 
            result.total_pnl / result.total_trades as f64 
        } else { 0.0 };
        
        // Calculate how much spread increase is needed to break even
        // If commission is 130% of gross, need to increase spread by 30% to break even
        let spread_increase_needed_pct = if avg_gross_per_trade > 0.0 {
            ((avg_commission_per_trade / avg_gross_per_trade) - 1.0).max(0.0) * 100.0
        } else { 0.0 };
        
        log_info!(MAIN_LOGGER,
            r#"{{"event": "TRADE_ECONOMICS", "job_id": "{}", "per_trade": {{"avg_gross": {:.2}, "avg_commission": {:.2}, "avg_net": {:.2}, "commission_pct_of_gross": {:.1}}}, "breakeven": {{"spread_increase_needed_pct": {:.1}, "min_profit_per_trade": {:.2}}}, "efficiency": {{"trades_per_dollar_commission": {:.2}, "gross_to_net_ratio": {:.2}}}}}"#,
            job_id,
            avg_gross_per_trade,
            avg_commission_per_trade,
            avg_net_per_trade,
            if avg_gross_per_trade > 0.0 { (avg_commission_per_trade / avg_gross_per_trade) * 100.0 } else { 0.0 },
            spread_increase_needed_pct,
            avg_commission_per_trade,  // This is the minimum profit needed per trade
            if commission > 0.0 { result.total_trades as f64 / commission } else { 0.0 },
            if gross_pnl != 0.0 { result.total_pnl / gross_pnl } else { 0.0 }
        );
    }
    
    /// Get the date range of market data
    fn get_data_date_range(&self, data: &[MarketData]) -> Result<(chrono::DateTime<Utc>, chrono::DateTime<Utc>), Box<dyn std::error::Error + Send + Sync>> {
        if data.is_empty() {
            return Err("No data to determine date range".into());
        }
        
        let first_ts = self.get_market_data_timestamp(&data[0]);
        let last_ts = self.get_market_data_timestamp(&data[data.len() - 1]);
        
        Ok((first_ts, last_ts))
    }
    
    /// Calculate parameter stability across windows (0.0 = highly variable, 1.0 = perfectly stable)
    fn calculate_parameter_stability(&self, windows: &[WalkForwardWindowResult]) -> f64 {
        if windows.len() < 2 {
            return 1.0;
        }
        
        // Calculate coefficient of variation for each parameter
        let calc_stability = |values: &[f64]| -> f64 {
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            if mean.abs() < 1e-10 {
                return 1.0;
            }
            let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
            let cv = variance.sqrt() / mean.abs();
            1.0 / (1.0 + cv) // Higher stability for lower CV
        };
        
        let risk_aversion_stability = calc_stability(
            &windows.iter().map(|w| w.optimal_params.risk_aversion).collect::<Vec<_>>()
        );
        let inventory_target_stability = calc_stability(
            &windows.iter().map(|w| w.optimal_params.inventory_target).collect::<Vec<_>>()
        );
        let order_size_stability = calc_stability(
            &windows.iter().map(|w| w.optimal_params.order_size_pct).collect::<Vec<_>>()
        );
        
        (risk_aversion_stability + inventory_target_stability + order_size_stability) / 3.0
    }
    
    /// Run full analysis with Monte Carlo and Walk-Forward enabled
    /// Used as the final step after genetic optimization
    /// 
    /// backtest_data: Sampled data (200K) for fast backtest + Monte Carlo
    /// walkforward_data: Full data for walk-forward (engine samples per-window)
    #[cfg(feature = "postgres")]
    #[allow(clippy::too_many_arguments)]
    async fn run_full_analysis_with_chromosome(
        &self,
        job_id: &str,
        _symbol: &str,  // Reserved for future use
        exchange: &str,
        initial_capital: f64,
        _initial_inventory_pct: f64,  // Reserved for future use (RealisticSimulator uses order_size_pct)
        _commission_rate: f64,  // Reserved for future use (using exchange_fee_config instead)
        initial_30d_volume: f64,  // Starting 30-day volume for fee tier calculation
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        backtest_data: &[MarketData],
        walkforward_data: &[MarketData],
        monte_carlo_enabled: bool,
        monte_carlo_runs: usize,
        walk_forward_enabled: bool,
        fill_model: &str,  // Fill model realism level: "optimistic", "standard", "realistic", "pessimistic"
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        // Run backtest + Monte Carlo on sampled data for speed
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_ENTER | backtest_pts={} walkforward_pts={} mc={} wf={}", 
            job_id, backtest_data.len(), walkforward_data.len(), monte_carlo_enabled, walk_forward_enabled);
        
        // ============================================================================
        // REALISTIC SIMULATION (Unified with GA workers)
        // ============================================================================
        // This uses the same RealisticSimulator that GA workers use for chromosome
        // evaluation, ensuring consistency between optimization and final analysis.
        
        // Get exchange-specific configuration with starting 30-day volume
        let mut exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
        if initial_30d_volume > 0.0 {
            exchange_fee_config.starting_30d_volume = initial_30d_volume;
            // Update base fee rates to match the volume tier
            exchange_fee_config.maker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, true);
            exchange_fee_config.taker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, false);
            log_info!(MAIN_LOGGER, "Job {} using initial_30d_volume=${:.0} → maker_fee={:.4}% taker_fee={:.4}%",
                job_id, initial_30d_volume, exchange_fee_config.maker_fee * 100.0, exchange_fee_config.taker_fee * 100.0);
        }
        
        // Get initial price for inventory calculation
        let asset_price = Self::get_reference_price(backtest_data);
        
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_ASSET_PRICE | price={:.2}", job_id, asset_price);
        
        // ============================================================================
        // CONVERT DATA TO ORDER BOOK EVENTS
        // ============================================================================
        // Use the same data format that GA workers use for consistency
        use backtest::{RealisticSimulator, RealisticASParams};
        #[cfg(feature = "postgres")]
        use backtest::realistic_simulation::market_data_to_order_events;
        use backtest::monte_carlo;
        
        let events = market_data_to_order_events(backtest_data);
        log_info!(MAIN_LOGGER, "Job {} converted {} MarketData to {} OrderBookEvents", 
            job_id, backtest_data.len(), events.len());
        
        if events.is_empty() {
            return Err(format!("No order book events found in data for job {}", job_id).into());
        }
        
        // ============================================================================
        // BUILD REALISTIC SIMULATION PARAMETERS (same as GA workers)
        // ============================================================================
        let realistic_params = RealisticASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            initial_inventory_pct: _initial_inventory_pct,  // Use the passed parameter
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            max_inventory_multiple: chromosome.max_inventory_multiple,
            latency_mean_ms: exchange_fee_config.latency_mean_ms,
            latency_std_ms: exchange_fee_config.latency_std_ms,
            enable_partial_fills: true,
            min_fill_fraction: 0.1,
            max_orders_per_second: exchange_fee_config.max_orders_per_second,
            rate_limit_cooldown_ms: exchange_fee_config.rate_limit_cooldown_ms,
            funding_rate_8h: exchange_fee_config.funding_rate_8h,
            funding_interval_ms: exchange_fee_config.funding_interval_ms,
            adverse_selection_sensitivity: chromosome.adverse_selection_sensitivity,
            fee_spread_multiplier: chromosome.fee_spread_multiplier,
            // New advanced AS parameters from chromosome
            market_depth_k: chromosome.market_depth_k,
            volatility_adjustment_factor: chromosome.volatility_adjustment_factor,
            inventory_penalty_factor: chromosome.inventory_penalty_factor,
            trend_sensitivity: chromosome.trend_sensitivity,
            flow_sensitivity: chromosome.flow_sensitivity,
            take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
            stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
            max_trades_per_minute: chromosome.max_trades_per_minute,
            window_ms: chromosome.window_ms as i64,
            min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms as i64,
            quote_withdrawal_threshold: chromosome.quote_withdrawal_threshold,
            // Fill model from function parameter, with auto-detection from exchange
            fill_model: backtest::FillModelType::from_str_or_exchange(fill_model, exchange),
        };
        
        // Store params for walk-forward (will need to reconstruct for each window)
        let _chromosome_for_wf = chromosome.clone();
        
        // Log the phases we're about to run
        log_info!(MAIN_LOGGER, "Job {} starting UNIFIED REALISTIC SIMULATION pipeline: RealisticSim{}{}",
            job_id,
            if monte_carlo_enabled { format!(" → Monte Carlo ({} runs)", monte_carlo_runs) } else { "".to_string() },
            if walk_forward_enabled { " → Walk-Forward" } else { "" });
        
        // ============================================================================
        // RUN REALISTIC SIMULATION (same engine as GA workers)
        // ============================================================================
        log_info!(MAIN_LOGGER, "Job {} CALLING_REALISTIC_SIMULATOR | events={}", job_id, events.len());
        eprintln!("[SYNC] Job {} ABOUT_TO_CALL_REALISTIC_SIMULATOR | events={}", job_id, events.len());
        
        let backtest_start = std::time::Instant::now();
        
        let simulator = RealisticSimulator::with_exchange_config(initial_capital, exchange_fee_config.clone());
        let sim_result = simulator.run(&events, &realistic_params);
        
        let backtest_elapsed = backtest_start.elapsed();
        
        // Log simulation results (matching GA worker output format)
        eprintln!("✅ [FINAL_ANALYSIS] REALISTIC sim complete in {:.2}s: PnL={:.2}, Sharpe={:.4}, Trades={}", 
            backtest_elapsed.as_secs_f64(), sim_result.total_pnl, sim_result.sharpe_ratio, sim_result.num_trades);
        
        log_info!(MAIN_LOGGER, "Job {} REALISTIC_SIM_COMPLETE | elapsed={:.2}s pnl={:.2} sharpe={:.4} trades={} max_dd={:.2}%",
            job_id, backtest_elapsed.as_secs_f64(), sim_result.total_pnl, sim_result.sharpe_ratio, 
            sim_result.num_trades, sim_result.max_drawdown * 100.0);
        
        // Convert RealisticSimResult to BacktestResult for Monte Carlo and final output
        let core_result: backtest::types::BacktestResult = sim_result.clone().into();
        
        // ============================================================================
        // MONTE CARLO SIMULATION
        // ============================================================================
        let mc_result = if monte_carlo_enabled && sim_result.num_trades >= 30 {
            log_info!(MAIN_LOGGER, "Job {} running Monte Carlo with {} trades", job_id, sim_result.num_trades);
            let mc_start = std::time::Instant::now();
            
            match monte_carlo::run_monte_carlo_simulation(&core_result, monte_carlo_runs) {
                Ok(mc) => {
                    let mc_elapsed = mc_start.elapsed();
                    log_info!(MAIN_LOGGER, "Job {} Monte Carlo complete in {:.2}s | mean_sharpe={:.3} VaR95=${:.2}",
                        job_id, mc_elapsed.as_secs_f64(), mc.mean_metrics.sharpe_ratio, mc.var_95);
                    Some(mc)
                }
                Err(e) => {
                    log_warn!(MAIN_LOGGER, "Job {} Monte Carlo failed: {} - continuing without MC", job_id, e);
                    None
                }
            }
        } else {
            if monte_carlo_enabled {
                log_info!(MAIN_LOGGER, "Job {} skipping Monte Carlo: only {} trades (min 30 required)", 
                    job_id, sim_result.num_trades);
            }
            None
        };
        
        // Build unified result structure
        let mut result = backtest::BacktestResults {
            core: core_result,
            monte_carlo: mc_result,
            walk_forward: None,
            metadata: backtest::AnalysisMetadata {
                analysis_mode: config::AnalysisMode::Production,
                mode_escalated: false,
                total_runtime_seconds: 0.0,  // Will be updated at end
                data_quality_score: 1.0,
                triggers_fired: vec![],
                recommendations: vec![],
            },
        };
        
        let backtest_elapsed = backtest_start.elapsed();
        
        // SYNC PRINT after engine completes
        eprintln!("⏱️ TIMING | BACKTEST+MC | completed in {:.2}s ({:.1} min) | {} events @ {:.0}/sec", 
            backtest_elapsed.as_secs_f64(), backtest_elapsed.as_secs_f64() / 60.0,
            backtest_data.len(), backtest_data.len() as f64 / backtest_elapsed.as_secs_f64());
        
        // Structured metadata for Kibana
        log_info_structured!(MAIN_LOGGER, "TIMING_BACKTEST_COMPLETE",
            "event" => "TIMING_BACKTEST_COMPLETE",
            "job_id" => job_id,
            "elapsed_sec" => format!("{:.2}", backtest_elapsed.as_secs_f64()),
            "elapsed_min" => format!("{:.1}", backtest_elapsed.as_secs_f64() / 60.0),
            "data_points" => backtest_data.len(),
            "events_per_sec" => format!("{:.0}", backtest_data.len() as f64 / backtest_elapsed.as_secs_f64())
        );
        
        log_info!(MAIN_LOGGER, "Job {} REALISTIC_SIM_COMPLETE | elapsed={:.1}s trades={}", 
            job_id, backtest_start.elapsed().as_secs_f64(), result.core.num_trades);
        
        // Log simulation summary (no portfolio to inspect - RealisticSimulator manages internally)
        log_info!(MAIN_LOGGER, "Job {} final results | pnl={:.2} sharpe={:.4} trades={} win_rate={:.2}%",
            job_id, sim_result.total_pnl, sim_result.sharpe_ratio, sim_result.num_trades, sim_result.win_rate * 100.0
        );
        
        // Log Monte Carlo results if available
        if let Some(ref mc) = result.monte_carlo {
            // VaR and CVaR are in USD (absolute potential loss), calculate percentage of capital
            let var_pct = mc.var_95 / initial_capital * 100.0;
            let cvar_pct = mc.cvar_95 / initial_capital * 100.0;
            log_info!(MAIN_LOGGER, "Job {} Monte Carlo complete | runs={} mean_sharpe={:.3} VaR95=${:.2} ({:.2}%) CVaR95=${:.2} ({:.2}%)",
                job_id, mc.num_runs, mc.mean_metrics.sharpe_ratio, mc.var_95, var_pct, mc.cvar_95, cvar_pct);
            
            // Structured metadata for Kibana
            log_info_structured!(MAIN_LOGGER, "TIMING_MONTE_CARLO_COMPLETE",
                "event" => "TIMING_MONTE_CARLO_COMPLETE",
                "job_id" => job_id,
                "runs" => mc.num_runs,
                "mean_sharpe" => format!("{:.3}", mc.mean_metrics.sharpe_ratio),
                "var_95" => format!("{:.2}", mc.var_95),
                "cvar_95" => format!("{:.2}", mc.cvar_95),
                "var_pct" => format!("{:.2}", var_pct),
                "cvar_pct" => format!("{:.2}", cvar_pct)
            );
        }
        
        // Run walk-forward on OUT-OF-SAMPLE test data (not seen during GA optimization)
        // This validates the strategy generalizes to unseen time periods
        // The test data is separate from training data (default 20% held out)
        if walk_forward_enabled && !walkforward_data.is_empty() {
            log_info!(MAIN_LOGGER, "Job {} Walk-Forward starting on {} out-of-sample data points", 
                job_id, walkforward_data.len());
            
            // ============ TIMING: Walk-Forward ============
            let wf_timing_start = std::time::Instant::now();
            eprintln!("⏱️ TIMING | WALK_FORWARD | starting on {} OOS data points...", walkforward_data.len());
            
            // Convert walkforward data to events
            let wf_events = market_data_to_order_events(walkforward_data);
            
            if wf_events.len() >= 1000 {
                // Simple walk-forward: run simulation on walkforward data and compare performance
                let wf_simulator = RealisticSimulator::with_exchange_config(initial_capital, exchange_fee_config.clone());
                let wf_sim_result = wf_simulator.run(&wf_events, &realistic_params);
                
                let wf_timing_elapsed = wf_timing_start.elapsed();
                eprintln!("⏱️ TIMING | WALK_FORWARD | completed in {:.2}s ({:.1} min)", 
                    wf_timing_elapsed.as_secs_f64(), wf_timing_elapsed.as_secs_f64() / 60.0);
                
                // Calculate OOS degradation
                let oos_sharpe = wf_sim_result.sharpe_ratio;
                let is_sharpe = sim_result.sharpe_ratio;
                let degradation = if is_sharpe.abs() > 0.0001 {
                    (is_sharpe - oos_sharpe) / is_sharpe.abs()
                } else {
                    0.0
                };
                
                // Build simple walk-forward result
                result.walk_forward = Some(backtest::WalkForwardSummary {
                    periods_tested: 1,  // Single OOS test
                    average_performance: wf_sim_result.total_pnl / initial_capital,
                    consistency_score: if wf_sim_result.sharpe_ratio > 0.0 { 1.0 } else { 0.0 },
                    out_of_sample_degradation: degradation.max(0.0),
                });
                
                log_info!(MAIN_LOGGER, "Job {} Walk-Forward complete | OOS_sharpe={:.4} IS_sharpe={:.4} degradation={:.1}%",
                    job_id, oos_sharpe, is_sharpe, degradation * 100.0);
                
                log_info_structured!(MAIN_LOGGER, "TIMING_WALKFORWARD_COMPLETE",
                    "event" => "TIMING_WALKFORWARD_COMPLETE",
                    "job_id" => job_id,
                    "elapsed_sec" => format!("{:.2}", wf_timing_elapsed.as_secs_f64()),
                    "elapsed_min" => format!("{:.1}", wf_timing_elapsed.as_secs_f64() / 60.0),
                    "data_points" => walkforward_data.len(),
                    "oos_sharpe" => format!("{:.4}", oos_sharpe),
                    "is_sharpe" => format!("{:.4}", is_sharpe),
                    "degradation_pct" => format!("{:.1}", degradation * 100.0)
                );
            } else {
                log_warn!(MAIN_LOGGER, "Job {} Walk-Forward skipped: insufficient events ({})", job_id, wf_events.len());
            }
        } else if let Some(ref wf) = result.walk_forward {
            // Log if walk-forward was somehow already done
            log_info!(MAIN_LOGGER, "Job {} Walk-Forward complete | periods={} avg_perf={:.2}% consistency={:.1}% OOS_degradation={:.1}%",
                job_id, wf.periods_tested, wf.average_performance * 100.0, wf.consistency_score * 100.0, wf.out_of_sample_degradation * 100.0);
        }
        
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
        // Debug: Log core result values to trace why win_rate might be 0
        log_info!(MAIN_LOGGER, "Job {} CORE_RESULT | win_rate={:?} num_trades={} closed={} open={} pnl={:.2} volatility={:?}",
            job_id, result.core.win_rate, result.core.num_trades, result.core.closed_trades, 
            result.core.open_positions, total_pnl, result.core.volatility);
        
        // Convert Monte Carlo results if available
        let monte_carlo_summary = result.monte_carlo.as_ref().map(|mc| MonteCarloSummary {
            num_runs: mc.num_runs,
            mean_sharpe: mc.mean_metrics.sharpe_ratio,
            stddev_sharpe: mc.stddev_metrics.sharpe_ratio,
            var_95: mc.var_95,
            cvar_95: mc.cvar_95,
            confidence_lower: mc.percentile_metrics.get(&5).map(|m| m.sharpe_ratio).unwrap_or(0.0),
            confidence_upper: mc.percentile_metrics.get(&95).map(|m| m.sharpe_ratio).unwrap_or(0.0),
            data_points: mc.data_points,
            var_unreliable: mc.var_unreliable,
            // Percentile distributions
            p5_total_pnl: mc.percentile_metrics.get(&5).map(|m| m.net_profit),
            p50_total_pnl: mc.percentile_metrics.get(&50).map(|m| m.net_profit),
            p95_total_pnl: mc.percentile_metrics.get(&95).map(|m| m.net_profit),
            p5_max_drawdown: mc.percentile_metrics.get(&5).map(|m| m.max_drawdown),
            p50_max_drawdown: mc.percentile_metrics.get(&50).map(|m| m.max_drawdown),
            p95_max_drawdown: mc.percentile_metrics.get(&95).map(|m| m.max_drawdown),
            p50_sharpe: mc.percentile_metrics.get(&50).map(|m| m.sharpe_ratio),
            p5_win_rate: mc.percentile_metrics.get(&5).map(|m| m.win_rate),
            p50_win_rate: mc.percentile_metrics.get(&50).map(|m| m.win_rate),
            p95_win_rate: mc.percentile_metrics.get(&95).map(|m| m.win_rate),
        });
        
        // Convert Walk-Forward results if available
        let walk_forward_summary = result.walk_forward.as_ref().map(|wf| WalkForwardSummary {
            periods_tested: wf.periods_tested,
            average_performance: wf.average_performance,
            consistency_score: wf.consistency_score,
            out_of_sample_degradation: wf.out_of_sample_degradation,
        });
        
        Ok(BacktestResult {
            job_id: job_id.to_string(),
            strategy_type: Some("market_making".to_string()),
            total_pnl,
            sharpe_ratio: result.core.sharpe_ratio.unwrap_or(0.0),
            max_drawdown,
            win_rate: result.core.win_rate.unwrap_or(0.0),
            total_trades: result.core.num_trades,
            profit_factor: result.core.profit_factor.unwrap_or(1.0),
            equity_curve: result.core.equity_curve,
            completed_at: Utc::now(),
            // Trade breakdown from core result
            closed_trades: Some(result.core.closed_trades),
            open_positions: Some(result.core.open_positions),
            realized_pnl: result.core.realized_pnl,
            unrealized_pnl: result.core.unrealized_pnl,
            annualized_return: None,
            volatility: result.core.volatility,
            sortino_ratio: None,
            calmar_ratio: None,
            avg_trade_return: result.core.avg_trade_return,
            best_trade: result.core.gross_profit,
            worst_trade: result.core.gross_loss,
            avg_time_in_trade: result.core.avg_trade_duration,
            value_at_risk_95: monte_carlo_summary.as_ref().map(|mc| mc.var_95),
            expected_shortfall_95: monte_carlo_summary.as_ref().map(|mc| mc.cvar_95),
            max_drawdown_duration_days: None,
            current_drawdown: None,
            avg_drawdown: None,
            total_orders: result.core.transaction_costs.as_ref().map(|_| result.core.num_trades as i32),
            filled_orders: Some(result.core.num_trades as i32),
            cancelled_orders: Some(0),
            avg_slippage: result.core.transaction_costs.as_ref().map(|tc| tc.total_slippage / result.core.num_trades.max(1) as f64),
            total_commission_paid: result.core.transaction_costs.as_ref().map(|tc| tc.total_commission),
            strategy_params: None,
            start_date: None, // Internal genetic eval doesn't have job params
            end_date: None,
            monte_carlo: monte_carlo_summary,
            walk_forward: walk_forward_summary,
            inventory_metrics: result.core.inventory_metrics.as_ref().map(|inv| InventorySummary {
                total_volume: inv.total_volume_traded,
                avg_inventory_value: inv.avg_inventory_value,
                turnover_per_day: inv.turnover_per_day,
                buy_fills: inv.buy_fills,
                sell_fills: inv.sell_fills,
                buy_volume: inv.buy_volume,
                sell_volume: inv.sell_volume,
                imbalance_ratio: inv.imbalance_ratio,
                ..Default::default()
            }),
            execution_metrics: result.core.execution_metrics.as_ref().map(|ex| ExecutionSummary {
                orders_placed: ex.orders_placed,
                orders_filled: ex.orders_filled,
                orders_cancelled: ex.orders_cancelled,
                total_fills: ex.total_fills,
                fill_rate: ex.fill_rate,
                cancellation_rate: ex.cancellation_rate,
                avg_fills_per_order: ex.avg_fills_per_order,
                avg_time_to_fill_seconds: ex.avg_time_to_fill_seconds,
                median_time_to_fill_seconds: ex.median_time_to_fill_seconds,
                bid_orders: ex.bid_orders,
                ask_orders: ex.ask_orders,
                bid_fills: ex.bid_fills,
                ask_fills: ex.ask_fills,
                bid_fill_rate: ex.bid_fill_rate,
                ask_fill_rate: ex.ask_fill_rate,
                avg_quote_spread_bps: ex.avg_quote_spread_bps,
                round_trip_win_rate: ex.round_trip_win_rate,
                total_round_trips: ex.total_round_trips,
                profitable_round_trips: ex.profitable_round_trips,
                spread_capture_rate: ex.spread_capture_rate,
                adverse_selection_rate: ex.adverse_selection_rate,
                avg_pnl_per_round_trip: ex.avg_pnl_per_round_trip,
            }),
            risk_halt_reason: None,
            metrics_profile: Some(Self::build_metrics_profile("market_making")),
            is_portfolio: false,
            portfolio_metrics: None,
            routing_metrics: None,
        })
    }
    
    /// Stub version when postgres feature is not enabled
    #[cfg(not(feature = "postgres"))]
    #[allow(clippy::too_many_arguments)]
    async fn run_full_analysis_with_chromosome(
        &self,
        _job_id: &str,
        _symbol: &str,
        _exchange: &str,
        _initial_capital: f64,
        _initial_inventory_pct: f64,
        _commission_rate: f64,
        _initial_30d_volume: f64,
        _chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        _backtest_data: &[MarketData],
        _walkforward_data: &[MarketData],
        _monte_carlo_enabled: bool,
        _monte_carlo_runs: usize,
        _walk_forward_enabled: bool,
        _fill_model: &str,  // Fill model realism level
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        Err("Full analysis requires postgres feature to be enabled".into())
    }
    
    /// Used for fitness evaluation and walk-forward window testing
    async fn run_backtest_with_chromosome(
        &self,
        job_id: &str,
        symbol: &str,
        exchange: &str,
        initial_capital: f64,
        initial_inventory_pct: f64,
        commission_rate: f64,
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        data: &[MarketData],
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = initial_capital;
        backtest_config.trading.commission_rate = commission_rate;
        backtest_config.analysis.skip_genetic_optimization = true; // Already done by worker
        backtest_config.analysis.auto_walk_forward = false; // Already done by engine, prevent nested loop
        backtest_config.data.gap_detection.enabled = false; // Disable gap filling - data is already dense (39 orders/min)
        
        // Don't use progress callback for final backtest - it spawns async tasks from spawn_blocking
        // which causes orphaned tasks that prevent job completion
        let engine = BacktestEngine::new(backtest_config);
        
        // Get initial price for inventory calculation
        let asset_price = Self::get_reference_price(data);
        
        // Initialize portfolio with BOTH quote and base assets for proper market making
        let inventory_pct = initial_inventory_pct;
        let quote_balance = initial_capital * (1.0 - inventory_pct);
        let base_value = initial_capital * inventory_pct;
        let base_quantity = if asset_price > 0.0 { base_value / asset_price } else { 0.0 };
        let first_timestamp = Self::get_first_timestamp(data);
        
        let mut portfolio = PortfolioState::with_inventory(
            symbol.to_string(),
            BigDecimal::from_f64(quote_balance).unwrap_or_default(),
            base_quantity,
            BigDecimal::from_f64(asset_price).unwrap_or_default(),
            first_timestamp,
        );
        
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(symbol.to_string(), exchange.to_string(), orderbook_config);
        let mut risk_manager = RiskManager::new();
        
        // Calculate order_size in asset units from percentage of capital
        let order_size_units = (initial_capital * chromosome.order_size_pct) / asset_price;
        
        // Get actual exchange maker fee
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
        let maker_fee = exchange_fee_config.maker_fee;
        
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: order_size_units,
            order_size_pct: chromosome.order_size_pct,
            initial_capital,
            initial_inventory_pct: 0.5,  // 50% of capital as base asset for market making
            window_ms: chromosome.window_ms,
            min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms,
            fee_rate: maker_fee,  // Use actual exchange maker fee
            // latency_buffer removed - latency simulated in order matching
            volatility_cap: chromosome.volatility_cap,
            volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
            time_horizon_minutes: chromosome.time_horizon_minutes,
            market_depth_k: chromosome.market_depth_k,
            volatility_adjustment_factor: chromosome.volatility_adjustment_factor,
            inventory_penalty_factor: chromosome.inventory_penalty_factor,
            trend_sensitivity: chromosome.trend_sensitivity,
            flow_sensitivity: chromosome.flow_sensitivity,
            take_profit_pct: 0.0,  // Disabled - MM uses inventory limits
            stop_loss_pct: 0.0,    // Disabled - MM uses inventory limits
            min_spread_bps: chromosome.min_spread_bps,
            max_trades_per_minute: chromosome.max_trades_per_minute,
            
            // Phase 3: Dynamic k parameters (k divides liquidity term)
            k_volatility_sensitivity: 0.3,
            k_flow_sensitivity: 0.2,
            k_min: 1.0,   // Floor for liquid markets
            k_max: 50.0,  // GA can explore tight spreads
            
            // Phase 4: Dynamic time horizon parameters
            time_horizon_vol_sensitivity: 0.5,
            time_horizon_min: 0.25,
            time_horizon_max: 5.0,
            
            // Phase 4: Dynamic min spread parameters
            min_spread_vol_sensitivity: 0.3,
            min_spread_flow_sensitivity: 0.2,
            min_spread_min_bps: 3.0,
            min_spread_max_bps: 50.0,
            
            // Phase 4: Dynamic order size parameters
            size_vol_sensitivity: 0.3,
            size_drawdown_sensitivity: 0.5,
            size_win_boost: 0.1,
            size_min_pct: 0.005,
            size_max_pct: 0.05,
            
            // Phase 4: Dynamic rate limiting parameters
            rate_vol_sensitivity: 0.4,
            rate_min: 5.0,
            rate_max: 60.0,
            
            // Inventory risk limits
            max_inventory_pct: 0.15,
            inventory_skew_multiplier: 3.0,
            
            // Phase 5: Regime detection parameters
            regime_vol_threshold_high: 1.5,
            regime_vol_threshold_low: 0.7,
            regime_trend_threshold: 0.3,
            regime_high_vol_spread_mult: 1.5,
            regime_low_vol_spread_mult: 0.8,
            regime_trending_inv_mult: 0.5,
            
            // Phase 5: Order book microstructure parameters
            orderbook_imbalance_sensitivity: 0.3,
            orderbook_depth_levels: 5,
            imbalance_skew_factor: 0.5,
            
            // Phase 5: Adverse selection parameters
            informed_trader_prob_window_ms: 5000,
            adverse_selection_multiplier: 2.0,
            toxic_flow_threshold: 0.6,
            
            // Exchange-specific order size limits
            min_order_size: exchange_fee_config.min_order_size,
            max_order_size: exchange_fee_config.max_order_size,
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chromosome.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chromosome.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
        
        let strategy_config = StrategyConfig {
            id: format!("final-{}", job_id),
            name: "Avellaneda-Stoikov".to_string(),
            strategy_type: "AvellanedaStoikov".to_string(),
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            parameters: strategy_params_map,
            risk_limits: StrategyRiskLimits::default(),
            enabled: true,
        };
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        let mut strategy_manager = StrategyManager::new();
        strategy_manager.register_strategy(strategy_config, strategy)
            .map_err(|e| format!("Failed to register strategy: {}", e))?;
        
        // log_info!(...)
        let result = engine.run_backtest(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            data,
        ).await?;
        
        // log_info!(...)
        // log_info!(...)
        
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
        // log_info!(...)
        
        Ok(BacktestResult {
            job_id: job_id.to_string(),
            strategy_type: Some("market_making".to_string()),
            total_pnl,
            sharpe_ratio: result.core.sharpe_ratio.unwrap_or(0.0),
            max_drawdown,
            win_rate: result.core.win_rate.unwrap_or(0.0),
            total_trades: result.core.num_trades,
            profit_factor: result.core.profit_factor.unwrap_or(1.0),
            equity_curve: result.core.equity_curve,
            completed_at: Utc::now(),
            // Trade breakdown from core result
            closed_trades: Some(result.core.closed_trades),
            open_positions: Some(result.core.open_positions),
            realized_pnl: result.core.realized_pnl,
            unrealized_pnl: result.core.unrealized_pnl,
            annualized_return: None,
            volatility: result.core.volatility,
            sortino_ratio: None,
            calmar_ratio: None,
            avg_trade_return: result.core.avg_trade_return,
            best_trade: result.core.gross_profit,
            worst_trade: result.core.gross_loss,
            avg_time_in_trade: result.core.avg_trade_duration,
            value_at_risk_95: None,
            expected_shortfall_95: None,
            max_drawdown_duration_days: None,
            current_drawdown: None,
            avg_drawdown: None,
            total_orders: Some(result.core.num_trades as i32),
            filled_orders: Some(result.core.num_trades as i32),
            cancelled_orders: Some(0),
            avg_slippage: result.core.transaction_costs.as_ref().map(|tc| tc.total_slippage / result.core.num_trades.max(1) as f64),
            total_commission_paid: result.core.transaction_costs.as_ref().map(|tc| tc.total_commission),
            // Comprehensive strategy params from GA-optimized chromosome for live trading
            strategy_params: Some(Self::build_strategy_params_from_chromosome(
                chromosome, order_size_units, exchange, symbol
            )),
            start_date: None, // Chromosome evaluation doesn't have date range
            end_date: None,
            monte_carlo: None, // Chromosome evaluation doesn't run MC
            walk_forward: None, // Chromosome evaluation doesn't run WF
            inventory_metrics: None, // Not tracked for chromosome eval
            execution_metrics: None, // Not tracked for chromosome eval
            risk_halt_reason: None,
            metrics_profile: Some(Self::build_metrics_profile("market_making")),
            is_portfolio: false,
            portfolio_metrics: None,
            routing_metrics: None,
        })
    }
    
    // ============================================================================
    // AUTO DATA PREPARATION
    // ============================================================================
    
    /// Ensure binary data files exist for the given symbol/exchange.
    /// If they don't exist, automatically prepare them from the database.
    /// 
    /// This eliminates the need for the manual `prepare-data` CLI step.
    #[cfg(feature = "postgres")]
    async fn ensure_binary_data(
        &self,
        job: &BacktestJob,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use data_prep::{DataPrepConfig, BinaryFileReader};
        
        let symbol = &job.params.symbol;
        let exchange = &job.params.exchange;
        
        // Get data directory from environment or use default
        let data_dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        let data_path = PathBuf::from(&data_dir);
        
        // Check if binary file already exists
        let filename = format!("{}_{}.bin", symbol.replace('/', "_"), exchange);
        let binary_path = data_path.join(&filename);
        
        if binary_path.exists() {
            // Verify file is valid
            match BinaryFileReader::open(&binary_path) {
                Ok(reader) => {
                    let tick_count = reader.tick_count();
                    if tick_count > 0 {
                        log_info!(MAIN_LOGGER, "Job {} binary data exists: {} ({} ticks)", 
                            job.id, binary_path.display(), tick_count);
                        return Ok(());
                    }
                    log_warn!(MAIN_LOGGER, "Job {} binary file has 0 ticks, will regenerate", job.id);
                }
                Err(e) => {
                    log_warn!(MAIN_LOGGER, "Job {} binary file invalid ({}), will regenerate", job.id, e);
                }
            }
        }
        
        // Binary file doesn't exist or is invalid - prepare it
        log_info!(MAIN_LOGGER, "Job {} preparing binary data for {}/{} -> {}", 
            job.id, symbol, exchange, binary_path.display());
        
        // Update job phase to show we're preparing data
        let _ = self.job_queue.update_phase(&job.id, "preparing_data", serde_json::json!({
            "symbol": symbol,
            "exchange": exchange,
            "output_path": binary_path.display().to_string(),
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 1.0).await;
        
        // Get database pool
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available for data preparation")?;
        
        // Create output directory
        std::fs::create_dir_all(&data_path)?;
        
        // Prepare data configuration
        let config = DataPrepConfig {
            symbol: symbol.clone(),
            exchange: exchange.clone(),
            output_dir: data_path,
            chunk_size: 10_000,
            compute_features: false, // Skip feature computation for speed
            start_time: None,
            end_time: None,
        };
        
        // Run data preparation
        let prep_start = std::time::Instant::now();
        let result = data_prep::prepare_data(&pool, &config).await?;
        let prep_elapsed = prep_start.elapsed();
        
        log_info!(MAIN_LOGGER, "Job {} binary data prepared in {:.1}s: {} ticks, {} MB", 
            job.id, 
            prep_elapsed.as_secs_f64(),
            result.total_ticks,
            result.file_size_bytes / 1_000_000
        );
        
        // Update progress
        let _ = self.job_queue.update_progress(&job.id, 2.0).await;
        
        Ok(())
    }
    
    /// Load market data from Tardis.dev CSV files (gzipped)
    /// 
    /// This loads trades data from pre-downloaded Tardis files.
    /// Files are expected at: {data_path}/{symbol}_trades.csv.gz
    /// 
    /// For L2 orderbook data, use load_tardis_orderbook.
    async fn load_tardis_data(
        &self,
        symbol: &str,
        data_path: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use dataloader::tardis::TardisLoader;
        use bigdecimal::BigDecimal;
        
        // Construct the trades file path: {data_path}/{symbol}_trades.csv.gz
        let trades_path = format!("{}/{}_trades.csv.gz", data_path, symbol);
        
        log_info!(MAIN_LOGGER, "Loading Tardis trades from: {}", trades_path);
        
        // Load trades using TardisLoader
        let trades = TardisLoader::load_trades(&trades_path)
            .map_err(|e| format!("Failed to load Tardis trades: {}", e))?;
        
        log_info!(MAIN_LOGGER, "Loaded {} trades from Tardis file", trades.len());
        
        // Convert TardisTrade to MarketData::Trade(TradeData)
        let market_data: Vec<MarketData> = trades
            .into_iter()
            .map(|t| {
                let side_str = match t.side {
                    dataloader::tick::Side::Buy => "buy".to_string(),
                    dataloader::tick::Side::Sell => "sell".to_string(),
                };
                let price = BigDecimal::try_from(t.price).unwrap_or_default();
                let quantity = BigDecimal::try_from(t.amount).unwrap_or_default();
                
                MarketData::Trade(TradeData::new(
                    dataloader::Tick::timestamp(&t),
                    symbol.to_string(),
                    "binanceus".to_string(), // Tardis data is from Binance.US
                    price,
                    quantity,
                    side_str,
                    t.trade_id.to_string(),
                ))
            })
            .collect();
        
        log_info!(MAIN_LOGGER, "Converted {} Tardis trades to MarketData", market_data.len());
        
        Ok(market_data)
    }
    
    /// Load market data from specific Tardis file paths (from file upload)
    /// 
    /// This loads both trades and orderbook data from uploaded files,
    /// converts orderbook snapshots to events via diffing, and merges them.
    async fn load_tardis_data_from_files(
        &self,
        trades_path: &str,
        orderbook_path: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use dataloader::tardis::TardisLoader;
        use bigdecimal::BigDecimal;
        
        log_info!(MAIN_LOGGER, "Loading Tardis data from uploaded files:");
        log_info!(MAIN_LOGGER, "  Trades: {}", trades_path);
        log_info!(MAIN_LOGGER, "  Orderbook: {}", orderbook_path);
        
        // Load trades
        let trades = TardisLoader::load_trades(trades_path)
            .map_err(|e| format!("Failed to load Tardis trades: {}", e))?;
        log_info!(MAIN_LOGGER, "Loaded {} trades", trades.len());
        
        // Load orderbook snapshots
        let orderbook_snapshots = TardisLoader::load_orderbook(orderbook_path)
            .map_err(|e| format!("Failed to load Tardis orderbook: {}", e))?;
        log_info!(MAIN_LOGGER, "Loaded {} orderbook snapshots", orderbook_snapshots.len());
        
        // Convert to unified order events using the diffing approach
        let order_events = TardisLoader::to_order_events(&orderbook_snapshots, &trades);
        log_info!(MAIN_LOGGER, "Generated {} order events from snapshots + trades", order_events.len());
        
        // Extract symbol from first trade for MarketData
        let symbol = if !trades.is_empty() {
            // Parse from trades path: /path/to/BTCUSD_trades.csv.gz
            let filename = std::path::Path::new(trades_path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("UNKNOWN");
            filename.split('_').next().unwrap_or("UNKNOWN").to_string()
        } else {
            "UNKNOWN".to_string()
        };
        
        // Convert to MarketData - use trades for now as the primary data source
        // The order events from diffing are useful for realistic simulation
        // but the current backtest loop expects MarketData::Trade
        let market_data: Vec<MarketData> = trades
            .into_iter()
            .map(|t| {
                let side_str = match t.side {
                    dataloader::tick::Side::Buy => "buy".to_string(),
                    dataloader::tick::Side::Sell => "sell".to_string(),
                };
                let price = BigDecimal::try_from(t.price).unwrap_or_default();
                let quantity = BigDecimal::try_from(t.amount).unwrap_or_default();
                
                MarketData::Trade(TradeData::new(
                    dataloader::Tick::timestamp(&t),
                    symbol.clone(),
                    "binanceus".to_string(),
                    price,
                    quantity,
                    side_str,
                    t.trade_id.to_string(),
                ))
            })
            .collect();
        
        log_info!(MAIN_LOGGER, "Converted {} trades to MarketData (from {} order events)", 
            market_data.len(), order_events.len());
        
        Ok(market_data)
    }
    
    /// Load market data intelligently based on strategy type
    /// 
    /// This function determines what data to load based on strategy requirements:
    /// - Market making strategies need order flow + trades (tick-level data)
    /// - Momentum strategies need OHLCV candles
    /// - Arbitrage strategies need both real-time data sources
    /// 
    /// Note: Exchange variants like "kraken_colocated" are normalized to the base
    /// exchange name ("kraken") for data lookup, since data is stored under the base name.
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        // Normalize exchange name for data lookup (strip _colocated, _established, etc.)
        let base_exchange = base_exchange_name(exchange);
        
        log_info!(GENETIC_LOGGER, "ENTER load_market_data_for_strategy | type='{}' sym={} ex={} (base={})", 
            strategy_type, symbol, exchange, base_exchange);
        
        // Determine data requirements based on strategy type
        // This maps string types to DataRequirements for intelligent loading
        let needs_order_flow = matches!(
            strategy_type.to_lowercase().as_str(),
            "market_making" | "avellaneda_stoikov" | "avellaneda-stoikov" | 
            "marketmaking" | "mm" | "arbitrage"
        );
        
        if needs_order_flow {
            log_info!(GENETIC_LOGGER, "Strategy '{}' requires order flow data (tick-level)", strategy_type);
            // For market making, load historical orders and trades
            self.load_order_events(symbol, base_exchange).await
        } else {
            log_info!(GENETIC_LOGGER, "Strategy '{}' requires trade/candle data", strategy_type);
            // For momentum/trend strategies, try multiple data sources in order:
            // 1. Candles (aggregated data, best for momentum)
            // 2. Trades table (raw trades from Tardis uploads)
            // 3. Order events (historical_orders, less ideal but works)
            
            // Try candles first
            match self.load_market_data(symbol, base_exchange).await {
                Ok(data) if !data.is_empty() => {
                    log_info!(GENETIC_LOGGER, "Successfully loaded {} candles", data.len());
                    return Ok(data);
                }
                Ok(_) => {
                    log_info!(GENETIC_LOGGER, "No candle data found for {} on {}", symbol, base_exchange);
                }
                Err(e) => {
                    log_info!(GENETIC_LOGGER, "Candle query failed ({})", e);
                }
            }
            
            // Try trades table (where Tardis trades are imported)
            match self.load_trades_data(symbol, base_exchange).await {
                Ok(data) if !data.is_empty() => {
                    log_info!(GENETIC_LOGGER, "Successfully loaded {} trades from trades table", data.len());
                    return Ok(data);
                }
                Ok(_) => {
                    log_info!(GENETIC_LOGGER, "No trades found in trades table for {} on {}", symbol, base_exchange);
                }
                Err(e) => {
                    log_info!(GENETIC_LOGGER, "Trades query failed ({})", e);
                }
            }
            
            // Final fallback: order events
            log_warn!(GENETIC_LOGGER, "Falling back to order events for {} on {}", symbol, base_exchange);
            self.load_order_events(symbol, base_exchange).await
        }
    }
    
    /// Load trades from the trades table (populated by Tardis CSV uploads)
    #[cfg(feature = "postgres")]
    async fn load_trades_data(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::streaming::ParallelDataLoader;
        
        log_info!(MAIN_LOGGER, "Loading trades from trades table for {} on {}", symbol, exchange);
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        const NUM_WORKERS: usize = 4;
        let loader = ParallelDataLoader::new(pool.clone(), NUM_WORKERS);
        
        loader.load_all_trades(symbol, exchange).await
    }
    
    #[cfg(not(feature = "postgres"))]
    async fn load_trades_data(
        &self,
        _symbol: &str,
        _exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        Err("PostgreSQL feature not enabled".into())
    }
    
    /// Load order book events (new, cancel, modify) for market making strategies
    /// Tries historical_orders first (higher fidelity L3-like data from DataEngine or Tardis incremental_book_L2),
    /// then falls back to L2 orderbook snapshots (from Tardis book_snapshot uploads)
    #[cfg(feature = "postgres")]
    async fn load_order_events(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::streaming::ParallelDataLoader;
        
        log_info!(MAIN_LOGGER, "Loading order events for {} on {} (prefer historical_orders, fall back to L2)", symbol, exchange);
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        // First, try to load from historical_orders (L3-like data)
        // This includes both DataEngine collected data and Tardis incremental_book_L2 uploads
        const NUM_WORKERS: usize = 10;
        let loader = ParallelDataLoader::new(pool.clone(), NUM_WORKERS);
        
        match loader.load_all(symbol, exchange).await {
            Ok(data) if !data.is_empty() => {
                log_info!(MAIN_LOGGER, "Loaded {} events from historical_orders for {} on {}", data.len(), symbol, exchange);
                return Ok(data);
            }
            Ok(_) => {
                log_info!(MAIN_LOGGER, "No historical_orders found for {} on {}, trying L2 snapshots", symbol, exchange);
            }
            Err(e) => {
                log_warn!(MAIN_LOGGER, "historical_orders query failed ({}), trying L2 snapshots", e);
            }
        }
        
        // Fall back to L2 orderbook snapshots (from Tardis book_snapshot uploads)
        log_info!(MAIN_LOGGER, "Trying to load L2 orderbook data for {} on {}", symbol, exchange);
        
        match self.load_l2_orderbook_as_market_data(symbol, exchange).await {
            Ok(data) if !data.is_empty() => {
                log_info!(MAIN_LOGGER, "Loaded {} L2 orderbook snapshots for {} on {}", data.len(), symbol, exchange);
                return Ok(data);
            }
            Ok(_) => {
                log_warn!(MAIN_LOGGER, "No L2 data found for {} on {}", symbol, exchange);
            }
            Err(e) => {
                log_warn!(MAIN_LOGGER, "L2 data query failed: {}", e);
            }
        }
        
        // No data found in either table
        log_error!(MAIN_LOGGER, "No market data found for {} on {} in either historical_orders or L2 snapshots", symbol, exchange);
        Ok(vec![])
    }
    
    /// Load L2 orderbook snapshots and convert to MarketData (trade events)
    /// Used for Tardis data uploads where we have L2 snapshots instead of L3 order events
    #[cfg(feature = "postgres")]
    async fn load_l2_orderbook_as_market_data(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use databaseschema::ops::l2_orderbook_snapshot_ops;
        use chrono::{Duration, Utc};
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        // Query L2 snapshots for this symbol/exchange
        // Load last 90 days of data (matching retention policy)
        let end_time = Utc::now();
        let start_time = end_time - Duration::days(90);
        
        let snapshots = l2_orderbook_snapshot_ops::get_l2_snapshots_by_range(
            pool.clone(),
            symbol,
            exchange,
            start_time,
            end_time,
        ).await?;
        
        if snapshots.is_empty() {
            log_info!(MAIN_LOGGER, "No L2 snapshots found for {} on {} in date range", symbol, exchange);
            return Ok(vec![]);
        }
        
        log_info!(MAIN_LOGGER, "Found {} L2 snapshots for {} on {}", snapshots.len(), symbol, exchange);
        
        // Convert L2 snapshots to OrderBookEvents, then wrap in MarketData::Trade
        let events = backtest::l2_snapshots_to_events(&snapshots);
        
        // Convert OrderBookEvents to MarketData::Trade format for compatibility
        let market_data: Vec<MarketData> = events.iter().map(|event| {
            // Create a synthetic TradeData from the OrderBookEvent using the constructor
            let price = BigDecimal::from_f64(event.price).unwrap_or_default();
            let quantity = BigDecimal::from_f64(event.quantity).unwrap_or_default();
            let timestamp = chrono::DateTime::from_timestamp_millis(event.timestamp_ms)
                .unwrap_or_else(|| Utc::now());
            let side = match event.side {
                backtest::Side::Buy => "buy".to_string(),
                backtest::Side::Sell => "sell".to_string(),
            };
            let trade = TradeData::new(
                timestamp,
                symbol.to_string(),
                exchange.to_string(),
                price,
                quantity,
                side,
                event.order_id.clone(),
            );
            MarketData::Trade(trade)
        }).collect();
        
        log_info!(MAIN_LOGGER, "Converted {} L2 snapshots to {} trade events", snapshots.len(), market_data.len());
        Ok(market_data)
    }
    
    #[cfg(not(feature = "postgres"))]
    async fn load_order_events(
        &self,
        _symbol: &str,
        _exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        Err("PostgreSQL feature not enabled".into())
    }
    
    /// Load market data from database (candles from continuous aggregates)
    async fn load_market_data(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "postgres")]
        {
            use diesel::prelude::*;
            use diesel::sql_types::{Timestamptz, Varchar, Numeric, BigInt};
            use diesel_async::RunQueryDsl;
            use dataloader::Candle;
            
            log_info!(GENETIC_LOGGER, "ENTER load_market_data for {} on {}", symbol, exchange);
            
            #[derive(QueryableByName)]
            struct CandleRow {
                #[diesel(sql_type = Timestamptz)]
                timestamp: chrono::NaiveDateTime,
                #[diesel(sql_type = Varchar)]
                symbol: String,
                #[diesel(sql_type = Varchar)]
                exchange: String,
                #[diesel(sql_type = Numeric)]
                open_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                high_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                low_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                close_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                volume: BigDecimal,
                #[diesel(sql_type = BigInt)]
                trade_count: i64,
            }
            
            // Get database pool from job queue
            log_info!(GENETIC_LOGGER, "Getting database pool for {} on {}", symbol, exchange);
            let pool = self.job_queue.get_pool()
                .ok_or_else(|| {
                    log_error!(GENETIC_LOGGER, "Database pool not available for {} on {}", symbol, exchange);
                    "Database pool not available"
                })?;
            
            let symbol = symbol.to_string();
            let exchange = exchange.to_string();
            
            // Get async connection with timeout
            log_info!(GENETIC_LOGGER, "Acquiring database connection for {} on {}", symbol, exchange);
            let mut conn = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                pool.get()
            ).await
                .map_err(|_| {
                    log_error!(GENETIC_LOGGER, "Connection pool timeout loading candles for {} on {}", symbol, exchange);
                    "Connection pool timeout - possible deadlock or pool exhaustion"
                })?
                .map_err(|e| {
                    log_error!(GENETIC_LOGGER, "Failed to get database connection: {}", e);
                    format!("Failed to get database connection: {}", e)
                })?;
            
            // Query candles_1m view for the symbol/exchange
            // Note: For now we load 1-minute candles. Could make timeframe configurable.
            let query = format!(
                "SELECT timestamp, symbol, exchange, open_price, high_price, low_price, \
                 close_price, volume, trade_count FROM candles_1m \
                 WHERE symbol = '{}' AND exchange = '{}' \
                 ORDER BY timestamp ASC LIMIT 10000",
                symbol, exchange
            );
            
            log_info!(GENETIC_LOGGER, "Executing candle query for {} on {}", symbol, exchange);
            
            // Execute query with timeout to prevent hanging on slow queries
            let results: Vec<CandleRow> = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                diesel::sql_query(&query).load(&mut conn)
            ).await
                .map_err(|_| {
                    log_error!(GENETIC_LOGGER, "Query timeout loading candles for {} on {} (30s limit)", symbol, exchange);
                    "Query execution timeout - database may be overloaded"
                })?
                .map_err(|e| {
                    log_error!(GENETIC_LOGGER, "Failed to load candles for {} on {}: {}", symbol, exchange, e);
                    format!("Failed to load candles: {}", e)
                })?;
            
            log_info!(GENETIC_LOGGER, "Loaded {} candle rows for {} on {}", results.len(), symbol, exchange);
            
            let candles: Vec<MarketData> = results
                .into_iter()
                .map(|row| {
                    MarketData::Candle(Candle {
                        timestamp: chrono::DateTime::from_naive_utc_and_offset(row.timestamp, chrono::Utc),
                        symbol: row.symbol,
                        exchange: row.exchange,
                        open: row.open_price,
                        high: row.high_price,
                        low: row.low_price,
                        close: row.close_price,
                        volume: row.volume,
                        trade_count: row.trade_count,
                    })
                })
                .collect();
            
            Ok(candles)
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = (symbol, exchange);
            log_error!(MAIN_LOGGER, "PostgreSQL feature not enabled - cannot load market data");
            Err("PostgreSQL feature not enabled".into())
        }
    }
    
    /// Build comprehensive strategy parameters for live trading deployment
    /// 
    /// This ensures all parameters needed for live trading are saved to the database,
    /// not just the basic 3 params that were being saved before.
    fn build_strategy_params_for_live(params: &crate::job_queue::BacktestJobParams) -> serde_json::Value {
        serde_json::json!({
            // Core AS parameters
            "risk_aversion": params.risk_aversion,
            "inventory_target": params.inventory_target,
            "order_size": params.order_size,
            "order_size_pct": params.order_size_pct,
            "initial_capital": params.initial_capital,
            "initial_inventory_pct": params.initial_inventory_pct,
            
            // Timing parameters
            "window_ms": params.window_ms,
            "min_quote_lifetime_ms": params.min_quote_lifetime_ms,
            "time_horizon_minutes": params.time_horizon_minutes,
            
            // Spread and sizing parameters
            "volatility_cap": params.volatility_cap,
            "market_depth_k": params.market_depth_k,
            "min_spread_bps": params.min_spread_bps,
            "fee_spread_multiplier": params.fee_spread_multiplier,
            
            // Risk control parameters
            "max_trades_per_minute": params.max_trades_per_minute,
            "take_profit_pct": params.take_profit_pct,
            "stop_loss_pct": params.stop_loss_pct,
            
            // Dynamic adjustment parameters
            "volatility_adjustment_factor": params.volatility_adjustment_factor,
            "inventory_penalty_factor": params.inventory_penalty_factor,
            "trend_sensitivity": params.trend_sensitivity,
            "flow_sensitivity": params.flow_sensitivity,
            "adverse_selection_sensitivity": params.adverse_selection_sensitivity,
            
            // Exchange and fee configuration
            "exchange": params.exchange,
            "symbol": params.symbol,
            "fee_rate": params.fee_rate,
            "commission_rate": params.commission_rate,
            
            // Fill model configuration
            "fill_model": params.fill_model,
            
            // Metadata for deployment
            "strategy_type": params.strategy_type,
            "optimization_method": params.optimization_method,
        })
    }
    
    /// Build comprehensive strategy parameters from a GA chromosome
    fn build_strategy_params_from_chromosome(
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome, 
        order_size_units: f64,
        exchange: &str,
        symbol: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            // Core AS parameters (from optimized chromosome)
            "risk_aversion": chromosome.risk_aversion,
            "inventory_target": chromosome.inventory_target,
            "order_size_pct": chromosome.order_size_pct,
            "order_size_units": order_size_units,
            
            // Timing parameters
            "window_ms": chromosome.window_ms,
            "min_quote_lifetime_ms": chromosome.min_quote_lifetime_ms,
            "time_horizon_minutes": chromosome.time_horizon_minutes,
            
            // Spread and sizing parameters
            "volatility_cap": chromosome.volatility_cap,
            "market_depth_k": chromosome.market_depth_k,
            "min_spread_bps": chromosome.min_spread_bps,
            "fee_spread_multiplier": chromosome.fee_spread_multiplier,
            
            // Risk control parameters
            "max_trades_per_minute": chromosome.max_trades_per_minute,
            
            // Dynamic adjustment parameters (optimized)
            "volatility_adjustment_factor": chromosome.volatility_adjustment_factor,
            "inventory_penalty_factor": chromosome.inventory_penalty_factor,
            "trend_sensitivity": chromosome.trend_sensitivity,
            "flow_sensitivity": chromosome.flow_sensitivity,
            "adverse_selection_sensitivity": chromosome.adverse_selection_sensitivity,
            
            // Exchange info
            "exchange": exchange,
            "symbol": symbol,
            
            // Mark as GA-optimized
            "optimization_source": "genetic_algorithm",
        })
    }
    
    /// Build a metrics profile JSON for a given strategy type
    /// 
    /// This determines which metrics are shown in the UI based on strategy type.
    /// Market making shows inventory metrics; momentum shows trend metrics, etc.
    fn build_metrics_profile(strategy_type: &str) -> serde_json::Value {
        let profile = metrics::MetricsProfile::for_strategy(
            metrics::StrategyType::from_str(strategy_type)
        );
        
        serde_json::json!({
            "strategy_type": format!("{:?}", profile.strategy_type),
            "primary_metrics": profile.primary_metrics,
            "secondary_metrics": profile.secondary_metrics,
            "hidden_metrics": profile.hidden_metrics,
            // Include definitions so the frontend knows how to render each metric
            "metric_definitions": profile.metric_definitions.iter().map(|(id, def)| {
                (id.clone(), serde_json::json!({
                    "name": def.name,
                    "description": def.description,
                    "category": format!("{:?}", def.category),
                    "unit": def.unit,
                    "decimals": def.decimals,
                    "higher_is_better": def.higher_is_better,
                    "warning_threshold": def.warning_threshold,
                    "critical_threshold": def.critical_threshold,
                }))
            }).collect::<std::collections::HashMap<_, _>>(),
        })
    }
    
    /// Execute genetic optimization for momentum/trend-following strategy
    async fn execute_momentum_ga_optimization(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use genetic::chromosome::MomentumChromosome;
        use genetic::Chromosome;
        use backtest::momentum_simulation::MomentumSimulator;
        use backtest::realistic_simulation::{OrderBookEvent, EventType, Side};
        use rand::{SeedableRng, Rng};
        
        let params = &job.params;
        
        eprintln!("\n============================================================");
        eprintln!("🚀 [MOMENTUM_GA] STARTING job_id={}", job.id);
        eprintln!("   symbol: {}", params.symbol);
        eprintln!("   exchange: {}", params.exchange);
        eprintln!("   strategy_type: {}", params.strategy_type);
        eprintln!("   population_size: {}", params.population_size);
        eprintln!("   generations: {}", params.generations);
        eprintln!("============================================================\n");
        
        let timing_total_start = std::time::Instant::now();
        
        log_info_structured!(GENETIC_LOGGER, "MOMENTUM_GA_STARTING",
            "event" => "MOMENTUM_GA_STARTING",
            "job_id" => &job.id,
            "population_size" => params.population_size,
            "generations" => params.generations,
            "symbol" => &params.symbol
        );
        
        // Create progress tracker
        let progress_tracker = Arc::new(ProgressTracker::new(job.id.clone(), Arc::clone(&self.job_queue)));
        let _ = progress_tracker.persist_progress(2.0).await;
        
        // Load market data - for momentum we need trades
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
        if data.is_empty() {
            return Err("No market data available".into());
        }
        
        // Convert MarketData to OrderBookEvent for MomentumSimulator
        let events: Vec<OrderBookEvent> = data.iter().filter_map(|md| {
            // Extract trade data from MarketData enum
            match md {
                MarketData::Trade(trade) => {
                    let side = if trade.side == "buy" { Side::Buy } else { Side::Sell };
                    Some(OrderBookEvent {
                        timestamp_ms: trade.timestamp.timestamp_millis(),
                        event_type: EventType::Trade,
                        order_id: trade.trade_id.clone(),
                        side,
                        price: trade.price_f64,
                        quantity: trade.quantity_f64,
                    })
                },
                #[cfg(feature = "postgres")]
                MarketData::Order(order) => {
                    use bigdecimal::ToPrimitive;
                    // Convert order to trade-like event
                    let side = if order.side == "buy" { Side::Buy } else { Side::Sell };
                    let price = order.price_level.to_f64().unwrap_or(0.0);
                    let qty = order.quantity.to_f64().unwrap_or(0.0);
                    Some(OrderBookEvent {
                        timestamp_ms: order.timestamp.timestamp_millis(),
                        event_type: EventType::Trade,
                        order_id: order.order_id.clone(),
                        side,
                        price,
                        quantity: qty,
                    })
                },
                _ => None,
            }
        }).collect();
        
        if events.is_empty() {
            return Err("No trade events found in market data".into());
        }
        
        let events_arc = Arc::new(events);
        let _ = progress_tracker.persist_progress(10.0).await;
        
        // Get exchange fee config
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&params.exchange);
        
        let initial_capital = params.initial_capital;
        let taker_fee = exchange_fee_config.taker_fee;
        let maker_fee = exchange_fee_config.maker_fee;
        
        // GA parameters
        let population_size = params.population_size;
        let generations = params.generations;
        let mutation_rate = 0.15;
        let elite_ratio = 0.1;
        
        // Initialize population
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut population: Vec<MomentumChromosome> = (0..population_size)
            .map(|_| {
                let mut chromo = MomentumChromosome::random(&mut rng);
                chromo.taker_fee_rate = taker_fee;
                chromo.maker_fee_rate = maker_fee;
                chromo
            })
            .collect();
        
        let simulator = MomentumSimulator::new(initial_capital);
        let mut best_fitness = f64::MIN;
        let mut best_chromo: Option<MomentumChromosome> = None;
        let mut best_result: Option<backtest::momentum_simulation::MomentumSimResult> = None;
        
        log_info!(GENETIC_LOGGER, "Momentum GA starting {} generations with {} population", 
            generations, population_size);
        
        for gen in 0..generations {
            let gen_start = std::time::Instant::now();
            
            // Evaluate population
            let mut fitness_scores: Vec<(usize, f64, backtest::momentum_simulation::MomentumSimResult)> = population
                .iter()
                .enumerate()
                .map(|(i, chromo)| {
                    let result = simulator.run_chromosome(&events_arc, chromo);
                    
                    // === HARD REJECTION CRITERIA ===
                    // These result in fitness = 0 (will be eliminated)
                    
                    // Hard reject: Losing strategies (must be profitable)
                    if result.net_pnl <= 0.0 {
                        return (i, 0.0, result);
                    }
                    
                    // Hard reject: Too few trades (not statistically significant)
                    if result.total_trades < 10 {
                        return (i, 0.0, result);
                    }
                    
                    // Hard reject: Catastrophic drawdown (too risky)
                    if result.max_drawdown_pct > 0.25 {
                        return (i, 0.0, result);
                    }
                    
                    // Hard reject: Negative Sharpe (risk-adjusted loss)
                    if result.sharpe_ratio < 0.0 {
                        return (i, 0.0, result);
                    }
                    
                    // === SOFT SCORING (only for profitable strategies) ===
                    let mut fitness = 0.0;
                    
                    // Base metrics (weighted)
                    fitness += result.sharpe_ratio * 0.20;
                    fitness += result.sortino_ratio * 0.15;
                    fitness += result.profit_factor.min(3.0) * 0.15;
                    fitness += result.win_rate * 0.10;
                    fitness += result.net_pnl_pct * 10.0 * 0.25; // Scale PnL %
                    fitness += result.calmar_ratio.min(5.0) * 0.15;
                    
                    // Soft penalties for suboptimal but acceptable strategies
                    if result.total_trades < 30 {
                        fitness *= 0.8; // Prefer more trades for statistical confidence
                    }
                    if result.max_drawdown_pct > 0.15 {
                        fitness *= 0.85; // Prefer lower drawdown
                    }
                    if result.win_rate < 0.4 {
                        fitness *= 0.9; // Prefer higher win rate
                    }
                    
                    (i, fitness, result)
                })
                .collect();
            
            // Sort by fitness (descending)
            fitness_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            
            let gen_best_fitness = fitness_scores[0].1;
            if gen_best_fitness > best_fitness {
                best_fitness = gen_best_fitness;
                best_chromo = Some(population[fitness_scores[0].0].clone());
                best_result = Some(fitness_scores[0].2.clone());
            }
            
            let gen_time = gen_start.elapsed();
            let avg_fitness = fitness_scores.iter().map(|(_, f, _)| f).sum::<f64>() / population.len() as f64;
            
            // Update progress
            let progress = 10.0 + (gen as f64 / generations as f64) * 80.0;
            let _ = progress_tracker.persist_progress(progress).await;
            
            progress_tracker.broadcast_phase("ga_evolution", serde_json::json!({
                "generation": gen + 1,
                "total_generations": generations,
                "best_fitness": gen_best_fitness,
                "avg_fitness": avg_fitness,
                "elapsed_ms": gen_time.as_millis(),
            }));
            
            log_info!(GENETIC_LOGGER, "Gen {}/{}: best={:.4}, avg={:.4} ({:.1}s)",
                gen + 1, generations, gen_best_fitness, avg_fitness, gen_time.as_secs_f64());
            
            // Selection + Crossover + Mutation
            let elite_count = (population.len() as f64 * elite_ratio).ceil() as usize;
            let mut new_population: Vec<MomentumChromosome> = Vec::with_capacity(population.len());
            
            // Keep elites
            for &(idx, _, _) in fitness_scores.iter().take(elite_count) {
                new_population.push(population[idx].clone());
            }
            
            // Tournament selection + crossover for rest
            let selection_pool_size = (population.len() / 2).max(1);
            while new_population.len() < population.len() {
                let parent1_idx = fitness_scores[rng.gen_range(0..selection_pool_size)].0;
                let parent2_idx = fitness_scores[rng.gen_range(0..selection_pool_size)].0;
                
                let mut child = population[parent1_idx].crossover(&population[parent2_idx], &mut rng);
                child.mutate(&mut rng, mutation_rate);
                child.taker_fee_rate = taker_fee;
                child.maker_fee_rate = maker_fee;
                new_population.push(child);
            }
            
            population = new_population;
        }
        
        let _ = progress_tracker.persist_progress(95.0).await;
        
        // Build result from best chromosome
        let best = best_chromo.ok_or("No best chromosome found")?;
        let result = best_result.ok_or("No best result found")?;
        
        let total_elapsed = timing_total_start.elapsed();
        
        log_info_structured!(GENETIC_LOGGER, "MOMENTUM_GA_COMPLETE",
            "event" => "MOMENTUM_GA_COMPLETE",
            "job_id" => &job.id,
            "best_fitness" => format!("{:.4}", best_fitness),
            "net_pnl" => format!("{:.2}", result.net_pnl),
            "sharpe" => format!("{:.4}", result.sharpe_ratio),
            "total_trades" => result.total_trades,
            "elapsed_sec" => format!("{:.2}", total_elapsed.as_secs_f64())
        );
        
        let _ = progress_tracker.persist_progress(100.0).await;
        
        // Build BacktestResult with momentum metrics
        Ok(BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some("momentum".to_string()),
            total_pnl: result.net_pnl,
            total_trades: result.total_trades as usize,
            sharpe_ratio: result.sharpe_ratio,
            max_drawdown: result.max_drawdown_pct,
            win_rate: result.win_rate,
            profit_factor: result.profit_factor,
            sortino_ratio: Some(result.sortino_ratio),
            calmar_ratio: Some(result.calmar_ratio),
            avg_trade_return: Some(result.avg_trade_pnl / initial_capital * 100.0),
            // Handle edge case where no trades occurred (default MIN/MAX values)
            best_trade: if result.best_trade_pnl > -1e100 { Some(result.best_trade_pnl) } else { None },
            worst_trade: if result.worst_trade_pnl < 1e100 { Some(result.worst_trade_pnl) } else { None },
            total_commission_paid: Some(result.total_fees),
            // Momentum-specific best parameters
            strategy_params: Some(serde_json::json!({
                "strategy": "momentum",
                "momentum_threshold_pct": best.momentum_threshold_pct,
                "momentum_lookback_ms": best.momentum_lookback_ms,
                "volume_spike_threshold": best.volume_spike_threshold,
                "imbalance_threshold": best.imbalance_threshold,
                "take_profit_pct": best.take_profit_pct,
                "stop_loss_pct": best.stop_loss_pct,
                "trailing_stop_enabled": best.trailing_stop_enabled,
                "trailing_stop_pct": best.trailing_stop_pct,
                "position_size_pct": best.position_size_pct,
                "max_position_pct": best.max_position_pct,
                "signal_stats": {
                    "total_long_signals": result.total_long_signals,
                    "total_short_signals": result.total_short_signals,
                    "signals_filtered_by_volatility": result.signals_filtered_by_volatility,
                    "signals_filtered_by_regime": result.signals_filtered_by_regime,
                    "winning_trades": result.winning_trades,
                    "losing_trades": result.losing_trades,
                },
                "fitness": best_fitness,
            })),
            metrics_profile: Some(serde_json::json!({
                "strategy_type": "momentum",
                "primary_metrics": ["total_pnl", "sharpe_ratio", "profit_factor", "win_rate"],
                "secondary_metrics": ["sortino_ratio", "max_drawdown", "total_trades", "calmar_ratio"],
            })),
            ..Default::default()
        })
    }
    
    /// Execute genetic optimization for momentum strategy with MULTIPLE symbol/exchange pairs
    /// Runs independent GA optimization on each asset, then aggregates portfolio results
    async fn execute_momentum_portfolio_ga_optimization(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use genetic::chromosome::MomentumChromosome;
        use genetic::Chromosome;
        use backtest::momentum_simulation::MomentumSimulator;
        use backtest::realistic_simulation::{OrderBookEvent, EventType, Side};
        use rand::{SeedableRng, Rng};
        
        let params = &job.params;
        
        // Get all assets from portfolio_assets
        let assets = job.params.get_portfolio_assets();
        if assets.is_empty() {
            return Err("No portfolio assets defined".into());
        }
        
        eprintln!("\n============================================================");
        eprintln!("🚀 [MOMENTUM_PORTFOLIO_GA] STARTING job_id={}", job.id);
        eprintln!("   assets: {:?}", assets.iter().map(|a| format!("{}:{}", a.symbol, a.exchange)).collect::<Vec<_>>());
        eprintln!("   population_size: {}", params.population_size);
        eprintln!("   generations: {}", params.generations);
        eprintln!("============================================================\n");
        
        let timing_total_start = std::time::Instant::now();
        
        log_info_structured!(GENETIC_LOGGER, "MOMENTUM_PORTFOLIO_GA_STARTING",
            "event" => "MOMENTUM_PORTFOLIO_GA_STARTING",
            "job_id" => &job.id,
            "num_assets" => assets.len(),
            "population_size" => params.population_size,
            "generations" => params.generations
        );
        
        let progress_tracker = Arc::new(ProgressTracker::new(job.id.clone(), Arc::clone(&self.job_queue)));
        let _ = progress_tracker.persist_progress(2.0).await;
        
        // Track results per asset
        let mut asset_results: Vec<(String, String, backtest::momentum_simulation::MomentumSimResult, MomentumChromosome, f64)> = Vec::new();
        let num_assets = assets.len();
        
        for (asset_idx, asset) in assets.iter().enumerate() {
            let symbol = &asset.symbol;
            let exchange = &asset.exchange;
            
            eprintln!("  📊 Processing asset {}/{}: {} on {}", asset_idx + 1, num_assets, symbol, exchange);
            
            // Load data for this asset
            let data = match self.load_market_data_for_strategy(symbol, exchange, &params.strategy_type).await {
                Ok(d) if !d.is_empty() => d,
                Ok(_) => {
                    log_warn!(GENETIC_LOGGER, "No data for {} on {}, skipping", symbol, exchange);
                    continue;
                }
                Err(e) => {
                    log_warn!(GENETIC_LOGGER, "Failed to load data for {} on {}: {}", symbol, exchange, e);
                    continue;
                }
            };
            
            // Convert to events
            let events: Vec<OrderBookEvent> = data.iter().filter_map(|md| {
                match md {
                    MarketData::Trade(trade) => {
                        let side = if trade.side == "buy" { Side::Buy } else { Side::Sell };
                        Some(OrderBookEvent {
                            timestamp_ms: trade.timestamp.timestamp_millis(),
                            event_type: EventType::Trade,
                            order_id: trade.trade_id.clone(),
                            side,
                            price: trade.price_f64,
                            quantity: trade.quantity_f64,
                        })
                    },
                    #[cfg(feature = "postgres")]
                    MarketData::Order(order) => {
                        use bigdecimal::ToPrimitive;
                        let side = if order.side == "buy" { Side::Buy } else { Side::Sell };
                        Some(OrderBookEvent {
                            timestamp_ms: order.timestamp.timestamp_millis(),
                            event_type: EventType::Trade,
                            order_id: order.order_id.clone(),
                            side,
                            price: order.price_level.to_f64().unwrap_or(0.0),
                            quantity: order.quantity.to_f64().unwrap_or(0.0),
                        })
                    },
                    _ => None,
                }
            }).collect();
            
            if events.is_empty() {
                log_warn!(GENETIC_LOGGER, "No trade events for {} on {}, skipping", symbol, exchange);
                continue;
            }
            
            let events_arc = Arc::new(events);
            
            // GA for this asset
            let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
            let initial_capital = params.initial_capital / num_assets as f64; // Split capital
            let taker_fee = exchange_fee_config.taker_fee;
            let maker_fee = exchange_fee_config.maker_fee;
            
            let population_size = params.population_size;
            let generations = params.generations;
            let mutation_rate = 0.15;
            let elite_ratio = 0.1;
            
            let mut rng = rand::rngs::StdRng::seed_from_u64(42 + asset_idx as u64);
            let mut population: Vec<MomentumChromosome> = (0..population_size)
                .map(|_| {
                    let mut chromo = MomentumChromosome::random(&mut rng);
                    chromo.taker_fee_rate = taker_fee;
                    chromo.maker_fee_rate = maker_fee;
                    chromo
                })
                .collect();
            
            let simulator = MomentumSimulator::new(initial_capital);
            let mut best_fitness = f64::MIN;
            let mut best_chromo: Option<MomentumChromosome> = None;
            let mut best_result: Option<backtest::momentum_simulation::MomentumSimResult> = None;
            
            for gen in 0..generations {
                let mut fitness_scores: Vec<(usize, f64, backtest::momentum_simulation::MomentumSimResult)> = population
                    .iter()
                    .enumerate()
                    .map(|(i, chromo)| {
                        let result = simulator.run_chromosome(&events_arc, chromo);
                        
                        // Hard rejection criteria
                        if result.net_pnl <= 0.0 || result.total_trades < 10 || 
                           result.max_drawdown_pct > 0.25 || result.sharpe_ratio < 0.0 {
                            return (i, 0.0, result);
                        }
                        
                        // Soft scoring
                        let mut fitness = 0.0;
                        fitness += result.sharpe_ratio * 0.20;
                        fitness += result.sortino_ratio * 0.15;
                        fitness += result.profit_factor.min(3.0) * 0.15;
                        fitness += result.win_rate * 0.10;
                        fitness += result.net_pnl_pct * 10.0 * 0.25;
                        fitness += result.calmar_ratio.min(5.0) * 0.15;
                        
                        if result.total_trades < 30 { fitness *= 0.8; }
                        if result.max_drawdown_pct > 0.15 { fitness *= 0.85; }
                        if result.win_rate < 0.4 { fitness *= 0.9; }
                        
                        (i, fitness, result)
                    })
                    .collect();
                
                fitness_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                
                let gen_best = fitness_scores[0].1;
                if gen_best > best_fitness {
                    best_fitness = gen_best;
                    best_chromo = Some(population[fitness_scores[0].0].clone());
                    best_result = Some(fitness_scores[0].2.clone());
                }
                
                // Selection + Crossover + Mutation
                let elite_count = (population.len() as f64 * elite_ratio).ceil() as usize;
                let mut new_population: Vec<MomentumChromosome> = Vec::with_capacity(population.len());
                
                for &(idx, _, _) in fitness_scores.iter().take(elite_count) {
                    new_population.push(population[idx].clone());
                }
                
                let selection_pool_size = (population.len() / 2).max(1);
                while new_population.len() < population.len() {
                    let p1 = fitness_scores[rng.gen_range(0..selection_pool_size)].0;
                    let p2 = fitness_scores[rng.gen_range(0..selection_pool_size)].0;
                    let mut child = population[p1].crossover(&population[p2], &mut rng);
                    child.mutate(&mut rng, mutation_rate);
                    child.taker_fee_rate = taker_fee;
                    child.maker_fee_rate = maker_fee;
                    new_population.push(child);
                }
                
                population = new_population;
            }
            
            // Store best result for this asset
            if let (Some(chromo), Some(result)) = (best_chromo, best_result) {
                eprintln!("    ✅ {} on {}: PnL=${:.2}, Sharpe={:.3}, Trades={}", 
                    symbol, exchange, result.net_pnl, result.sharpe_ratio, result.total_trades);
                asset_results.push((symbol.clone(), exchange.clone(), result, chromo, best_fitness));
            }
            
            // Update progress
            let progress = 10.0 + ((asset_idx + 1) as f64 / num_assets as f64) * 85.0;
            let _ = progress_tracker.persist_progress(progress).await;
        }
        
        if asset_results.is_empty() {
            return Err("No profitable strategies found for any asset".into());
        }
        
        // Aggregate portfolio results
        let total_pnl: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.net_pnl).sum();
        let total_trades: usize = asset_results.iter().map(|(_, _, r, _, _)| r.total_trades as usize).sum();
        let avg_sharpe: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.sharpe_ratio).sum::<f64>() / asset_results.len() as f64;
        let avg_win_rate: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.win_rate).sum::<f64>() / asset_results.len() as f64;
        let max_dd: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.max_drawdown_pct).fold(0.0, f64::max);
        let avg_profit_factor: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.profit_factor).sum::<f64>() / asset_results.len() as f64;
        let total_fees: f64 = asset_results.iter().map(|(_, _, r, _, _)| r.total_fees).sum();
        
        let total_elapsed = timing_total_start.elapsed();
        
        log_info_structured!(GENETIC_LOGGER, "MOMENTUM_PORTFOLIO_GA_COMPLETE",
            "event" => "MOMENTUM_PORTFOLIO_GA_COMPLETE",
            "job_id" => &job.id,
            "num_assets" => asset_results.len(),
            "total_pnl" => format!("{:.2}", total_pnl),
            "avg_sharpe" => format!("{:.4}", avg_sharpe),
            "total_trades" => total_trades,
            "elapsed_sec" => format!("{:.2}", total_elapsed.as_secs_f64())
        );
        
        let _ = progress_tracker.persist_progress(100.0).await;
        
        // Build per-symbol results
        let symbol_results: Vec<serde_json::Value> = asset_results.iter().map(|(sym, ex, res, chromo, fitness)| {
            serde_json::json!({
                "symbol": sym,
                "exchange": ex,
                "net_pnl": res.net_pnl,
                "sharpe_ratio": res.sharpe_ratio,
                "sortino_ratio": res.sortino_ratio,
                "profit_factor": res.profit_factor,
                "win_rate": res.win_rate,
                "total_trades": res.total_trades,
                "max_drawdown_pct": res.max_drawdown_pct,
                "fitness": fitness,
                "best_params": {
                    "momentum_threshold_pct": chromo.momentum_threshold_pct,
                    "momentum_lookback_ms": chromo.momentum_lookback_ms,
                    "take_profit_pct": chromo.take_profit_pct,
                    "stop_loss_pct": chromo.stop_loss_pct,
                    "position_size_pct": chromo.position_size_pct,
                }
            })
        }).collect();
        
        Ok(BacktestResult {
            job_id: job.id.clone(),
            strategy_type: Some("momentum_portfolio".to_string()),
            total_pnl,
            total_trades,
            sharpe_ratio: avg_sharpe,
            max_drawdown: max_dd,
            win_rate: avg_win_rate,
            profit_factor: avg_profit_factor,
            sortino_ratio: Some(asset_results.iter().map(|(_, _, r, _, _)| r.sortino_ratio).sum::<f64>() / asset_results.len() as f64),
            calmar_ratio: Some(asset_results.iter().map(|(_, _, r, _, _)| r.calmar_ratio).sum::<f64>() / asset_results.len() as f64),
            total_commission_paid: Some(total_fees),
            portfolio_metrics: Some(crate::job_queue::PortfolioMetrics {
                total_symbols: asset_results.len(),
                symbol_results: asset_results.iter().map(|(sym, ex, res, _, _)| {
                    crate::job_queue::SymbolResult {
                        symbol: sym.clone(),
                        exchange: Some(ex.clone()),
                        total_pnl: res.net_pnl,
                        total_trades: res.total_trades as usize,
                        sharpe_ratio: res.sharpe_ratio,
                        win_rate: res.win_rate,
                        max_drawdown: res.max_drawdown_pct,
                        profit_factor: res.profit_factor,
                        allocation_weight: 1.0 / asset_results.len() as f64, // Equal weight for now
                        starting_capital: None,
                        ending_capital: None,
                        starting_inventory: None,
                        ending_inventory: None,
                        starting_inventory_value: None,
                        ending_inventory_value: None,
                        starting_mark_price: None,
                        ending_mark_price: None,
                    }
                }).collect(),
                correlation_matrix: vec![],
                portfolio_sharpe: avg_sharpe,
                portfolio_max_drawdown: max_dd,
                diversification_ratio: Some(1.0),
            }),
            strategy_params: Some(serde_json::json!({
                "strategy": "momentum_portfolio",
                "num_assets": asset_results.len(),
                "symbol_results": symbol_results,
            })),
            metrics_profile: Some(serde_json::json!({
                "strategy_type": "momentum_portfolio",
                "primary_metrics": ["total_pnl", "sharpe_ratio", "profit_factor"],
                "secondary_metrics": ["sortino_ratio", "max_drawdown", "total_trades"],
            })),
            ..Default::default()
        })
    }
}

/// Spawn multiple workers
pub fn spawn_workers(num_workers: usize, job_queue: Arc<dyn JobQueue>) {
    for i in 0..num_workers {
        let job_queue_clone = Arc::clone(&job_queue);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_e| {
                    panic!("Cannot create worker runtime");
                });
            
            runtime.block_on(async {
                let worker = Worker::new(i, job_queue_clone);
                worker.run().await;
            });
        });
    }
    
    // log_info!(...)
}


