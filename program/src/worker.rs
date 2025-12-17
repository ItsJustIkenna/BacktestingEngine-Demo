//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
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
use dataloader::MarketData;

// ============================================================================
// COMPREHENSIVE FITNESS FUNCTION
// ============================================================================

/// Calculate comprehensive fitness score for genetic optimization
/// 
/// This function evaluates strategies holistically to find genuinely profitable ones.
/// It penalizes strategies that look good on paper but fail after transaction costs.
/// 
/// # Weights (sum to 1.0)
/// - Sortino Ratio: 0.25 (risk-adjusted returns, downside only)
/// - Sharpe Ratio: 0.15 (risk-adjusted returns, total volatility)
/// - Profit Factor: 0.20 (gross profit / gross loss)
/// - Win Rate: 0.15 (consistency of winning)
/// - Net Profit %: 0.15 (absolute profitability)
/// - Calmar Ratio: 0.10 (recovery ability)
/// 
/// # Penalties
/// - Commission ratio > 30%: scaled penalty up to 0.1x (stricter than before)
/// - Drawdown > 10%: 0.8x, > 20%: 0.5x, > 35%: 0.2x
/// - < 30 trades: linear penalty
/// - Avg trade < 4x commission: 0.5x (raised from 3x)
/// - VaR > 5% of capital: penalty
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
    
    fitness.max(0.0)
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
        if let Err(e) = self.job_queue.update_phase(&job.id, "initializing", serde_json::json!({
            "optimization_method": job.params.optimization_method,
            "symbol": job.params.symbol,
            "exchange": job.params.exchange,
        })).await {
            log_error!(MAIN_LOGGER, "Job {} failed to set initial phase: {}", job.id, e);
        }
        
        let result = match job.params.optimization_method.as_str() {
            "genetic" => {
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
        let _ = self.job_queue.update_phase(&job.id, "loading_data", serde_json::json!({
            "symbol": params.symbol,
            "exchange": params.exchange,
        })).await;
        let _ = self.job_queue.update_progress(&job.id, 5.0).await;
        
        // Load market data intelligently based on strategy type
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
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
            strategy_params: Some(serde_json::json!({
                "risk_aversion": job.params.risk_aversion,
                "inventory_target": job.params.inventory_target,
                "order_size": job.params.order_size
            })),
            // Date range for equity curve x-axis
            start_date: job.params.start_date.clone(),
            end_date: job.params.end_date.clone(),
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
    
    /// Execute genetic optimization to find best strategy parameters
    async fn execute_genetic_optimization(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use genetic::GeneticOptimizer;
        use genetic::chromosome::AvellanedaStoikovChromosome;
        use config::GeneticConfig;
        
        let params = &job.params;
        
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
        
        // Use FULL dataset for all phases - no sampling
        // This ensures accurate fill simulation for market making strategies
        // OPTIMIZATION: Wrap in Arc ONCE to avoid cloning 6.8M elements multiple times
        // Previously we cloned data twice which used ~3x memory and was very slow
        let data_len = data.len();
        let shared_data = Arc::new(data);
        
        log_info_structured!(GENETIC_LOGGER, "GA_DATA_SPLIT",
            "event" => "GA_DATA_SPLIT",
            "job_id" => &job.id,
            "ga_data_points" => data_len,
            "backtest_data_points" => data_len,
            "walkforward_data_points" => data_len
        );
        eprintln!("📊 Data wrapped in Arc (no cloning) - {} points shared across all phases", data_len);
        
        // === FAST PATH: Pre-compute vectorized market features ONCE for entire dataset ===
        // This is the key optimization: calculate EWMA volatility, log returns, etc. once
        // instead of recalculating per-event during each chromosome evaluation.
        // 
        // With 6.9M events and 100 chromosomes per generation:
        // - Old: 6.9M * 100 = 690M async awaits per generation
        // - New: Pre-compute once, then pure synchronous vectorized evaluation
        //
        // Speedup: ~50-100x for GA fitness evaluations
        let fast_sim_start = std::time::Instant::now();
        eprintln!("📊 Starting MarketFeatures computation...");
        let market_features = Arc::new(backtest::fast_simulation::MarketFeatures::from_market_data(&shared_data));
        eprintln!("📊 MarketFeatures computed in {:.2}s", fast_sim_start.elapsed().as_secs_f64());
        log_info!(GENETIC_LOGGER, "Pre-computed market features in {:.2}ms | {} data points | {} log returns | {} volatility values",
            fast_sim_start.elapsed().as_millis(),
            market_features.len(),
            market_features.log_returns.len(),
            market_features.volatility.len());
        
        // Check if fast simulation mode is enabled (default: true for GA)
        let use_fast_simulation = std::env::var("GA_FAST_SIMULATION")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        
        eprintln!("📊 Fast simulation enabled: {}", use_fast_simulation);
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
        
        // All phases share the same Arc - no cloning needed
        let data_arc = Arc::clone(&shared_data);
        let data_for_backtest = Arc::clone(&shared_data);
        let data_for_walkforward = Arc::clone(&shared_data);
        let features_arc = Arc::clone(&market_features);
        // Create separate clones for distributed mode fallback (local GA uses these too)
        let data_for_distributed = Arc::clone(&shared_data);
        let symbol = params.symbol.clone();
        let exchange = params.exchange.clone();
        let symbol_for_distributed = params.symbol.clone();
        let exchange_for_distributed = params.exchange.clone();
        let exchange_for_logging = exchange.clone(); // Clone for use after the closure
        let initial_capital = params.initial_capital;
        let initial_capital_for_distributed = params.initial_capital;
        let initial_inventory_pct = params.initial_inventory_pct;
        let initial_inventory_pct_for_distributed = params.initial_inventory_pct;
        let commission_rate = params.commission_rate;
        let commission_rate_for_distributed = params.commission_rate;
        
        // Create fitness function that evaluates chromosomes and returns equity curve for robustness analysis
        // THREAD SAFETY: We clone Arc references and create fresh state inside each async block
        // Use static counter to track fitness evaluation progress
        static FITNESS_EVAL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        
        // === FITNESS FUNCTION ===
        // Two modes:
        // 1. FAST MODE (default): Vectorized simulation using pre-computed features
        //    - ~50-100x faster than event-by-event simulation
        //    - No async overhead, pure CPU-bound computation
        //    - Suitable for exploring parameter space during GA
        // 2. DETAILED MODE: Full event-by-event simulation with async strategy processing
        //    - Used for final validation of best chromosome
        //    - More accurate but much slower
        
        let fitness_fn: genetic::AsyncFitnessFn<AvellanedaStoikovChromosome> = Arc::new(move |chromosome: &AvellanedaStoikovChromosome| {
            let features = Arc::clone(&features_arc);
            let data = Arc::clone(&data_arc);
            let symbol = symbol.clone();
            let exchange = exchange.clone();
            let chr = chromosome.clone();
            
            Box::pin(async move {
                let eval_id = FITNESS_EVAL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let eval_start = std::time::Instant::now();
                
                // === FAST PATH: Vectorized simulation (default for GA) ===
                // This is synchronous - no .await overhead for millions of data points
                if use_fast_simulation {
                    // Convert chromosome to fast simulation parameters
                    let fast_params = backtest::fast_simulation::FastASParams {
                        risk_aversion: chr.risk_aversion,
                        inventory_target: chr.inventory_target,
                        order_size_pct: chr.order_size_pct,
                        volatility_cap: chr.volatility_cap,
                        min_spread_bps: chr.min_spread_bps,
                        time_horizon_minutes: chr.time_horizon_minutes,
                        fee_rate: commission_rate,
                    };
                    
                    // Run vectorized simulation (synchronous, no async overhead!)
                    let simulator = backtest::fast_simulation::FastSimulator::new(&features, initial_capital);
                    let fast_result = simulator.evaluate(&fast_params);
                    
                    // Convert to standard BacktestResult and calculate fitness
                    let backtest_result: backtest::types::BacktestResult = fast_result.clone().into();
                    let fitness = calculate_comprehensive_fitness(&backtest_result, initial_capital);
                    
                    log_info_structured!(GENETIC_LOGGER, "FAST_FITNESS_EVAL",
                        "event" => "FAST_FITNESS_EVAL",
                        "eval_id" => eval_id,
                        "fitness" => format!("{:.6}", fitness),
                        "pnl" => format!("{:.2}", fast_result.total_pnl),
                        "sharpe" => format!("{:.3}", fast_result.sharpe_ratio),
                        "trades" => fast_result.num_trades,
                        "elapsed_ms" => eval_start.elapsed().as_millis()
                    );
                    
                    return genetic::FitnessResult::new(fitness, fast_result.equity_curve);
                }
                
                // === DETAILED PATH: Full event-by-event simulation (legacy/validation) ===
                // GA FITNESS SAMPLING: Adaptive sampling based on dataset size
                // Target ~500K events per evaluation for consistent ~10-15 second evaluations
                // ENV: GA_FITNESS_SAMPLE_RATE overrides adaptive calculation
                const TARGET_EVENTS_PER_EVAL: usize = 500_000;
                
                let base_sample_rate: usize = std::env::var("GA_FITNESS_SAMPLE_RATE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0); // 0 means use adaptive
                
                let sample_rate = if base_sample_rate > 0 {
                    base_sample_rate
                } else {
                    // Adaptive: calculate sample rate to hit ~500K events
                    // 6.9M / 500K = 14, 2M / 500K = 4, 500K / 500K = 1
                    (data.len() / TARGET_EVENTS_PER_EVAL).max(1)
                };
                
                let sampled_data: Vec<dataloader::MarketData> = if sample_rate > 1 {
                    data.iter().step_by(sample_rate).cloned().collect()
                } else {
                    data.to_vec()
                };
                
                log_info_structured!(GENETIC_LOGGER, "FITNESS_EVAL_ENTER",
                    "event" => "FITNESS_EVAL_ENTER",
                    "eval_id" => eval_id,
                    "original_data_len" => data.len(),
                    "sampled_data_len" => sampled_data.len(),
                    "sample_rate" => sample_rate,
                    "adaptive" => base_sample_rate == 0,
                    "risk_aversion" => format!("{:.4}", chr.risk_aversion)
                );
                let eval_start = std::time::Instant::now();
                
                // Each async block gets its own fresh state - no shared mutable state
                let mut backtest_config = BacktestConfig::default();
                backtest_config.trading.initial_capital = initial_capital;
                backtest_config.trading.commission_rate = commission_rate;
                backtest_config.analysis.skip_genetic_optimization = true;
                backtest_config.analysis.auto_monte_carlo = false;
                backtest_config.analysis.auto_walk_forward = false;
                backtest_config.data.gap_detection.enabled = false;
                
                let engine = BacktestEngine::new(backtest_config);
                
                // Get initial price for inventory calculation
                let asset_price = Self::get_reference_price(&data);
                
                // Initialize portfolio with BOTH quote and base assets for proper market making
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
                
                // Calculate order_size in asset units from percentage of capital
                // order_size = (capital * order_size_pct) / asset_price
                let asset_price = Self::get_reference_price(&data);
                let order_size_units = (initial_capital * chr.order_size_pct) / asset_price;
                let order_size_usd = order_size_units * asset_price;
                
                log_debug!(GENETIC_LOGGER, "Order size calc | capital=${:.0} order_size_pct={:.2}% asset_price=${:.2} => order_size={:.6} units = ${:.2}",
                    initial_capital, chr.order_size_pct * 100.0, asset_price, order_size_units, order_size_usd);
                
                // Get actual exchange maker fee instead of chromosome fee_rate
                let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&exchange);
                let maker_fee = exchange_fee_config.maker_fee;
                
                // Create strategy with ALL chromosome parameters (fixing incomplete mapping)
                let strategy_params = AvellanedaStoikovParams {
                    risk_aversion: chr.risk_aversion,
                    inventory_target: chr.inventory_target,
                    order_size: order_size_units,
                    order_size_pct: chr.order_size_pct,
                    initial_capital,
                    initial_inventory_pct: 0.5,  // 50% of capital as base asset for market making
                    window_ms: chr.window_ms,
                    min_quote_lifetime_ms: chr.min_quote_lifetime_ms,
                    fee_rate: maker_fee,  // Use actual exchange maker fee, not chromosome
                    // latency_buffer removed - latency simulated in order matching
                    volatility_cap: chr.volatility_cap,
                    volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
                    time_horizon_minutes: chr.time_horizon_minutes,
                    market_depth_k: chr.market_depth_k,
                    min_spread_bps: chr.min_spread_bps,
                    max_trades_per_minute: chr.max_trades_per_minute,
                    take_profit_pct: chr.take_profit_pct,
                    stop_loss_pct: chr.stop_loss_pct,
                    volatility_adjustment_factor: chr.volatility_adjustment_factor,
                    inventory_penalty_factor: chr.inventory_penalty_factor,
                    trend_sensitivity: chr.trend_sensitivity,
                    flow_sensitivity: chr.flow_sensitivity,
                    
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
                };
                
                let mut strategy_params_map = HashMap::new();
                strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chr.inventory_target));
                strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
                strategy_params_map.insert("order_size_pct".to_string(), config::ParameterValue::Float(chr.order_size_pct));
                strategy_params_map.insert("window_ms".to_string(), config::ParameterValue::Float(chr.window_ms as f64));
                strategy_params_map.insert("fee_rate".to_string(), config::ParameterValue::Float(maker_fee));  // Use exchange fee
                strategy_params_map.insert("volatility_cap".to_string(), config::ParameterValue::Float(chr.volatility_cap));
                
                let strategy_config = StrategyConfig {
                    id: "genetic-eval".to_string(),
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
                    log_warn!(GENETIC_LOGGER, "FITNESS_EVAL #{} FAIL | strategy registration failed | elapsed={:.2}s", 
                        eval_id, eval_start.elapsed().as_secs_f64());
                    return genetic::FitnessResult::failure();
                }
                
                log_info!(GENETIC_LOGGER, "FITNESS_EVAL #{} | strategy registered, starting simulation on {} points...", eval_id, sampled_data.len());
                
                // Run ONLY core simulation (no Monte Carlo, no Walk-Forward)
                // Uses sampled data for faster fitness evaluation during GA exploration
                match engine.run_core_simulation_direct(
                    &mut strategy_manager,
                    &mut orderbook,
                    &mut portfolio,
                    &mut risk_manager,
                    &sampled_data,
                ).await {
                    Ok(result) => {
                        // Use comprehensive fitness function that penalizes commission-heavy strategies
                        let fitness = calculate_comprehensive_fitness(&result, initial_capital);
                        log_info_structured!(GENETIC_LOGGER, "FITNESS_EVAL_SUCCESS",
                            "event" => "FITNESS_EVAL_SUCCESS",
                            "eval_id" => eval_id,
                            "fitness" => format!("{:.6}", fitness),
                            "pnl" => format!("{:.2}", result.total_pnl),
                            "trade_count" => result.num_trades,
                            "elapsed_secs" => format!("{:.2}", eval_start.elapsed().as_secs_f64())
                        );
                        genetic::FitnessResult::new(fitness, result.equity_curve)
                    }
                    Err(e) => {
                        log_warn!(GENETIC_LOGGER, "FITNESS_EVAL #{} ERROR | {} | elapsed={:.2}s", 
                            eval_id, e, eval_start.elapsed().as_secs_f64());
                        genetic::FitnessResult::failure()
                    }
                }
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
            
            // Create distributed GA config
            let distributed_config = DistributedGAConfig {
                genetic_config: config_for_distributed.clone(),
                symbol: params.symbol.clone(),
                exchange: params.exchange.clone(),
                initial_capital,
                cache_ttl_seconds: 3600, // 1 hour cache TTL
            };
            eprintln!("🧬 Created DistributedGAConfig, about to call run_with_fallback...");
            
            // Clone data for the closure
            let data_for_fallback = Arc::clone(&data_for_distributed);
            let symbol_for_fallback = symbol_for_distributed.clone();
            let exchange_for_fallback = exchange_for_distributed.clone();
            let initial_capital_fallback = initial_capital_for_distributed;
            let initial_inventory_pct_fallback = initial_inventory_pct_for_distributed;
            let commission_rate_fallback = commission_rate_for_distributed;
            
            // Run with automatic fallback to local if distributed fails
            DistributedGARunner::run_with_fallback(
                &distributed_mode,
                &distributed_config,
                &shared_data,  // Pass slice reference
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
                            take_profit_pct: chr.take_profit_pct,
                            stop_loss_pct: chr.stop_loss_pct,
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
            "take_profit_pct" => format!("{:.4}", best_chromosome.take_profit_pct),
            "stop_loss_pct" => format!("{:.4}", best_chromosome.stop_loss_pct),
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
        
        // Update phase for final analysis with granular sub-phases
        progress_tracker.broadcast_phase("final_analysis", serde_json::json!({
            "status": "starting",
            "sub_phase": "backtest",
            "monte_carlo_enabled": params.monte_carlo_enabled,
            "walk_forward_enabled": params.walk_forward_enabled,
            "data_points": data_for_backtest.len(),
        }));
        let _ = progress_tracker.persist_phase_change("final_analysis", serde_json::json!({
            "status": "starting",
            "sub_phase": "backtest",
            "monte_carlo_runs": params.monte_carlo_runs.min(200),
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
            &best_chromosome,
            &data_for_backtest,      // Full data for backtest + Monte Carlo
            &data_for_walkforward,   // Full data for walk-forward
            params.monte_carlo_enabled,
            params.monte_carlo_runs.min(200), // Cap at 200 runs for speed (1000 is excessive)
            params.walk_forward_enabled,
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
            take_profit_pct: params.take_profit_pct,
            stop_loss_pct: params.stop_loss_pct,
            min_spread_bps: params.min_spread_bps,
            max_trades_per_minute: params.max_trades_per_minute,
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
    async fn run_full_analysis_with_chromosome(
        &self,
        job_id: &str,
        symbol: &str,
        exchange: &str,
        initial_capital: f64,
        initial_inventory_pct: f64,
        commission_rate: f64,
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        backtest_data: &[MarketData],
        walkforward_data: &[MarketData],
        monte_carlo_enabled: bool,
        monte_carlo_runs: usize,
        walk_forward_enabled: bool,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        // Run backtest + Monte Carlo on sampled data for speed
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_ENTER | backtest_pts={} walkforward_pts={} mc={} wf={}", 
            job_id, backtest_data.len(), walkforward_data.len(), monte_carlo_enabled, walk_forward_enabled);
        
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = initial_capital;
        backtest_config.trading.commission_rate = commission_rate;
        backtest_config.analysis.skip_genetic_optimization = true; // Already done by worker
        backtest_config.analysis.auto_monte_carlo = monte_carlo_enabled;
        backtest_config.analysis.monte_carlo_runs = monte_carlo_runs;
        backtest_config.analysis.auto_walk_forward = false; // We'll run WF separately with full data
        backtest_config.analysis.mode = if monte_carlo_enabled {
            config::AnalysisMode::Production
        } else {
            config::AnalysisMode::Development
        };
        backtest_config.data.gap_detection.enabled = false;
        
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_CONFIG | gap_detection=false mode={:?}", 
            job_id, backtest_config.analysis.mode);
        
        let engine = BacktestEngine::new(backtest_config.clone());
        
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_ENGINE_CREATED", job_id);
        
        // Get initial price for inventory calculation
        let asset_price = Self::get_reference_price(backtest_data);
        
        log_info!(MAIN_LOGGER, "Job {} FINAL_ANALYSIS_ASSET_PRICE | price={:.2}", job_id, asset_price);
        
        // Initialize portfolio with BOTH quote and base assets for proper market making
        let inventory_pct = initial_inventory_pct;
        let quote_balance = initial_capital * (1.0 - inventory_pct);
        let base_value = initial_capital * inventory_pct;
        let base_quantity = if asset_price > 0.0 { base_value / asset_price } else { 0.0 };
        let first_timestamp = Self::get_first_timestamp(backtest_data);
        
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
        
        // IMPORTANT: Explicitly set ALL parameters including order_size_pct and initial_capital
        // Using ..Default::default() would use wrong initial_capital ($100K instead of job's capital)
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: order_size_units,
            order_size_pct: chromosome.order_size_pct,
            initial_capital,  // Use the job's initial_capital, NOT the default $100K
            initial_inventory_pct: 0.5,  // 50% of capital as base asset for market making
            window_ms: chromosome.window_ms,
            min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms,
            fee_rate: maker_fee,  // Use actual exchange maker fee, not chromosome
            // latency_buffer removed - latency simulated in order matching
            volatility_cap: chromosome.volatility_cap,
            volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
            time_horizon_minutes: chromosome.time_horizon_minutes,
            market_depth_k: chromosome.market_depth_k,
            min_spread_bps: chromosome.min_spread_bps,
            max_trades_per_minute: chromosome.max_trades_per_minute,
            take_profit_pct: chromosome.take_profit_pct,
            stop_loss_pct: chromosome.stop_loss_pct,
            volatility_adjustment_factor: chromosome.volatility_adjustment_factor,
            inventory_penalty_factor: chromosome.inventory_penalty_factor,
            trend_sensitivity: chromosome.trend_sensitivity,
            flow_sensitivity: chromosome.flow_sensitivity,
            
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
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chromosome.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chromosome.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
        strategy_params_map.insert("order_size_pct".to_string(), config::ParameterValue::Float(chromosome.order_size_pct));
        strategy_params_map.insert("initial_capital".to_string(), config::ParameterValue::Float(initial_capital));
        
        let strategy_config = StrategyConfig {
            id: format!("full-analysis-{}", job_id),
            name: "Avellaneda-Stoikov".to_string(),
            strategy_type: "AvellanedaStoikov".to_string(),
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            parameters: strategy_params_map,
            risk_limits: StrategyRiskLimits::default(),
            enabled: true,
        };
        
        // Clone before moving for potential walk-forward reuse
        let strategy_params_for_wf = strategy_params.clone();
        let strategy_config_for_wf = strategy_config.clone();
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        let mut strategy_manager = StrategyManager::new();
        strategy_manager.register_strategy(strategy_config, strategy)
            .map_err(|e| format!("Failed to register strategy: {}", e))?;
        
        // Log the phases we're about to run
        log_info!(MAIN_LOGGER, "Job {} starting final analysis pipeline: Backtest{}{}",
            job_id,
            if monte_carlo_enabled { format!(" → Monte Carlo ({} runs)", monte_carlo_runs) } else { "".to_string() },
            if walk_forward_enabled { " → Walk-Forward" } else { "" });
        
        // Run backtest + Monte Carlo on sampled data (fast)
        // Walk-forward disabled in engine - we'll run it separately with full data
        log_info!(MAIN_LOGGER, "Job {} CALLING_ENGINE_RUN_BACKTEST | data_points={}", job_id, backtest_data.len());
        
        // SYNC PRINT for diagnosis - this will appear in stdout immediately
        eprintln!("[SYNC] Job {} ABOUT_TO_CALL_ENGINE_RUN_BACKTEST | data_points={}", job_id, backtest_data.len());
        
        // ============ TIMING: Backtest + Monte Carlo ============
        let backtest_start = std::time::Instant::now();
        
        let mut result = engine.run_backtest(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            backtest_data,
        ).await?;
        
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
        
        log_info!(MAIN_LOGGER, "Job {} ENGINE_RUN_BACKTEST_COMPLETE | elapsed={:.1}s trades={}", 
            job_id, backtest_start.elapsed().as_secs_f64(), result.core.num_trades);
        
        // Debug: Log portfolio state after final backtest using MAIN_LOGGER (which we know works)
        log_info!(MAIN_LOGGER, "Job {} final portfolio | balance={} positions={} open={}",
            job_id,
            portfolio.balance.to_string(),
            portfolio.positions.len(),
            portfolio.positions.iter().filter(|p| p.close_time.is_none()).count()
        );
        
        // Log each position for debugging the PnL issue
        for (i, pos) in portfolio.positions.iter().enumerate() {
            let mark = pos.mark_price.as_ref().map(|p| p.to_string()).unwrap_or_else(|| "None".to_string());
            log_info!(MAIN_LOGGER, "Job {} position {} | side={:?} qty={} entry={} mark={} closed={}",
                job_id, i, pos.side, pos.quantity, pos.entry_price, mark, pos.close_time.is_some()
            );
        }
        
        // Log total value calculation
        let total_value = portfolio.get_total_value();
        log_info!(MAIN_LOGGER, "Job {} total_value={} initial={} => pnl={}",
            job_id, total_value, initial_capital, &total_value - bigdecimal::BigDecimal::from_f64(initial_capital).unwrap_or_default()
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
        
        // Run walk-forward on FULL data separately (engine samples per-window at 200K max)
        // This validates the strategy generalizes across all time periods
        if walk_forward_enabled && !walkforward_data.is_empty() {
            log_info!(MAIN_LOGGER, "Job {} starting Walk-Forward on {} data points", job_id, walkforward_data.len());
            
            // ============ TIMING: Walk-Forward ============
            let wf_timing_start = std::time::Instant::now();
            eprintln!("⏱️ TIMING | WALK_FORWARD | starting on {} data points...", walkforward_data.len());
            
            // Create a separate engine config for walk-forward only
            let mut wf_config = backtest_config.clone();
            wf_config.analysis.auto_walk_forward = true;
            wf_config.analysis.auto_monte_carlo = false; // Already done above
            
            let wf_engine = BacktestEngine::new(wf_config);
            
            // Fresh state for walk-forward
            let wf_asset_price = Self::get_reference_price(walkforward_data);
            let wf_first_timestamp = Self::get_first_timestamp(walkforward_data);
            let wf_quote_balance = initial_capital * 0.5;
            let wf_base_quantity = if wf_asset_price > 0.0 { (initial_capital * 0.5) / wf_asset_price } else { 0.0 };
            
            let mut wf_portfolio = PortfolioState::with_inventory(
                symbol.to_string(),
                BigDecimal::from_f64(wf_quote_balance).unwrap_or_default(),
                wf_base_quantity,
                BigDecimal::from_f64(wf_asset_price).unwrap_or_default(),
                wf_first_timestamp,
            );
            let mut wf_orderbook = OrderBook::new(symbol.to_string(), exchange.to_string(), OrderBookConfig::default());
            let mut wf_risk_manager = RiskManager::new();
            
            // Re-register strategy for walk-forward (using cloned versions)
            let wf_strategy = AvellanedaStoikovStrategy::new(strategy_params_for_wf);
            let mut wf_strategy_manager = StrategyManager::new();
            let _ = wf_strategy_manager.register_strategy(strategy_config_for_wf, wf_strategy);
            
            match wf_engine.run_backtest(
                &mut wf_strategy_manager,
                &mut wf_orderbook,
                &mut wf_portfolio,
                &mut wf_risk_manager,
                walkforward_data,
            ).await {
                Ok(wf_result) => {
                    let wf_timing_elapsed = wf_timing_start.elapsed();
                    eprintln!("⏱️ TIMING | WALK_FORWARD | completed in {:.2}s ({:.1} min)", 
                        wf_timing_elapsed.as_secs_f64(), wf_timing_elapsed.as_secs_f64() / 60.0);
                    
                    // Structured metadata for Kibana
                    log_info_structured!(MAIN_LOGGER, "TIMING_WALKFORWARD_COMPLETE",
                        "event" => "TIMING_WALKFORWARD_COMPLETE",
                        "job_id" => job_id,
                        "elapsed_sec" => format!("{:.2}", wf_timing_elapsed.as_secs_f64()),
                        "elapsed_min" => format!("{:.1}", wf_timing_elapsed.as_secs_f64() / 60.0),
                        "data_points" => walkforward_data.len()
                    );
                    
                    // Merge walk-forward result into main result
                    result.walk_forward = wf_result.walk_forward;
                    if let Some(ref wf) = result.walk_forward {
                        log_info!(MAIN_LOGGER, "Job {} Walk-Forward complete | periods={} avg_perf={:.2}% consistency={:.1}% OOS_degradation={:.1}%",
                            job_id, wf.periods_tested, wf.average_performance * 100.0, wf.consistency_score * 100.0, wf.out_of_sample_degradation * 100.0);
                    }
                }
                Err(e) => {
                    let wf_timing_elapsed = wf_timing_start.elapsed();
                    eprintln!("⏱️ TIMING | WALK_FORWARD | FAILED after {:.2}s: {}", wf_timing_elapsed.as_secs_f64(), e);
                    log_warn!(MAIN_LOGGER, "Job {} Walk-Forward failed: {} - continuing without WF results", job_id, e);
                }
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
        })
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
            take_profit_pct: chromosome.take_profit_pct,
            stop_loss_pct: chromosome.stop_loss_pct,
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
            strategy_params: Some(serde_json::json!({
                "risk_aversion": chromosome.risk_aversion,
                "inventory_target": chromosome.inventory_target,
                "order_size_pct": chromosome.order_size_pct,
                "order_size_units": order_size_units
            })),
            start_date: None, // Chromosome evaluation doesn't have date range
            end_date: None,
            monte_carlo: None, // Chromosome evaluation doesn't run MC
            walk_forward: None, // Chromosome evaluation doesn't run WF
            inventory_metrics: None, // Not tracked for chromosome eval
            execution_metrics: None, // Not tracked for chromosome eval
            risk_halt_reason: None,
            metrics_profile: Some(Self::build_metrics_profile("market_making")),
        })
    }
    
    /// Load market data intelligently based on strategy type
    /// 
    /// This function determines what data to load based on strategy requirements:
    /// - Market making strategies need order flow + trades (tick-level data)
    /// - Momentum strategies need OHLCV candles
    /// - Arbitrage strategies need both real-time data sources
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        log_info!(GENETIC_LOGGER, "ENTER load_market_data_for_strategy | type='{}' sym={} ex={}", 
            strategy_type, symbol, exchange);
        
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
            self.load_order_events(symbol, exchange).await
        } else {
            log_info!(GENETIC_LOGGER, "Strategy '{}' requires candle data", strategy_type);
            // For momentum/trend strategies, try candles first
            match self.load_market_data(symbol, exchange).await {
                Ok(data) if !data.is_empty() => {
                    log_info!(GENETIC_LOGGER, "Successfully loaded {} candles", data.len());
                    Ok(data)
                }
                Ok(_) => {
                    log_warn!(GENETIC_LOGGER, "No candle data found, falling back to order events for {} on {}", symbol, exchange);
                    // Fall back to order events if no candles
                    self.load_order_events(symbol, exchange).await
                }
                Err(e) => {
                    log_warn!(GENETIC_LOGGER, "Failed to load candles ({}), falling back to order events", e);
                    // Fall back to order events if candle query fails (e.g., view doesn't exist)
                    self.load_order_events(symbol, exchange).await
                }
            }
        }
    }
    
    /// Load order book events (new, cancel, modify) for market making strategies
    /// Uses PARALLEL streaming to load FULL dataset ~10x faster than sequential
    #[cfg(feature = "postgres")]
    async fn load_order_events(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::streaming::ParallelDataLoader;
        
        log_info!(MAIN_LOGGER, "Loading ALL market data for {} on {} using PARALLEL loader (10 workers)", symbol, exchange);
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        // Use parallel loader with 10 concurrent connections
        // Divides data into 10 time-based partitions and loads them concurrently
        // ~10x faster than sequential loading
        const NUM_WORKERS: usize = 10;
        let loader = ParallelDataLoader::new(pool, NUM_WORKERS);
        
        // Load ALL data - no limits
        // Memory management is handled by running only 1 job at a time (--threads 1)
        let merged = loader.load_all(symbol, exchange).await?;
        
        log_info!(MAIN_LOGGER, "Loaded {} total events (orders + trades) using parallel streaming", merged.len());
        Ok(merged)
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


