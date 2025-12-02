//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};

use crate::job_queue::{BacktestJob, BacktestResult, JobQueue, JobStatus};
use crate::{log_info, log_error};
use crate::logging_facade::MAIN_LOGGER;

// Import existing backtest infrastructure
use backtest::BacktestEngine;
use strategy::{StrategyManager, StrategyConfig, StrategyRiskLimits};
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use riskmanager::RiskManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use config::BacktestConfig;
use dataloader::MarketData;

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
        println!("🚀 Worker {} started and polling for jobs", self.id);
        log_info!(MAIN_LOGGER, "Worker {} started", self.id);
        
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
                    log_error!(MAIN_LOGGER, "⚠️ Worker {} error: {}", self.id, e);
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
        
        log_info!(MAIN_LOGGER, "🎯 Worker {} claimed job: {} (symbol={}, exchange={})", 
            self.id, job.id, job.params.symbol, job.params.exchange);
        println!("🎯 Worker {} claimed job: {} (symbol={}, exchange={})", 
            self.id, job.id, job.params.symbol, job.params.exchange);
        
        // Note: get_next_job() already set status to "running" atomically in the database
        
        // Spawn heartbeat task to update job every 30 seconds
        let heartbeat_queue = self.job_queue.clone();
        let heartbeat_job_id = job.id.clone();
        let (heartbeat_tx, mut heartbeat_rx) = tokio::sync::mpsc::channel::<()>(1);
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        println!("💓 Heartbeat update for job {}", heartbeat_job_id);
                        log_info!(MAIN_LOGGER, "💓 Heartbeat update for job {}", heartbeat_job_id);
                        if let Err(e) = heartbeat_queue.update_heartbeat(&heartbeat_job_id).await {
                            eprintln!("⚠️ Failed to update heartbeat for job {}: {}", heartbeat_job_id, e);
                            log_error!(MAIN_LOGGER, "⚠️ Failed to update heartbeat for job {}: {}", heartbeat_job_id, e);
                            break; // Exit loop on persistent errors
                        }
                    }
                    _ = heartbeat_rx.recv() => {
                        log_info!(MAIN_LOGGER, "💓 Heartbeat task gracefully stopping for job {}", heartbeat_job_id);
                        break; // Graceful shutdown
                    }
                }
            }
        });
        
        // Execute backtest (genetic or manual)
        log_info!(MAIN_LOGGER, "🔀 Job {} optimization_method: '{}'", job.id, job.params.optimization_method);
        
        let result = match job.params.optimization_method.as_str() {
            "genetic" => {
                log_info!(MAIN_LOGGER, "🧬 Branching to genetic optimization for job {}", job.id);
                self.execute_genetic_optimization(&job).await
            },
            _ => {
                log_info!(MAIN_LOGGER, "📊 Branching to manual backtest for job {}", job.id);
                self.execute_backtest(&job).await
            },
        };
        
        let result = match result {
            Ok(result) => {
                // Gracefully stop heartbeat task and wait for it to complete
                let _ = heartbeat_tx.send(()).await;
                let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), heartbeat_handle).await;
                
                log_info!(MAIN_LOGGER, "💾 Storing results for job: {}", job.id);
                
                // Store results
                self.job_queue.store_result(&result).await?;
                
                // Update status to completed
                self.job_queue
                    .update_job_status(&job.id, JobStatus::Completed, None)
                    .await?;
                
                log_info!(
                    MAIN_LOGGER,
                    "✅ Worker {} completed job {} - Sharpe: {:.2}, PnL: ${:.2}, Trades: {}",
                    self.id,
                    job.id,
                    result.sharpe_ratio,
                    result.total_pnl,
                    result.total_trades
                );
                
                Ok(true)
            }
            Err(e) => {
                // Gracefully stop heartbeat task
                let _ = heartbeat_tx.send(()).await;
                let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), heartbeat_handle).await;
                
                let error_msg = format!("Backtest failed: {}", e);
                log_error!(MAIN_LOGGER, "❌ Worker {} job {} failed: {}", self.id, job.id, error_msg);
                
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
        
        log_info!(MAIN_LOGGER, "📂 Loading market data for job {}: symbol={}, exchange={}, strategy={}", 
            job.id, params.symbol, params.exchange, params.strategy_type);
        
        // Update progress: loading data
        self.job_queue.update_progress(&job.id, 10.0).await?;
        
        // Load market data intelligently based on strategy type
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
        if data.is_empty() {
            log_error!(MAIN_LOGGER, "❌ No market data available for job {}: symbol={}, exchange={}", 
                job.id, params.symbol, params.exchange);
            return Err("No market data available".into());
        }
        
        log_info!(MAIN_LOGGER, "✅ Loaded {} data points for job {}", data.len(), job.id);
        
        // Update progress: initializing
        self.job_queue.update_progress(&job.id, 20.0).await?;
        
        log_info!(MAIN_LOGGER, "⚙️ Initializing backtest engine for job {}: capital=${:.2}, commission={:.4}", 
            job.id, params.initial_capital, params.commission_rate);
        
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
        
        log_info!(MAIN_LOGGER, "📊 Registering strategy for job {}: gamma={:.4}, k={:.4}, order_size={:.4}", 
            job.id, params.risk_aversion, params.inventory_target, params.order_size);
        
        let mut strategy_manager = StrategyManager::new();
        strategy_manager
            .register_strategy(strategy_config.clone(), strategy)
            .map_err(|e| {
                log_error!(MAIN_LOGGER, "❌ Failed to register strategy for job {}: {}", job.id, e);
                format!("Failed to register strategy: {}", e)
            })?;
        
        // Update progress: running backtest
        self.job_queue.update_progress(&job.id, 30.0).await?;
        
        log_info!(MAIN_LOGGER, "🚀 Starting backtest execution for job {}", job.id);
        
        // Run backtest with progress updates
        let start_time = std::time::Instant::now();
        
        // Simulate progress during execution (temporary solution until engine supports callbacks)
        let job_id = job.id.clone();
        let queue = self.job_queue.clone();
        let progress_handle = tokio::spawn(async move {
            // Update progress from 30% to 85% over the backtest duration
            for progress in (35..=85).step_by(5) {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                let _ = queue.update_progress(&job_id, progress as f64).await;
            }
        });
        
        let result = engine
            .run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &data,
            )
            .await?;
        
        // Cancel progress updates once backtest is done
        progress_handle.abort();
        
        let execution_time = start_time.elapsed();
        
        log_info!(MAIN_LOGGER, "⏱️ Backtest execution completed for job {} in {:.2}s", 
            job.id, execution_time.as_secs_f64());
        
        // Update progress: calculating metrics
        self.job_queue.update_progress(&job.id, 90.0).await?;
        
        log_info!(MAIN_LOGGER, "📈 Calculating final metrics for job {}: trades={}, pnl=${:.2}", 
            job.id, result.core.num_trades, result.core.total_pnl);
        
        // Convert to API result format
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
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
        };
        
        Ok(backtest_result)
    }
    
    /// Execute genetic optimization to find best strategy parameters
    async fn execute_genetic_optimization(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        log_info!(MAIN_LOGGER, "🧬🧬🧬 ENTERED execute_genetic_optimization for job {}", job.id);
        
        use genetic::{GeneticOptimizer, ProgressCallback};
        use genetic::chromosome::AvellanedaStoikovChromosome;
        use config::GeneticConfig;
        
        let params = &job.params;
        
        log_info!(MAIN_LOGGER, "🧬 Starting genetic optimization for job {}: pop={}, gen={}", 
            job.id, params.population_size, params.generations);
        
        // Update phase: genetic optimization
        self.job_queue.update_phase(&job.id, "genetic_optimization", serde_json::json!({
            "population_size": params.population_size,
            "generations": params.generations,
            "current_generation": 0
        })).await?;
        
        // Update progress: loading data
        self.job_queue.update_progress(&job.id, 10.0).await?;
        
        // Load market data once and share it
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        if data.is_empty() {
            return Err("No market data available".into());
        }
        
        log_info!(MAIN_LOGGER, "✅ Loaded {} data points for genetic optimization", data.len());
        
        // Update progress: initializing
        self.job_queue.update_progress(&job.id, 15.0).await?;
        
        // Set total generations in database
        self.job_queue.update_generation(&job.id, 0, params.generations as i32).await?;
        
        // Create genetic config
        let config = GeneticConfig {
            population_size: params.population_size,
            generations: params.generations,
            mutation_rate: 0.1,
            crossover_rate: 0.7,
            ..Default::default() // Use defaults for MC lazy eval (disabled)
        };
        
        // Create progress callback to update generation tracking
        let job_queue = self.job_queue.clone();
        let job_id = job.id.clone();
        let total_gens = params.generations;
        let progress_callback: ProgressCallback = Arc::new(move |current_gen, _total| {
            let queue = job_queue.clone();
            let id = job_id.clone();
            Box::pin(async move {
                log_info!(MAIN_LOGGER, "📊 Progress callback fired: generation {}/{}", current_gen, total_gens);
                // Update generation in database
                if let Err(e) = queue.update_generation(&id, current_gen as i32, total_gens as i32).await {
                    log_error!(MAIN_LOGGER, "⚠️ Failed to update generation for job {}: {}", id, e);
                } else {
                    log_info!(MAIN_LOGGER, "✅ Generation updated successfully in database");
                }
                // Update progress (20% to 85% during optimization)
                let progress = 20.0 + (65.0 * current_gen as f64 / total_gens as f64);
                if let Err(e) = queue.update_progress(&id, progress).await {
                    log_error!(MAIN_LOGGER, "⚠️ Failed to update progress for job {}: {}", id, e);
                } else {
                    log_info!(MAIN_LOGGER, "✅ Progress updated to {:.1}%", progress);
                }
                log_info!(MAIN_LOGGER, "🧬 Job {} - Generation {}/{}", id, current_gen, total_gens);
            })
        });
        
        // Prepare data for fitness function (as Arc for sharing)
        let data_arc = Arc::new(data);
        let data_for_final = Arc::clone(&data_arc); // Clone for use after optimization
        let symbol = params.symbol.clone();
        let exchange = params.exchange.clone();
        let initial_capital = params.initial_capital;
        let commission_rate = params.commission_rate;
        
        // Create fitness function that evaluates chromosomes
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
                backtest_config.analysis.skip_genetic_optimization = true; // Skip genetic optimization in fitness evaluation
                backtest_config.analysis.auto_monte_carlo = false; // Skip Monte Carlo in fitness evaluation
                backtest_config.analysis.auto_walk_forward = false; // Skip Walk-Forward in fitness evaluation
                backtest_config.data.gap_detection.enabled = false; // Disable gap filling for fitness evaluation
                
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
                    return 0.0;
                }
                
                // Run ONLY core simulation (no Monte Carlo, no Walk-Forward)
                // Fitness evaluation should be fast and not trigger analysis loops
                match engine.run_core_simulation_direct(
                    &mut strategy_manager,
                    &mut orderbook,
                    &mut portfolio,
                    &mut risk_manager,
                    &data,
                ).await {
                    Ok(result) => {
                        // Fitness = Sharpe ratio (primary) + PnL bonus
                        let sharpe = result.sharpe_ratio.unwrap_or(0.0);
                        let pnl = result.total_pnl.to_string().parse::<f64>().unwrap_or(0.0);
                        sharpe + (pnl / initial_capital) * 0.1
                    }
                    Err(_) => 0.0,
                }
            })
        });
        
        // Run genetic optimization
        log_info!(MAIN_LOGGER, "🚀 Running genetic optimization...");
        log_info!(MAIN_LOGGER, "   Config: pop_size={}, generations={}, mutation={}, crossover={}", 
            config.population_size, config.generations, config.mutation_rate, config.crossover_rate);
        
        log_info!(MAIN_LOGGER, "📌 Creating optimizer with progress callback...");
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: Some(progress_callback),
        };
        log_info!(MAIN_LOGGER, "✅ Optimizer created successfully");
        
        log_info!(MAIN_LOGGER, "🔄 Calling optimizer.run()...");
        let (best_chromosome, best_fitness) = optimizer.run().await;
        log_info!(MAIN_LOGGER, "✅ optimizer.run() completed!");
        
        log_info!(MAIN_LOGGER, "🏆 Genetic optimization complete! Best fitness: {:.4}", best_fitness);
        log_info!(MAIN_LOGGER, "   Best params: γ={:.4}, q={:.4}, size={:.4}", 
            best_chromosome.risk_aversion, best_chromosome.inventory_target, best_chromosome.order_size);
        
        // Skip database updates here to avoid deadlock with heartbeat task
        // Heartbeat will be stopped after execute_genetic_optimization() returns
        // Progress is already at ~85% from genetic optimization callback
        
        // Note: Progress already at 90% from earlier update, skip redundant DB call
        // to avoid deadlock with heartbeat task
        
        // Run final backtest with best parameters to get full results
        log_info!(MAIN_LOGGER, "🚀 About to run final backtest for job {}", job.id);
        let final_result = self.run_backtest_with_chromosome(
            &job.id,
            &params.symbol,
            &params.exchange,
            params.initial_capital,
            params.commission_rate,
            &best_chromosome,
            &data_for_final,
        ).await?;
        
        log_info!(MAIN_LOGGER, "✅ Final backtest complete: Sharpe={:.2}, PnL=${:.2}", 
            final_result.sharpe_ratio, final_result.total_pnl);
        
        Ok(final_result)
    }
    
    /// Helper: Run backtest with specific chromosome parameters
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
        
        log_info!(MAIN_LOGGER, "🎯 About to call engine.run_backtest() for job {}", job_id);
        let result = engine.run_backtest(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            data,
        ).await?;
        
        log_info!(MAIN_LOGGER, "✅ engine.run_backtest() returned for job {}", job_id);
        log_info!(MAIN_LOGGER, "📊 Parsing results for job {}", job_id);
        
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
        log_info!(MAIN_LOGGER, "📦 Creating BacktestResult for job {}", job_id);
        
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
        })
    }
    
    /// Load market data intelligently based on strategy type
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        match strategy_type {
            "market_making" => {
                log_info!(MAIN_LOGGER, "📊 Loading order book data for market making strategy");
                // For market making, load order events to reconstruct order book
                self.load_order_events(symbol, exchange).await
            }
            _ => {
                // Default to candles for other strategies
                log_info!(MAIN_LOGGER, "📊 Loading candle data for strategy type: {}", strategy_type);
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
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        let loader = OrderBookBatchLoader::new(
            pool.clone(),
            symbol.to_string(),
            exchange.to_string(),
        );
        
        // Load with limit to prevent OOM during genetic optimization
        // 20K events is enough for meaningful backtesting while staying under memory limits
        const MAX_EVENTS: usize = 20000;
        
        log_info!(MAIN_LOGGER, "📊 Loading order book data for market making strategy (max {} events)", MAX_EVENTS);
        
        let orders = loader.load_with_limit(MAX_EVENTS).await?;
        
        log_info!(MAIN_LOGGER, "📊 Loaded {} order book events for {}/{}", orders.len(), symbol, exchange);
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
            use dataloader::Candle;
            
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
            let pool = self.job_queue.get_pool()
                .ok_or("Database pool not available")?;
            
            let mut conn = pool.get()
                .map_err(|e| format!("Failed to get database connection: {}", e))?;
            
            // Query candles_1m view for the symbol/exchange
            // Note: For now we load 1-minute candles. Could make timeframe configurable.
            let query = format!(
                "SELECT timestamp, symbol, exchange, open_price, high_price, low_price, \
                 close_price, volume, trade_count FROM candles_1m \
                 WHERE symbol = '{}' AND exchange = '{}' \
                 ORDER BY timestamp ASC LIMIT 10000",
                symbol, exchange
            );
            
            let results: Vec<CandleRow> = diesel::sql_query(&query)
                .load(&mut conn)
                .map_err(|e| format!("Failed to load candles: {}", e))?;
            
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
            
            log_info!(MAIN_LOGGER, "📊 Loaded {} candles for {}/{}", candles.len(), symbol, exchange);
            Ok(candles)
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = (symbol, exchange);
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
                .unwrap_or_else(|e| {
                    log_error!(MAIN_LOGGER, "Failed to create worker runtime: {}", e);
                    panic!("Cannot create worker runtime");
                });
            
            runtime.block_on(async {
                let worker = Worker::new(i, job_queue_clone);
                worker.run().await;
            });
        });
    }
    
    log_info!(MAIN_LOGGER, "Spawned {} workers", num_workers);
}
