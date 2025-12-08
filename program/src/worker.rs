//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};

use crate::job_queue::{BacktestJob, BacktestResult, JobQueue, JobStatus, MonteCarloSummary, WalkForwardSummary};

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
fn calculate_comprehensive_fitness(result: &backtest::types::BacktestResult, initial_capital: f64) -> f64 {
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
                log_info!(GENETIC_LOGGER, "Job {} generation {}/{} ({:.1}%)", 
                    job_id, current, total, (current as f64 / total as f64) * 100.0);
                
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
        log_info!(MAIN_LOGGER, "Worker {} started and polling for jobs", self.id);
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
        
        log_info!(MAIN_LOGGER, "Worker {} claimed job {} | symbol={} exchange={} method={}", 
            self.id, job.id, job.params.symbol, job.params.exchange, job.params.optimization_method);
        log_debug!(MAIN_LOGGER, "🎯 Worker {} claimed job: {} (symbol={}, exchange={})", 
            self.id, job.id, job.params.symbol, job.params.exchange);
        
        // Note: get_next_job() already set status to "running" atomically in the database
        
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
                log_info!(GENETIC_LOGGER, "Job {} starting genetic optimization | pop_size={} generations={}", 
                    job.id, job.params.population_size, job.params.generations);
                self.execute_genetic_optimization(&job).await
            },
            "walkforward" => {
                log_info!(WALKFORWARD_LOGGER, "Job {} starting walk-forward analysis | opt_days={:?} test_days={:?}", 
                    job.id, job.params.walkforward_optimization_days, job.params.walkforward_test_days);
                log_debug!(MAIN_LOGGER, "🔄 Starting walk-forward analysis for job {}", job.id);
                self.execute_walkforward_analysis(&job).await
            },
            _ => {
                log_info!(BACKTEST_LOGGER, "Job {} starting manual backtest | risk_aversion={} order_size={}", 
                    job.id, job.params.risk_aversion, job.params.order_size);
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
                if let Some(ref pool) = self.db_pool {
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
        
        // Initialize portfolio
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from_f64(params.initial_capital)
            .ok_or("Invalid initial capital")?;
        
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
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
        
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
                    let annualized = ((1.0 + total_return).powf(252.0 / trading_days) - 1.0);
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
            risk_halt_reason: None,
        };
        
        log_info!(BACKTEST_LOGGER, "Job {} backtest complete | pnl={:.2} sharpe={:.3} trades={}", 
            job.id, backtest_result.total_pnl, backtest_result.sharpe_ratio, backtest_result.total_trades);
        
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
        
        log_info!(GENETIC_LOGGER, "Job {} genetic optimization starting | pop={} gen={} symbol={} strategy={}", 
            job.id, params.population_size, params.generations, params.symbol, params.strategy_type);
        
        // Create efficient progress tracker (broadcasts via channel, no DB in hot path)
        let progress_tracker = Arc::new(ProgressTracker::new(job.id.clone(), Arc::clone(&self.job_queue)));
        
        // Persist phase change to DB (lifecycle event) + broadcast
        progress_tracker.broadcast_phase("genetic_optimization", serde_json::json!({
            "population_size": params.population_size,
            "generations": params.generations,
        }));
        let _ = progress_tracker.persist_phase_change("genetic_optimization", serde_json::json!({
            "population_size": params.population_size,
            "generations": params.generations,
        })).await;
        
        // Load market data
        log_info!(GENETIC_LOGGER, "Job {} loading market data for {} on {}", job.id, params.symbol, params.exchange);
        
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
        if data.is_empty() {
            log_error!(GENETIC_LOGGER, "Job {} no market data available", job.id);
            return Err("No market data available".into());
        }
        
        log_info!(GENETIC_LOGGER, "Job {} loaded {} market data points", job.id, data.len());
        
        // Subsample data for faster optimization (full data used for final backtest)
        const MAX_OPTIMIZATION_SAMPLES: usize = 5000;
        let optimization_data: Vec<MarketData> = if data.len() > MAX_OPTIMIZATION_SAMPLES {
            let step = data.len() / MAX_OPTIMIZATION_SAMPLES;
            data.iter().step_by(step).take(MAX_OPTIMIZATION_SAMPLES).cloned().collect()
        } else {
            data.clone()
        };
        
        log_info!(GENETIC_LOGGER, "Job {} using {} samples for optimization", job.id, optimization_data.len());
        
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
        
        // Prepare data for fitness function (as Arc for sharing)
        // Use subsampled data for faster optimization
        let data_arc = Arc::new(optimization_data);
        let data_for_final = Arc::new(data); // Full data for final backtest
        let symbol = params.symbol.clone();
        let exchange = params.exchange.clone();
        let initial_capital = params.initial_capital;
        let commission_rate = params.commission_rate;
        
        // Create fitness function that evaluates chromosomes and returns equity curve for robustness analysis
        // THREAD SAFETY: We clone Arc references and create fresh state inside each async block
        let fitness_fn: genetic::AsyncFitnessFn<AvellanedaStoikovChromosome> = Arc::new(move |chromosome: &AvellanedaStoikovChromosome| {
            let data = Arc::clone(&data_arc);
            let symbol = symbol.clone();
            let exchange = exchange.clone();
            let chr = chromosome.clone(); // Clone the chromosome to move into async
            
            Box::pin(async move {
                // Each async block gets its own fresh state - no shared mutable state
                let mut backtest_config = BacktestConfig::default();
                backtest_config.trading.initial_capital = initial_capital;
                backtest_config.trading.commission_rate = commission_rate;
                backtest_config.analysis.skip_genetic_optimization = true;
                backtest_config.analysis.auto_monte_carlo = false;
                backtest_config.analysis.auto_walk_forward = false;
                backtest_config.data.gap_detection.enabled = false;
                
                let engine = BacktestEngine::new(backtest_config);
                let mut portfolio = PortfolioState::default();
                portfolio.balance = BigDecimal::from_f64(initial_capital).unwrap_or_default();
                
                let orderbook_config = OrderBookConfig::default();
                let mut orderbook = OrderBook::new(symbol.clone(), exchange.clone(), orderbook_config);
                let mut risk_manager = RiskManager::new();
                
                // Calculate order_size in asset units from percentage of capital
                // order_size = (capital * order_size_pct) / asset_price
                let asset_price = Self::get_reference_price(&data);
                let order_size_units = (initial_capital * chr.order_size_pct) / asset_price;
                
                // Create strategy with ALL chromosome parameters (fixing incomplete mapping)
                let strategy_params = AvellanedaStoikovParams {
                    risk_aversion: chr.risk_aversion,
                    inventory_target: chr.inventory_target,
                    order_size: order_size_units,
                    window_ms: chr.window_ms,
                    min_quote_lifetime_ms: chr.min_quote_lifetime_ms,
                    fee_rate: chr.fee_rate,
                    latency_buffer: chr.latency_buffer,
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
                };
                
                let mut strategy_params_map = HashMap::new();
                strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chr.inventory_target));
                strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
                strategy_params_map.insert("order_size_pct".to_string(), config::ParameterValue::Float(chr.order_size_pct));
                strategy_params_map.insert("window_ms".to_string(), config::ParameterValue::Float(chr.window_ms as f64));
                strategy_params_map.insert("fee_rate".to_string(), config::ParameterValue::Float(chr.fee_rate));
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
                    return genetic::FitnessResult::failure();
                }
                
                // Run ONLY core simulation (no Monte Carlo, no Walk-Forward)
                match engine.run_core_simulation_direct(
                    &mut strategy_manager,
                    &mut orderbook,
                    &mut portfolio,
                    &mut risk_manager,
                    &data,
                ).await {
                    Ok(result) => {
                        // Use comprehensive fitness function that penalizes commission-heavy strategies
                        let fitness = calculate_comprehensive_fitness(&result, initial_capital);
                        genetic::FitnessResult::new(fitness, result.equity_curve)
                    }
                    Err(_) => genetic::FitnessResult::failure(),
                }
            })
        });
        
        // Run genetic optimization with robustness-weighted fitness
        log_info!(GENETIC_LOGGER, "Job {} creating GeneticOptimizer", job.id);
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: Some(generation_callback),
            job_id: Some(job.id.clone()),
        };
        
        log_info!(GENETIC_LOGGER, "Job {} starting genetic algorithm with robustness-weighted fitness", job.id);
        let (best_chromosome, best_fitness) = optimizer.run().await;
        log_info!(GENETIC_LOGGER, "Job {} optimizer.run() completed | best_fitness={:.6}", job.id, best_fitness);
        
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
        let _ = progress_tracker.persist_phase_change("post_optimization", serde_json::json!({
            "best_fitness": best_fitness,
            "risk_aversion": best_chromosome.risk_aversion,
            "inventory_target": best_chromosome.inventory_target,
        })).await;
        
        log_info!(GENETIC_LOGGER, "Job {} genetic optimization complete | best_fitness={:.4} risk_aversion={:.3} inventory_target={:.3} order_size_pct={:.2}%", 
            job.id, best_fitness, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size_pct * 100.0);
        
        // Update phase for final analysis
        progress_tracker.broadcast_phase("final_analysis", serde_json::json!({
            "stage": "monte_carlo_walkforward"
        }));
        progress_tracker.broadcast_progress(90.0);
        let _ = progress_tracker.persist_phase_change("final_analysis", serde_json::json!({
            "stage": "monte_carlo_walkforward",
            "progress": 90.0
        })).await;
        let _ = progress_tracker.persist_progress(90.0).await;
        
        // Run final backtest with best parameters to get full results
        // This includes Monte Carlo and Walk-Forward analysis automatically
        log_info!(GENETIC_LOGGER, "Job {} running full analysis pipeline (Backtest → Monte Carlo → Walk-Forward)", job.id);
        log_debug!(MAIN_LOGGER, "🚀 About to run final backtest for job {}", job.id);
        
        let mut final_result = self.run_full_analysis_with_chromosome(
            &job.id,
            &params.symbol,
            &params.exchange,
            params.initial_capital,
            params.commission_rate,
            &best_chromosome,
            &data_for_final,
            params.monte_carlo_enabled,
            params.monte_carlo_runs,
            params.walk_forward_enabled,
        ).await?;
        
        // If strategy was below viability threshold, add warning to result
        if strategy_below_threshold {
            final_result.risk_halt_reason = Some(format!(
                "NOT_VIABLE: Best fitness {:.6} < 0.01 threshold. Commission costs exceeded 80% of gross profits. \
                This market/timeframe may not be suitable for market-making.",
                best_fitness
            ));
        }
        
        // Final broadcast + persist (lifecycle event)
        progress_tracker.broadcast_progress(100.0);
        progress_tracker.broadcast_phase("completed", serde_json::json!({
            "total_pnl": final_result.total_pnl,
            "sharpe_ratio": final_result.sharpe_ratio,
        }));
        let _ = progress_tracker.persist_progress(100.0).await;
        let _ = progress_tracker.persist_phase_change("completed", serde_json::json!({
            "total_pnl": final_result.total_pnl,
            "sharpe_ratio": final_result.sharpe_ratio,
        })).await;
        
        log_info!(GENETIC_LOGGER, "Job {} full analysis complete | pnl={:.2} sharpe={:.3}", 
            job.id, final_result.total_pnl, final_result.sharpe_ratio);
        
        Ok(final_result)
    }
    
    /// Execute walk-forward analysis: rolling optimization/test windows
    /// 
    /// Walk-forward tests the robustness of the optimization process itself,
    /// not just specific parameters. Each window:
    /// 1. Runs genetic optimization on in-sample data (optimization window)
    /// 2. Tests the optimal parameters on out-of-sample data (test window)
    /// 3. Slides forward and repeats
    async fn execute_walkforward_analysis(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        use genetic::GeneticOptimizer;
        use genetic::chromosome::AvellanedaStoikovChromosome;
        use genetic::FitnessResult;
        use config::GeneticConfig;
        
        let params = &job.params;
        
        // Walk-forward configuration with defaults
        let optimization_days = params.walkforward_optimization_days.unwrap_or(180);
        let test_days = params.walkforward_test_days.unwrap_or(30);
        let step_days = params.walkforward_step_days.unwrap_or(30);
        
        log_info!(WALKFORWARD_LOGGER, "Job {} walk-forward config | opt_days={} test_days={} step_days={}", 
            job.id, optimization_days, test_days, step_days);
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
        log_info!(WALKFORWARD_LOGGER, "Job {} data range {} to {} | {} data points", 
            job.id, data_start.format("%Y-%m-%d"), data_end.format("%Y-%m-%d"), all_data.len());
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
        
        log_info!(WALKFORWARD_LOGGER, "Job {} generated {} walk-forward windows", job.id, windows.len());
        log_debug!(MAIN_LOGGER, "🔢 Generated {} walk-forward windows", windows.len());
        
        // Track results for each window
        let mut window_results: Vec<WalkForwardWindowResult> = Vec::new();
        let mut aggregated_equity_curve: Vec<f64> = Vec::new();
        let total_windows = windows.len();
        
        // Genetic config for each window's optimization
        let config = GeneticConfig {
            population_size: params.population_size,
            generations: params.generations,
            mutation_rate: 0.1,
            crossover_rate: 0.7,
            ..Default::default()
        };
        
        for (window_idx, window) in windows.iter().enumerate() {
            let window_num = window_idx + 1;
            
            log_info!(WALKFORWARD_LOGGER, "Job {} window {}/{} | opt: {} to {} | test: {} to {}", 
                job.id, window_num, total_windows,
                window.opt_start.format("%Y-%m-%d"), window.opt_end.format("%Y-%m-%d"),
                window.test_start.format("%Y-%m-%d"), window.test_end.format("%Y-%m-%d"));
            
            log_debug!(MAIN_LOGGER, "\n🔄 Window {}/{}: Opt {} to {}, Test {} to {}", 
                window_num, total_windows,
                window.opt_start.format("%Y-%m-%d"),
                window.opt_end.format("%Y-%m-%d"),
                window.test_start.format("%Y-%m-%d"),
                window.test_end.format("%Y-%m-%d"));
            
            // Update progress
            let progress = (window_idx as f64 / total_windows as f64) * 90.0;
            let _ = self.job_queue.update_progress(&job.id, progress).await;
            let _ = self.job_queue.update_phase(
                &job.id,
                "walkforward_window",
                serde_json::json!({
                    "current_window": window_num,
                    "total_windows": total_windows,
                    "phase": "optimizing"
                })
            ).await;
            
            // Filter data for optimization window
            let opt_data: Vec<MarketData> = all_data.iter()
                .filter(|d| {
                    let ts = self.get_market_data_timestamp(d);
                    ts >= window.opt_start && ts < window.opt_end
                })
                .cloned()
                .collect();
            
            if opt_data.is_empty() {
                log_warn!(WALKFORWARD_LOGGER, "Job {} window {} no optimization data, skipping", job.id, window_num);
                log_debug!(MAIN_LOGGER, "⚠️ Window {}: No optimization data, skipping", window_num);
                continue;
            }
            
            log_debug!(WALKFORWARD_LOGGER, "Job {} window {} optimization data: {} points", job.id, window_num, opt_data.len());
            
            // Run genetic optimization on this window's in-sample data
            let opt_data_arc = Arc::new(opt_data);
            let symbol = params.symbol.clone();
            let exchange = params.exchange.clone();
            let initial_capital = params.initial_capital;
            let commission_rate = params.commission_rate;
            
            let fitness_fn: genetic::AsyncFitnessFn<AvellanedaStoikovChromosome> = {
                let data = Arc::clone(&opt_data_arc);
                Arc::new(move |chromosome: &AvellanedaStoikovChromosome| {
                    let data = Arc::clone(&data);
                    let symbol = symbol.clone();
                    let exchange = exchange.clone();
                    let chr = chromosome.clone();
                    
                    Box::pin(async move {
                        let mut backtest_config = BacktestConfig::default();
                        backtest_config.trading.initial_capital = initial_capital;
                        backtest_config.trading.commission_rate = commission_rate;
                        backtest_config.analysis.skip_genetic_optimization = true;
                        backtest_config.analysis.auto_monte_carlo = false;
                        backtest_config.analysis.auto_walk_forward = false;
                        backtest_config.data.gap_detection.enabled = false;
                        
                        let engine = BacktestEngine::new(backtest_config);
                        let mut portfolio = PortfolioState::default();
                        portfolio.balance = BigDecimal::from_f64(initial_capital).unwrap_or_default();
                        
                        let orderbook_config = OrderBookConfig::default();
                        let mut orderbook = OrderBook::new(symbol.clone(), exchange.clone(), orderbook_config);
                        let mut risk_manager = RiskManager::new();
                        
                        // Calculate order_size in asset units from percentage
                        let asset_price = Self::get_reference_price(&data);
                        let order_size_units = (initial_capital * chr.order_size_pct) / asset_price;
                        
                        let strategy_params = AvellanedaStoikovParams {
                            risk_aversion: chr.risk_aversion,
                            inventory_target: chr.inventory_target,
                            order_size: order_size_units,
                            ..Default::default()
                        };
                        
                        let mut strategy_params_map = HashMap::new();
                        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chr.inventory_target));
                        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
                        
                        let strategy_config = StrategyConfig {
                            id: "wf-genetic-eval".to_string(),
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
                            return FitnessResult::new(0.0, Vec::new());
                        }
                        
                        match engine.run_core_simulation_direct(
                            &mut strategy_manager,
                            &mut orderbook,
                            &mut portfolio,
                            &mut risk_manager,
                            &data,
                        ).await {
                            Ok(result) => {
                                // Use comprehensive fitness function that penalizes commission-heavy strategies
                                let fitness = calculate_comprehensive_fitness(&result, initial_capital);
                                FitnessResult::new(fitness, result.equity_curve.clone())
                            }
                            Err(_) => FitnessResult::new(0.0, Vec::new()),
                        }
                    })
                })
            };
            
            let optimizer = GeneticOptimizer {
                config: config.clone(),
                fitness_fn,
                progress_callback: None, // Don't update DB for each window's generations
                job_id: Some(format!("{}-window-{}", job.id, window_num)),
            };
            
            log_debug!(WALKFORWARD_LOGGER, "Job {} window {} starting genetic optimization", job.id, window_num);
            log_debug!(MAIN_LOGGER, "🧬 Window {}: Running genetic optimization...", window_num);
            let (best_chromosome, optimization_fitness) = optimizer.run().await;
            
            // Check if optimization found a viable strategy
            if optimization_fitness < 0.01 {
                log_warn!(WALKFORWARD_LOGGER, "Job {} window {} no viable strategy found (fitness={:.6}), skipping window", 
                    job.id, window_num, optimization_fitness);
                continue;
            }
            
            log_info!(WALKFORWARD_LOGGER, "Job {} window {} optimization complete | fitness={:.4} risk_aversion={:.3} inventory_target={:.3} order_size_pct={:.2}%",
                job.id, window_num, optimization_fitness, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size_pct * 100.0);
            log_debug!(MAIN_LOGGER, "✅ Window {}: Best params - risk_aversion={:.3}, inventory_target={:.3}, order_size_pct={:.2}%",
                window_num, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size_pct * 100.0);
            
            // Update phase to testing
            let _ = self.job_queue.update_phase(
                &job.id,
                "walkforward_window",
                serde_json::json!({
                    "current_window": window_num,
                    "total_windows": total_windows,
                    "phase": "testing"
                })
            ).await;
            
            // Filter data for test window (out-of-sample)
            let test_data: Vec<MarketData> = all_data.iter()
                .filter(|d| {
                    let ts = self.get_market_data_timestamp(d);
                    ts >= window.test_start && ts < window.test_end
                })
                .cloned()
                .collect();
            
            if test_data.is_empty() {
                log_warn!(WALKFORWARD_LOGGER, "Job {} window {} no test data, skipping", job.id, window_num);
                log_debug!(MAIN_LOGGER, "⚠️ Window {}: No test data, skipping", window_num);
                continue;
            }
            
            log_debug!(WALKFORWARD_LOGGER, "Job {} window {} testing on {} out-of-sample points", job.id, window_num, test_data.len());
            // Run backtest on out-of-sample data with optimal parameters
            log_debug!(MAIN_LOGGER, "📈 Window {}: Testing on out-of-sample data ({} events)...", window_num, test_data.len());
            
            let test_result = self.run_backtest_with_chromosome(
                &format!("{}-window{}", job.id, window_num),
                &params.symbol,
                &params.exchange,
                params.initial_capital,
                params.commission_rate,
                &best_chromosome,
                &test_data,
            ).await?;
            
            log_info!(WALKFORWARD_LOGGER, "Job {} window {} OOS results | pnl={:.2} sharpe={:.3} trades={} max_dd={:.2}%", 
                job.id, window_num, test_result.total_pnl, test_result.sharpe_ratio, test_result.total_trades, test_result.max_drawdown * 100.0);
            log_debug!(MAIN_LOGGER, "📊 Window {}: OOS PnL={:.2}, Sharpe={:.3}, Trades={}", 
                window_num, test_result.total_pnl, test_result.sharpe_ratio, test_result.total_trades);
            
            // Store window result
            window_results.push(WalkForwardWindowResult {
                window_id: window_num as u32,
                opt_start: window.opt_start,
                opt_end: window.opt_end,
                test_start: window.test_start,
                test_end: window.test_end,
                optimal_params: OptimalParams {
                    risk_aversion: best_chromosome.risk_aversion,
                    inventory_target: best_chromosome.inventory_target,
                    order_size_pct: best_chromosome.order_size_pct,
                },
                optimization_fitness,
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
        log_info!(WALKFORWARD_LOGGER, "Job {} aggregating {} window results", job.id, window_results.len());
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
        
        log_info!(WALKFORWARD_LOGGER, "Job {} walk-forward complete | windows={} total_pnl={:.2} avg_sharpe={:.3} max_dd={:.2}% param_stability={:.2}%", 
            job.id, window_results.len(), total_oos_pnl, avg_oos_sharpe, max_oos_drawdown * 100.0, param_stability * 100.0);
        
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
            risk_halt_reason: None,
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

        // Log single structured JSON entry - Kibana can parse and index all fields
        // VaR values are in USD, calculate percentages for easier interpretation
        let var_95_usd = result.value_at_risk_95.unwrap_or(0.0);
        let cvar_95_usd = result.expected_shortfall_95.unwrap_or(0.0);
        let var_95_pct = if initial_capital > 0.0 { var_95_usd / initial_capital * 100.0 } else { 0.0 };
        let cvar_95_pct = if initial_capital > 0.0 { cvar_95_usd / initial_capital * 100.0 } else { 0.0 };
        
        log_info!(MAIN_LOGGER, 
            r#"{{"event": "BACKTEST_RESULT", "job_id": "{}", "status": "{}", "initial_capital": {:.2}, "performance": {{"total_pnl": {:.2}, "gross_pnl": {:.2}, "sharpe_ratio": {:.4}, "max_drawdown_pct": {:.2}, "win_rate_pct": {:.2}, "profit_factor": {:.4}, "total_trades": {}}}, "returns": {{"annualized_pct": {:.2}, "volatility_pct": {:.2}, "sortino_ratio": {:.4}, "calmar_ratio": {:.4}}}, "trades": {{"avg_return": {:.4}, "avg_time_min": {:.1}, "best_trade": {:.2}, "worst_trade": {:.2}}}, "risk": {{"var_95_usd": {:.2}, "var_95_pct": {:.2}, "cvar_95_usd": {:.2}, "cvar_95_pct": {:.2}, "current_drawdown_pct": {:.2}, "avg_drawdown_pct": {:.2}, "max_dd_days": {}}}, "execution": {{"commission": {:.2}, "commission_ratio_pct": {:.1}, "avg_slippage_pct": {:.4}, "total_orders": {}, "filled_orders": {}, "cancelled_orders": {}, "fill_rate_pct": {:.1}}}{}{}}}"#,
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
            wf_json
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
    async fn run_full_analysis_with_chromosome(
        &self,
        job_id: &str,
        symbol: &str,
        exchange: &str,
        initial_capital: f64,
        commission_rate: f64,
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        data: &[MarketData],
        monte_carlo_enabled: bool,
        monte_carlo_runs: usize,
        walk_forward_enabled: bool,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = initial_capital;
        backtest_config.trading.commission_rate = commission_rate;
        backtest_config.analysis.skip_genetic_optimization = true; // Already done by worker
        backtest_config.analysis.auto_monte_carlo = monte_carlo_enabled;
        backtest_config.analysis.monte_carlo_runs = monte_carlo_runs;
        backtest_config.analysis.auto_walk_forward = walk_forward_enabled;
        backtest_config.analysis.mode = if monte_carlo_enabled || walk_forward_enabled {
            config::AnalysisMode::Production
        } else {
            config::AnalysisMode::Development
        };
        backtest_config.data.gap_detection.enabled = false;
        
        log_debug!(MAIN_LOGGER, "📊 Running full analysis pipeline: Backtest{}{}", 
            if monte_carlo_enabled { " → Monte Carlo" } else { "" },
            if walk_forward_enabled { " → Walk-Forward" } else { "" });
        
        let engine = BacktestEngine::new(backtest_config);
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from_f64(initial_capital).unwrap_or_default();
        
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(symbol.to_string(), exchange.to_string(), orderbook_config);
        let mut risk_manager = RiskManager::new();
        
        // Calculate order_size in asset units from percentage of capital
        let asset_price = Self::get_reference_price(data);
        let order_size_units = (initial_capital * chromosome.order_size_pct) / asset_price;
        
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: order_size_units,
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chromosome.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chromosome.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
        
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
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        let mut strategy_manager = StrategyManager::new();
        strategy_manager.register_strategy(strategy_config, strategy)
            .map_err(|e| format!("Failed to register strategy: {}", e))?;
        
        // Log the phases we're about to run
        log_info!(MAIN_LOGGER, "Job {} starting final analysis pipeline: Backtest{}{}",
            job_id,
            if monte_carlo_enabled { format!(" → Monte Carlo ({} runs)", monte_carlo_runs) } else { "".to_string() },
            if walk_forward_enabled { " → Walk-Forward" } else { "" });
        
        // Run full analysis (includes Monte Carlo and Walk-Forward)
        let result = engine.run_backtest(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            data,
        ).await?;
        
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
        }
        
        // Log Walk-Forward results if available
        if let Some(ref wf) = result.walk_forward {
            log_info!(MAIN_LOGGER, "Job {} Walk-Forward complete | periods={} avg_perf={:.2}% consistency={:.1}% OOS_degradation={:.1}%",
                job_id, wf.periods_tested, wf.average_performance * 100.0, wf.consistency_score * 100.0, wf.out_of_sample_degradation * 100.0);
        }
        
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
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
            risk_halt_reason: None,
        })
    }
    /// Used for fitness evaluation and walk-forward window testing
    async fn run_backtest_with_chromosome(
        &self,
        job_id: &str,
        symbol: &str,
        exchange: &str,
        initial_capital: f64,
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
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from_f64(initial_capital).unwrap_or_default();
        
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(symbol.to_string(), exchange.to_string(), orderbook_config);
        let mut risk_manager = RiskManager::new();
        
        // Calculate order_size in asset units from percentage of capital
        let asset_price = Self::get_reference_price(data);
        let order_size_units = (initial_capital * chromosome.order_size_pct) / asset_price;
        
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: order_size_units,
            ..Default::default()
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
            risk_halt_reason: None,
        })
    }
    
    /// Load market data intelligently based on strategy type
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        log_info!(GENETIC_LOGGER, "ENTER load_market_data_for_strategy | type='{}' sym={} ex={}", 
            strategy_type, symbol, exchange);
        
        match strategy_type {
            "market_making" => {
                log_info!(GENETIC_LOGGER, "Using order events loader for market_making strategy");
                // For market making, load order events to reconstruct order book
                self.load_order_events(symbol, exchange).await
            }
            _ => {
                log_info!(GENETIC_LOGGER, "Using candle loader for '{}' strategy", strategy_type);
                // Try to load candles first
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
    }
    
    /// Load order book events (new, cancel, modify) for market making strategies
    /// Uses sampling to reduce memory footprint for genetic optimization
    #[cfg(feature = "postgres")]
    async fn load_order_events(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::streaming::OrderBookBatchLoader;
        
        log_info!(MAIN_LOGGER, "Loading order events for {} on {}", symbol, exchange);
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        let loader = OrderBookBatchLoader::new(
            pool,
            symbol.to_string(),
            exchange.to_string(),
        );
        
        // Load with limit to prevent OOM during genetic optimization
        // 20K events is enough for meaningful backtesting while staying under memory limits
        const MAX_EVENTS: usize = 20000;
        
        log_info!(MAIN_LOGGER, "Starting batch load with limit {}", MAX_EVENTS);
        
        let orders = loader.load_with_limit(MAX_EVENTS).await?;
        
        log_info!(MAIN_LOGGER, "Loaded {} order events", orders.len());
        Ok(orders)
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


