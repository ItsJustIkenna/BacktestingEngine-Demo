//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::{sleep, Duration, Instant};
use tokio::sync::RwLock;
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};

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

/// Minimum interval between DB flushes (milliseconds)
const PROGRESS_FLUSH_INTERVAL_MS: u64 = 500;

// ============================================================================
// EFFICIENT PROGRESS TRACKING
// ============================================================================

/// In-memory progress state that gets periodically flushed to database
/// 
/// # Optimization Strategy
/// 1. All updates write to in-memory state (instant, no I/O)
/// 2. Background task flushes to DB at most every 500ms
/// 3. Coalesces multiple updates into single DB write
/// 4. Final state always flushed on completion
#[derive(Debug)]
struct ProgressState {
    progress: f64,
    current_generation: i32,
    total_generations: i32,
    phase: String,
    phase_details: serde_json::Value,
    dirty: bool,  // True if state has changed since last flush
}

impl Default for ProgressState {
    fn default() -> Self {
        Self {
            progress: 0.0,
            current_generation: 0,
            total_generations: 0,
            phase: "initializing".to_string(),
            phase_details: serde_json::json!({}),
            dirty: false,
        }
    }
}

/// Efficient progress tracker with batched database updates
/// 
/// # Usage
/// ```ignore
/// let tracker = ProgressTracker::new(job_id, job_queue);
/// tracker.start_background_flush(); // Spawns flush task
/// 
/// // Fast in-memory updates (no DB I/O)
/// tracker.set_progress(25.0).await;
/// tracker.set_generation(5, 20).await;
/// tracker.set_phase("genetic_optimization", json!({"best_fitness": 0.95})).await;
/// 
/// // Final flush on completion
/// tracker.flush_final().await;
/// ```
pub struct ProgressTracker {
    job_id: String,
    job_queue: Arc<dyn JobQueue>,
    state: Arc<RwLock<ProgressState>>,
    last_flush: Arc<RwLock<Instant>>,
}

impl ProgressTracker {
    pub fn new(job_id: String, job_queue: Arc<dyn JobQueue>) -> Self {
        Self {
            job_id,
            job_queue,
            state: Arc::new(RwLock::new(ProgressState::default())),
            last_flush: Arc::new(RwLock::new(Instant::now())),
        }
    }
    
    /// Set progress percentage (0-100) - in-memory only
    pub async fn set_progress(&self, progress: f64) {
        let mut state = self.state.write().await;
        if (state.progress - progress).abs() >= 0.1 {
            state.progress = progress;
            state.dirty = true;
        }
    }
    
    /// Set generation progress - in-memory only
    pub async fn set_generation(&self, current: i32, total: i32) {
        let mut state = self.state.write().await;
        if state.current_generation != current || state.total_generations != total {
            state.current_generation = current;
            state.total_generations = total;
            // Calculate progress from generation (0-85% for genetic phase)
            if total > 0 {
                state.progress = (current as f64 / total as f64) * 85.0;
            }
            state.dirty = true;
        }
    }
    
    /// Set current phase with details - in-memory only
    pub async fn set_phase(&self, phase: &str, details: serde_json::Value) {
        let mut state = self.state.write().await;
        state.phase = phase.to_string();
        state.phase_details = details;
        state.dirty = true;
    }
    
    /// Check if enough time has passed and state is dirty, then flush
    pub async fn maybe_flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let should_flush = {
            let last = self.last_flush.read().await;
            let state = self.state.read().await;
            state.dirty && last.elapsed() >= Duration::from_millis(PROGRESS_FLUSH_INTERVAL_MS)
        };
        
        if should_flush {
            self.flush().await?;
        }
        Ok(())
    }
    
    /// Force flush current state to database
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (progress, gen, total_gen, phase, details) = {
            let mut state = self.state.write().await;
            if !state.dirty {
                return Ok(());
            }
            state.dirty = false;
            (state.progress, state.current_generation, state.total_generations, 
             state.phase.clone(), state.phase_details.clone())
        };
        
        // Update last flush time
        {
            let mut last = self.last_flush.write().await;
            *last = Instant::now();
        }
        
        // Single combined update - more efficient than 3 separate calls
        // Using update_phase which includes all the info we need
        let combined_details = serde_json::json!({
            "progress": progress,
            "current_generation": gen,
            "total_generations": total_gen,
            "phase_info": details,
        });
        
        self.job_queue.update_phase(&self.job_id, &phase, combined_details).await?;
        self.job_queue.update_progress(&self.job_id, progress).await?;
        
        if gen > 0 {
            self.job_queue.update_generation(&self.job_id, gen, total_gen).await?;
        }
        
        Ok(())
    }
    
    /// Final flush - always writes to DB regardless of timing
    pub async fn flush_final(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Mark dirty to force flush
        {
            let mut state = self.state.write().await;
            state.dirty = true;
        }
        self.flush().await
    }
    
    /// Create a generation callback for genetic optimizer
    /// Returns a callback that updates in-memory state (fast)
    pub fn generation_callback(self: &Arc<Self>) -> Arc<dyn Fn(usize, usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync> {
        let tracker = Arc::clone(self);
        let job_id = self.job_id.clone();
        Arc::new(move |current: usize, total: usize| {
            let tracker = Arc::clone(&tracker);
            let job_id = job_id.clone();
            Box::pin(async move {
                // Log progress every 5 generations or on first/last
                if current == 1 || current == total || current % 5 == 0 {
                    log_info!(GENETIC_LOGGER, "Job {} generation {}/{} ({:.1}%)", 
                        job_id, current, total, (current as f64 / total as f64) * 100.0);
                }
                tracker.set_generation(current as i32, total as i32).await;
                // Try to flush if enough time has passed (non-blocking check)
                let _ = tracker.maybe_flush().await;
            })
        })
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
    order_size: f64,
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
}

impl Worker {
    pub fn new(id: usize, job_queue: Arc<dyn JobQueue>) -> Self {
        Self { id, job_queue }
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
                
                log_info!(MAIN_LOGGER, "Job {} storing results | pnl={:.2} sharpe={:.3} trades={}", 
                    job.id, result.total_pnl, result.sharpe_ratio, result.total_trades);
                
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
                
                log_info!(MAIN_LOGGER, "Job {} completed successfully | total_pnl={:.2} sharpe={:.3} max_dd={:.2}% trades={}", 
                    job.id, result.total_pnl, result.sharpe_ratio, result.max_drawdown * 100.0, result.total_trades);
                
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
            name: "Avellaneda-Stoikov".to_string(),
            strategy_type: "AvellanedaStoikov".to_string(),
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
        });
        
        // Convert Walk-Forward results if available
        let walk_forward = result.walk_forward.as_ref().map(|wf| WalkForwardSummary {
            periods_tested: wf.periods_tested,
            average_performance: wf.average_performance,
            consistency_score: wf.consistency_score,
            out_of_sample_degradation: wf.out_of_sample_degradation,
        });
        
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
            // Additional metrics
            annualized_return: None, // Calculated from equity curve
            volatility: result.core.volatility,
            sortino_ratio: None, // TODO: Add to core metrics
            calmar_ratio: None, // TODO: Add to core metrics
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
            monte_carlo,
            walk_forward,
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
        
        log_info!(GENETIC_LOGGER, "Job {} genetic optimization starting | pop={} gen={} symbol={}", 
            job.id, params.population_size, params.generations, params.symbol);
        
        // Create efficient progress tracker (batches DB writes)
        let progress_tracker = Arc::new(ProgressTracker::new(job.id.clone(), Arc::clone(&self.job_queue)));
        progress_tracker.set_phase("genetic_optimization", serde_json::json!({
            "population_size": params.population_size,
            "generations": params.generations,
        })).await;
        
        // Load market data once and share it
        log_info!(GENETIC_LOGGER, "Job {} loading market data for {} on {}", job.id, params.symbol, params.exchange);
        log_info!(GENETIC_LOGGER, "Job {} BEFORE calling load_market_data_for_strategy", job.id);
        
        // SYNC println to bypass async logging
        println!("[SYNC] Job {} about to call load_market_data_for_strategy with strategy_type={}", job.id, params.strategy_type);
        
        let data = match self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await {
            Ok(d) => {
                println!("[SYNC] Job {} load_market_data_for_strategy returned {} records", job.id, d.len());
                log_info!(GENETIC_LOGGER, "Job {} load_market_data_for_strategy returned {} records", job.id, d.len());
                d
            }
            Err(e) => {
                println!("[SYNC] Job {} load_market_data_for_strategy FAILED: {}", job.id, e);
                log_error!(GENETIC_LOGGER, "Job {} load_market_data_for_strategy FAILED: {}", job.id, e);
                return Err(e);
            }
        };
        
        if data.is_empty() {
            log_error!(GENETIC_LOGGER, "Job {} no market data available", job.id);
            return Err("No market data available".into());
        }
        
        log_info!(GENETIC_LOGGER, "Job {} loaded {} market data points", job.id, data.len());
        
        // For genetic optimization, use a subsample to speed up fitness evaluation
        // With true parallelism (spawn_blocking), we can use more samples
        // We'll run the full backtest on the winning chromosome after optimization
        const MAX_OPTIMIZATION_SAMPLES: usize = 5000;
        log_debug!(GENETIC_LOGGER, "Job {} starting subsampling of {} points", job.id, data.len());
        let optimization_data: Vec<MarketData> = if data.len() > MAX_OPTIMIZATION_SAMPLES {
            // Take evenly spaced samples to maintain temporal diversity
            let step = data.len() / MAX_OPTIMIZATION_SAMPLES;
            log_debug!(GENETIC_LOGGER, "Job {} subsampling with step={}", job.id, step);
            data.iter().step_by(step).take(MAX_OPTIMIZATION_SAMPLES).cloned().collect()
        } else {
            log_debug!(GENETIC_LOGGER, "Job {} using all data (no subsampling needed)", job.id);
            data.clone()
        };
        
        log_info!(GENETIC_LOGGER, "Job {} using {} samples for optimization (from {} total)", 
            job.id, optimization_data.len(), data.len());
        
        // Create genetic config
        log_debug!(GENETIC_LOGGER, "Job {} creating genetic config", job.id);
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
                
                // Create strategy with chromosome parameters
                let strategy_params = AvellanedaStoikovParams {
                    risk_aversion: chr.risk_aversion,
                    inventory_target: chr.inventory_target,
                    order_size: chr.order_size,
                    ..Default::default()
                };
                
                let mut strategy_params_map = HashMap::new();
                strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chr.inventory_target));
                strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(chr.order_size));
                
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
                        let sharpe = result.sharpe_ratio.unwrap_or(0.0);
                        let pnl = result.total_pnl.to_string().parse::<f64>().unwrap_or(0.0);
                        let fitness = sharpe + (pnl / initial_capital) * 0.1;
                        genetic::FitnessResult::new(fitness, result.equity_curve)
                    }
                    Err(_) => genetic::FitnessResult::failure(),
                }
            })
        });
        
        // Run genetic optimization with robustness-weighted fitness
        log_debug!(GENETIC_LOGGER, "Job {} creating GeneticOptimizer", job.id);
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: Some(generation_callback),
        };
        
        log_info!(GENETIC_LOGGER, "Job {} starting genetic algorithm with robustness-weighted fitness", job.id);
        let (best_chromosome, best_fitness) = optimizer.run().await;
        log_debug!(GENETIC_LOGGER, "Job {} optimizer.run() completed", job.id);
        
        // Flush progress after optimization completes
        progress_tracker.set_phase("post_optimization", serde_json::json!({
            "best_fitness": best_fitness,
            "risk_aversion": best_chromosome.risk_aversion,
            "inventory_target": best_chromosome.inventory_target,
        })).await;
        let _ = progress_tracker.flush().await;
        
        log_info!(GENETIC_LOGGER, "Job {} genetic optimization complete | best_fitness={:.4} risk_aversion={:.3} inventory_target={:.3} order_size={:.4}", 
            job.id, best_fitness, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size);
        
        // Update phase for final analysis
        progress_tracker.set_phase("final_analysis", serde_json::json!({
            "stage": "monte_carlo_walkforward"
        })).await;
        progress_tracker.set_progress(90.0).await;
        let _ = progress_tracker.flush().await;
        
        // Run final backtest with best parameters to get full results
        // This includes Monte Carlo and Walk-Forward analysis automatically
        log_info!(GENETIC_LOGGER, "Job {} running full analysis pipeline (Backtest → Monte Carlo → Walk-Forward)", job.id);
        log_debug!(MAIN_LOGGER, "🚀 About to run final backtest for job {}", job.id);
        
        let final_result = self.run_full_analysis_with_chromosome(
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
        
        // Final flush
        progress_tracker.set_progress(100.0).await;
        progress_tracker.set_phase("completed", serde_json::json!({
            "total_pnl": final_result.total_pnl,
            "sharpe_ratio": final_result.sharpe_ratio,
        })).await;
        let _ = progress_tracker.flush_final().await;
        
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
                        
                        let strategy_params = AvellanedaStoikovParams {
                            risk_aversion: chr.risk_aversion,
                            inventory_target: chr.inventory_target,
                            order_size: chr.order_size,
                            ..Default::default()
                        };
                        
                        let mut strategy_params_map = HashMap::new();
                        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chr.risk_aversion));
                        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chr.inventory_target));
                        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(chr.order_size));
                        
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
                                let sharpe = result.sharpe_ratio.unwrap_or(0.0);
                                let pnl = result.total_pnl.to_string().parse::<f64>().unwrap_or(0.0);
                                let fitness = sharpe + (pnl / initial_capital) * 0.1;
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
            };
            
            log_debug!(WALKFORWARD_LOGGER, "Job {} window {} starting genetic optimization", job.id, window_num);
            log_debug!(MAIN_LOGGER, "🧬 Window {}: Running genetic optimization...", window_num);
            let (best_chromosome, optimization_fitness) = optimizer.run().await;
            
            log_info!(WALKFORWARD_LOGGER, "Job {} window {} optimization complete | fitness={:.4} risk_aversion={:.3} inventory_target={:.3} order_size={:.4}",
                job.id, window_num, optimization_fitness, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size);
            log_debug!(MAIN_LOGGER, "✅ Window {}: Best params - risk_aversion={:.3}, inventory_target={:.3}, order_size={:.4}",
                window_num, best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size);
            
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
                    order_size: best_chromosome.order_size,
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
                        "order_size": w.optimal_params.order_size,
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
            monte_carlo: None, // Walk-forward uses aggregated OOS results
            walk_forward: None, // This IS the walk-forward result
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
            &windows.iter().map(|w| w.optimal_params.order_size).collect::<Vec<_>>()
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
        
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: chromosome.order_size,
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chromosome.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chromosome.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(chromosome.order_size));
        
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
        
        // Run full analysis (includes Monte Carlo and Walk-Forward)
        let result = engine.run_backtest(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            data,
        ).await?;
        
        // Log Monte Carlo results if available
        if let Some(ref mc) = result.monte_carlo {
            log_debug!(MAIN_LOGGER, "🎲 Monte Carlo ({} runs): mean_sharpe={:.3}, VaR95={:.2}%, CVaR95={:.2}%",
                mc.num_runs, mc.mean_metrics.sharpe_ratio, mc.var_95 * 100.0, mc.cvar_95 * 100.0);
        }
        
        // Log Walk-Forward results if available
        if let Some(ref wf) = result.walk_forward {
            log_debug!(MAIN_LOGGER, "🔄 Walk-Forward: periods={}, avg_perf={:.2}%, consistency={:.1}%, OOS_degradation={:.1}%",
                wf.periods_tested, wf.average_performance * 100.0, wf.consistency_score * 100.0, wf.out_of_sample_degradation * 100.0);
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
            monte_carlo: monte_carlo_summary,
            walk_forward: walk_forward_summary,
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
        
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size: chromosome.order_size,
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(chromosome.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(chromosome.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(chromosome.order_size));
        
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
                "order_size": chromosome.order_size
            })),
            monte_carlo: None, // Chromosome evaluation doesn't run MC
            walk_forward: None, // Chromosome evaluation doesn't run WF
        })
    }
    
    /// Load market data intelligently based on strategy type
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        // SYNC println to bypass async logging - appears immediately in container logs
        println!("[SYNC] ENTER load_market_data_for_strategy | type='{}' sym={} ex={}", 
            strategy_type, symbol, exchange);
        
        // Also log via async logger for Elasticsearch
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
                // Default to candles for other strategies
                self.load_market_data(symbol, exchange).await
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


