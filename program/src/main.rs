use clap::{Parser, Subcommand};
use std::sync::Arc;

// Use the library's modules (they have access to macros properly)
use program::job_queue::{JobQueue, PostgresQueue};
use program::worker;
use program::api;
use program::distributed::DistributedWorker;
use program::distributed::ga_worker::DistributedGAWorker;

// Walk-forward analysis
use walkforward;

#[derive(Parser)]
#[command(name = "backtesting-engine")]
#[command(version, about = "High-performance backtesting and optimization engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the API server only (no workers)
    Serve {
        /// Host to bind to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Port to bind to
        #[arg(long, default_value = "8080")]
        port: u16,
    },
    /// Start background workers only (no API server) - polls database for jobs
    Worker {
        /// Number of worker threads
        #[arg(long, default_value = "4")]
        threads: usize,
    },
    /// Start distributed workers that process chunks via message broker
    DistributedWorker {
        /// Number of distributed workers to spawn
        #[arg(long, default_value = "4")]
        workers: usize,
        
        /// Worker ID prefix
        #[arg(long, default_value = "distributed")]
        prefix: String,
    },
    /// Start as coordinator for distributed backtesting
    Coordinator {
        /// Host to bind API to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Port to bind API to
        #[arg(long, default_value = "8080")]
        port: u16,
        
        /// Enable distributed mode (use message broker for job distribution)
        #[arg(long, default_value = "true")]
        distributed: bool,
    },
    /// Prepare historical data for backtesting (converts DB to binary format)
    PrepareData {
        /// Trading symbol (e.g., "BTC/USD")
        #[arg(long)]
        symbol: String,
        
        /// Exchange name (e.g., "Kraken")
        #[arg(long)]
        exchange: String,
        
        /// Output directory for binary files
        #[arg(long, default_value = "/data/historical")]
        output_dir: String,
        
        /// Number of rows per database chunk
        #[arg(long, default_value = "10000")]
        chunk_size: usize,
        
        /// Compute features (volatility, EMA, imbalance)
        #[arg(long, default_value = "true")]
        compute_features: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up panic hook to log panics before crashing
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };
        eprintln!("🔥 PANIC at {}: {}", location, message);
        // Also try to get backtrace
        eprintln!("Backtrace:\n{:?}", std::backtrace::Backtrace::capture());
    }));
    
    println!("🚀 Backtesting Engine starting...");

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { host, port } => {
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            println!("📦 Connecting to database...");
            let postgres_queue = PostgresQueue::new(&database_url)
                .map_err(|e| format!("Failed to connect: {}", e))?;
            let job_queue: Arc<dyn JobQueue> = Arc::new(postgres_queue);

            // API-only mode: no workers spawned here
            // Workers run in separate pods using the 'worker' command
            println!("📡 API-only mode (workers run in separate pods)");

            // Build and start the API server
            let app = api::build_router(Arc::clone(&job_queue));
            let bind_addr = format!("{}:{}", host, port);
            let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
            
            println!("✅ Server ready at http://{}", bind_addr);
            axum::serve(listener, app).await?;
        }
        
        Commands::Worker { threads } => {
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            println!("📦 Connecting to database...");
            let postgres_queue = PostgresQueue::new(&database_url)
                .map_err(|e| format!("Failed to connect: {}", e))?;
            let job_queue: Arc<dyn JobQueue> = Arc::new(postgres_queue);

            println!("👷 Starting {} worker threads...", threads);
            
            // Spawn worker tasks
            let mut handles = Vec::with_capacity(threads);
            for i in 0..threads {
                let queue = Arc::clone(&job_queue);
                let handle = tokio::spawn(async move {
                    let worker = worker::Worker::new(i, queue);
                    worker.run().await;
                });
                handles.push(handle);
            }
            
            println!("✅ Workers running");
            
            // Wait for all workers (they run forever)
            for handle in handles {
                let _ = handle.await;
            }
        }
        
        Commands::DistributedWorker { workers, prefix } => {
            use program::distributed::broker_client::{init_broker_client, BrokerRole};
            #[cfg(feature = "postgres")]
            use diesel_async::pooled_connection::{AsyncDieselConnectionManager, deadpool::Pool};
            #[cfg(feature = "postgres")]
            use diesel_async::AsyncPgConnection;
            
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            println!("🌐 Starting {} distributed workers (prefix: {})...", workers, prefix);
            
            // Create database pool for GA workers
            #[cfg(feature = "postgres")]
            let db_pool = {
                let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&database_url);
                Arc::new(Pool::builder(manager)
                    .max_size(workers * 2) // 2 connections per worker
                    .build()
                    .expect("Failed to create database pool"))
            };
            
            // Initialize broker connection
            let broker = init_broker_client().await
                .map_err(|e| format!("Failed to connect to broker: {:?}", e))?;
            
            // Start receiver for chromosome requests AND data load requests
            broker.start_receiver(BrokerRole::Worker).await
                .map_err(|e| format!("Failed to start broker receiver: {:?}", e))?;
            
            println!("📡 Connected to message broker, subscribed to topics");
            
            // Create shared state for all GA workers in this pod
            // This prevents multiple workers from loading the same data
            let shared_cache = std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
            let shared_loading_lock = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new()));
            
            // Spawn GA workers (for chromosome evaluation)
            let mut handles = Vec::with_capacity(workers * 2);
            for i in 0..workers {
                let worker_id = format!("{}-ga-{}", prefix, i);
                let broker_clone = Arc::clone(&broker);
                let cache_clone = Arc::clone(&shared_cache);
                let lock_clone = Arc::clone(&shared_loading_lock);
                #[cfg(feature = "postgres")]
                let pool_clone = Arc::clone(&db_pool);
                
                let handle = tokio::spawn(async move {
                    #[cfg(feature = "postgres")]
                    let worker = DistributedGAWorker::new_with_shared_state(
                        worker_id, 
                        broker_clone,
                        cache_clone,
                        lock_clone,
                    ).with_db_pool(pool_clone);
                    #[cfg(not(feature = "postgres"))]
                    let worker = DistributedGAWorker::new_with_shared_state(
                        worker_id,
                        broker_clone,
                        cache_clone,
                        lock_clone,
                    );
                    let _ = worker.run().await;
                });
                handles.push(handle);
            }
            
            // Spawn chunk workers (for distributed backtesting - legacy)
            for i in 0..workers {
                let worker_id = format!("{}-chunk-{}", prefix, i);
                let db_url = database_url.clone();
                let broker_clone = Arc::clone(&broker);
                
                let handle = tokio::spawn(async move {
                    let mut worker = DistributedWorker::with_broker(
                        worker_id,
                        db_url,
                        broker_clone,
                    );
                    worker.run().await;
                });
                handles.push(handle);
            }
            
            println!("✅ Distributed workers running");
            println!("   GA workers subscribe to: ga.chromosome.requests, ga.data.load");
            println!("   Chunk workers subscribe to: backtest.chunks");
            println!("   Results published to: ga.chromosome.results, backtest.results");
            
            // Wait for all workers
            for handle in handles {
                let _ = handle.await;
            }
        }
        
        Commands::Coordinator { host, port, distributed } => {
            use program::distributed::broker_client::{init_broker_client, BrokerRole};
            use program::distributed::ga_coordinator::{DistributedGACoordinator, DistributedGAConfig};
            use program::job_queue::JobStatus;
            
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            println!("📦 Connecting to database...");
            let postgres_queue = PostgresQueue::new(&database_url)
                .map_err(|e| format!("Failed to connect: {}", e))?;
            let job_queue: Arc<dyn JobQueue> = Arc::new(postgres_queue);

            let broker = if distributed {
                println!("🌐 Coordinator mode with distributed processing");
                
                // Initialize broker connection for distributed mode
                let broker = init_broker_client().await
                    .map_err(|e| format!("Failed to connect to broker: {:?}", e))?;
                
                // Start receiver for chromosome results (coordinator receives results from workers)
                broker.start_receiver(BrokerRole::Coordinator).await
                    .map_err(|e| format!("Failed to start broker receiver: {:?}", e))?;
                
                println!("📡 Connected to message broker, subscribed to chromosome results");
                println!("   Requests published to: ga.chromosome.requests");
                println!("   Results collected from: ga.chromosome.results");
                
                Some(broker)
            } else {
                println!("📡 Coordinator mode with local processing");
                None
            };

            // Spawn job processing loop
            if let Some(broker) = broker {
                let job_queue_clone = Arc::clone(&job_queue);
                tokio::spawn(async move {
                    println!("🔄 Starting job processing loop...");
                    loop {
                        // Poll for queued jobs
                        match job_queue_clone.get_next_job().await {
                            Ok(Some(job)) => {
                                eprintln!("📋 [COORDINATOR] Claimed job: {} (status now running)", job.id);
                                
                                // Create GA config from job params
                                let genetic_config = config::GeneticConfig {
                                    population_size: job.params.population_size,
                                    generations: job.params.generations,
                                    mutation_rate: 0.1,  // Default
                                    crossover_rate: 0.8, // Default
                                    use_monte_carlo_fitness: false,
                                    mc_sample_size: 50,
                                    top_percentile_threshold: 0.1,
                                };
                                
                                let config = DistributedGAConfig {
                                    genetic_config,
                                    symbol: job.params.symbol.clone(),
                                    exchange: job.params.exchange.clone(),
                                    start_time: job.params.start_date.clone().unwrap_or_default(),
                                    end_time: job.params.end_date.clone().unwrap_or_default(),
                                    initial_capital: job.params.initial_capital,
                                    cache_ttl_seconds: 14400, // 4 hours - enough for long GA runs + final evaluation
                                    // Data split ratios for train/test/validate
                                    train_ratio: job.params.train_ratio,
                                    test_ratio: job.params.test_ratio,
                                    validate_ratio: job.params.validate_ratio,
                                };
                                
                                let mut coordinator = DistributedGACoordinator::new(
                                    Arc::clone(&broker),
                                    config,
                                    job.id.clone(),
                                );
                                
                                // Set up granular progress callback
                                let job_queue_for_progress = Arc::clone(&job_queue_clone);
                                let job_id_for_progress = job.id.clone();
                                let total_generations = job.params.generations;
                                coordinator.set_progress_callback(move |progress, detail| {
                                    let jq = Arc::clone(&job_queue_for_progress);
                                    let jid = job_id_for_progress.clone();
                                    let det = detail.clone();
                                    
                                    // Parse detail to extract structured info
                                    // Examples: 
                                    //   "Generation 5/10: evaluating chromosomes"
                                    //   "Generation 5/10: 25/50 evaluated"
                                    //   "Generation 5/10 complete, best_fitness=0.1234"
                                    let mut phase_details = serde_json::json!({
                                        "detail": &det,
                                        "total_generations": total_generations
                                    });
                                    
                                    // Try to parse generation info (handles both "Generation 5/10:" and "Generation 5/10 complete")
                                    if det.starts_with("Generation ") {
                                        let gen_str = det.trim_start_matches("Generation ");
                                        // Find where the generation part ends (either ":" or " complete")
                                        let gen_part = gen_str.split(|c| c == ':' || c == ' ').next().unwrap_or("");
                                        if let Some((current, total)) = gen_part.split_once('/') {
                                            if let (Ok(cur), Ok(tot)) = (current.trim().parse::<i32>(), total.trim().parse::<i32>()) {
                                                phase_details["current_generation"] = serde_json::json!(cur);
                                                phase_details["total_generations"] = serde_json::json!(tot);
                                            }
                                        }
                                    }
                                    
                                    // Try to parse best_fitness from completion message
                                    if det.contains("best_fitness=") {
                                        if let Some(fitness_str) = det.split("best_fitness=").nth(1) {
                                            if let Some(fitness_val) = fitness_str.split_whitespace().next() {
                                                if let Ok(fitness) = fitness_val.parse::<f64>() {
                                                    phase_details["best_fitness"] = serde_json::json!(fitness);
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Try to parse chromosome progress
                                    if det.contains("evaluated") && det.contains('/') {
                                        if let Some(eval_part) = det.split(": ").nth(1) {
                                            if let Some((cur, rest)) = eval_part.split_once('/') {
                                                if let Some(tot) = rest.split_whitespace().next() {
                                                    if let (Ok(cur_val), Ok(tot_val)) = (cur.trim().parse::<i32>(), tot.trim().parse::<i32>()) {
                                                        phase_details["chromosomes_evaluated"] = serde_json::json!(cur_val);
                                                        phase_details["total_chromosomes"] = serde_json::json!(tot_val);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Set stage based on message content
                                    if det.contains("evaluating") {
                                        phase_details["stage"] = serde_json::json!("Evaluating");
                                    } else if det.contains("complete") {
                                        phase_details["stage"] = serde_json::json!("Generation Complete");
                                    }
                                    
                                    // Spawn a task to update progress (non-blocking)
                                    tokio::spawn(async move {
                                        let _ = jq.update_progress(&jid, progress).await;
                                        let _ = jq.update_phase(&jid, "genetic_optimization", phase_details).await;
                                    });
                                });
                                
                                // First request workers to load data
                                eprintln!("📤 [COORDINATOR] Requesting data load for job {}", job.id);
                                match coordinator.request_data_load().await {
                                    Ok(cache_key) => {
                                        eprintln!("✅ [COORDINATOR] Data loaded with cache_key: {}", cache_key);
                                        
                                        // Set up data splits for train/test/validate
                                        if let Some(tick_count) = coordinator.probe_tick_count() {
                                            coordinator.set_data_splits(tick_count);
                                            eprintln!("📊 [COORDINATOR] Data split: {} ticks, train=60%, test=20%, validate=20%", tick_count);
                                        } else {
                                            eprintln!("⚠️ [COORDINATOR] Could not probe tick count - using full data for all phases");
                                        }
                                        
                                        // Now run the distributed GA
                                        match coordinator.run().await {
                                            Ok((best_chromosome, best_fitness)) => {
                                                eprintln!("✅ [COORDINATOR] Job {} GA completed: fitness={:.4}", job.id, best_fitness);
                                                eprintln!("� [COORDINATOR] Best chromosome: risk_aversion={:.4}, order_size_pct={:.4}, inventory_target={:.4}",
                                                    best_chromosome.risk_aversion, best_chromosome.order_size_pct, best_chromosome.inventory_target);
                                                eprintln!("🔄 [COORDINATOR] Sending final evaluation request (MC={}, WF={})...",
                                                    job.params.monte_carlo_enabled, job.params.walk_forward_enabled);
                                                
                                                let final_eval_start = std::time::Instant::now();
                                                
                                                // Run final evaluation to get real metrics
                                                let final_result = coordinator.run_final_evaluation(
                                                    &best_chromosome,
                                                    job.params.monte_carlo_enabled,
                                                    job.params.walk_forward_enabled,
                                                ).await;
                                                
                                                eprintln!("⏱️ [COORDINATOR] Final evaluation took {:.2}s", final_eval_start.elapsed().as_secs_f64());
                                                
                                                let result = match final_result {
                                                    Ok(eval_result) => {
                                                        eprintln!("✅ [COORDINATOR] Final evaluation complete: PnL={:.2}, Sharpe={:.4}, trades={}",
                                                            eval_result.net_pnl, eval_result.sharpe_ratio, eval_result.num_trades);
                                                        
                                                        // Parse Monte Carlo results if present - convert to MonteCarloSummary
                                                        let monte_carlo: Option<program::job_queue::MonteCarloSummary> = if !eval_result.monte_carlo_results.is_empty() {
                                                            serde_json::from_slice::<backtest::monte_carlo::MonteCarloResult>(&eval_result.monte_carlo_results)
                                                                .ok()
                                                                .map(|mc| program::job_queue::MonteCarloSummary {
                                                                    num_runs: mc.num_runs,
                                                                    mean_sharpe: mc.mean_metrics.sharpe_ratio,
                                                                    stddev_sharpe: mc.stddev_metrics.sharpe_ratio,
                                                                    var_95: mc.var_95,
                                                                    cvar_95: mc.cvar_95,
                                                                    confidence_lower: mc.percentile_metrics.get(&5).map(|m| m.sharpe_ratio).unwrap_or(0.0),
                                                                    confidence_upper: mc.percentile_metrics.get(&95).map(|m| m.sharpe_ratio).unwrap_or(0.0),
                                                                    data_points: mc.data_points,
                                                                    var_unreliable: mc.var_unreliable,
                                                                    p5_total_pnl: None,
                                                                    p50_total_pnl: None,
                                                                    p95_total_pnl: None,
                                                                    p5_max_drawdown: None,
                                                                    p50_max_drawdown: None,
                                                                    p95_max_drawdown: None,
                                                                    p50_sharpe: None,
                                                                    p5_win_rate: None,
                                                                    p50_win_rate: None,
                                                                    p95_win_rate: None,
                                                                })
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Parse Walk-Forward results if present - convert to job_queue WalkForwardSummary
                                                        let walk_forward: Option<program::job_queue::WalkForwardSummary> = if !eval_result.walk_forward_results.is_empty() {
                                                            serde_json::from_slice::<walkforward::WalkForwardSummary>(&eval_result.walk_forward_results)
                                                                .ok()
                                                                .map(|wf| program::job_queue::WalkForwardSummary {
                                                                    periods_tested: wf.num_windows,
                                                                    average_performance: wf.average_test_sharpe,
                                                                    consistency_score: wf.consistency_score,
                                                                    out_of_sample_degradation: 1.0 - wf.out_of_sample_performance_ratio,
                                                                })
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Parse equity curve
                                                        let equity_curve: Vec<f64> = serde_json::from_slice(&eval_result.equity_curve)
                                                            .unwrap_or_default();
                                                        
                                                        program::job_queue::BacktestResult {
                                                            job_id: job.id.clone(),
                                                            strategy_type: Some(job.params.strategy_type.clone()),
                                                            total_pnl: eval_result.net_pnl,
                                                            sharpe_ratio: eval_result.sharpe_ratio,
                                                            max_drawdown: eval_result.max_drawdown,
                                                            win_rate: eval_result.win_rate,
                                                            total_trades: eval_result.num_trades as usize,
                                                            profit_factor: eval_result.profit_factor,
                                                            equity_curve,
                                                            completed_at: chrono::Utc::now(),
                                                            strategy_params: Some(serde_json::json!({
                                                                "risk_aversion": best_chromosome.risk_aversion,
                                                                "order_size_pct": best_chromosome.order_size_pct,
                                                                "window_ms": best_chromosome.window_ms,
                                                                "inventory_target": best_chromosome.inventory_target,
                                                                "time_horizon_minutes": best_chromosome.time_horizon_minutes,
                                                                "market_depth_k": best_chromosome.market_depth_k,
                                                                "take_profit_pct": best_chromosome.take_profit_pct,
                                                                "stop_loss_pct": best_chromosome.stop_loss_pct,
                                                                "fitness": best_fitness,
                                                            })),
                                                            start_date: job.params.start_date.clone(),
                                                            end_date: job.params.end_date.clone(),
                                                            closed_trades: Some(eval_result.closed_trades as usize),
                                                            open_positions: None,
                                                            realized_pnl: Some(eval_result.net_pnl),
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
                                                            total_orders: Some(eval_result.total_orders),
                                                            filled_orders: Some(eval_result.num_trades),
                                                            cancelled_orders: None,
                                                            avg_slippage: None,
                                                            total_commission_paid: None,
                                                            monte_carlo,
                                                            walk_forward,
                                                            inventory_metrics: None,
                                                            execution_metrics: None,
                                                            risk_halt_reason: None,
                                                            metrics_profile: None,
                                                        }
                                                    }
                                                    Err(e) => {
                                                        eprintln!("❌ [COORDINATOR] Final evaluation FAILED: {:?}", e);
                                                        eprintln!("❌ [COORDINATOR] Using fallback result with fitness={:.4}, initial_capital={}", 
                                                            best_fitness, job.params.initial_capital);
                                                        // Fallback to basic result if final eval fails
                                                        program::job_queue::BacktestResult {
                                                            job_id: job.id.clone(),
                                                            strategy_type: Some(job.params.strategy_type.clone()),
                                                            total_pnl: best_fitness * job.params.initial_capital,
                                                            sharpe_ratio: best_fitness,
                                                            max_drawdown: 0.0,
                                                            win_rate: 0.0,
                                                            total_trades: 0,
                                                            profit_factor: 1.0,
                                                            equity_curve: vec![],
                                                            completed_at: chrono::Utc::now(),
                                                            strategy_params: Some(serde_json::json!({
                                                                "risk_aversion": best_chromosome.risk_aversion,
                                                                "order_size_pct": best_chromosome.order_size_pct,
                                                                "window_ms": best_chromosome.window_ms,
                                                                "inventory_target": best_chromosome.inventory_target,
                                                                "time_horizon_minutes": best_chromosome.time_horizon_minutes,
                                                                "market_depth_k": best_chromosome.market_depth_k,
                                                                "take_profit_pct": best_chromosome.take_profit_pct,
                                                                "stop_loss_pct": best_chromosome.stop_loss_pct,
                                                                "fitness": best_fitness,
                                                                "final_eval_error": format!("{:?}", e),
                                                            })),
                                                            start_date: job.params.start_date.clone(),
                                                            end_date: job.params.end_date.clone(),
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
                                                            total_orders: None,
                                                            filled_orders: None,
                                                            cancelled_orders: None,
                                                            avg_slippage: None,
                                                            total_commission_paid: None,
                                                            monte_carlo: None,
                                                            walk_forward: None,
                                                            inventory_metrics: None,
                                                            execution_metrics: None,
                                                            risk_halt_reason: None,
                                                            metrics_profile: None,
                                                        }
                                                    }
                                                };
                                                
                                                // Save result
                                                if let Err(e) = job_queue_clone.store_result(&result).await {
                                                    eprintln!("❌ [COORDINATOR] Failed to save result: {}", e);
                                                }
                                                
                                                // Update status to completed
                                                if let Err(e) = job_queue_clone.update_job_status(&job.id, JobStatus::Completed, None).await {
                                                    eprintln!("❌ [COORDINATOR] Failed to update status: {}", e);
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("❌ [COORDINATOR] Job {} GA failed: {:?}", job.id, e);
                                                let _ = job_queue_clone.update_job_status(
                                                    &job.id, 
                                                    JobStatus::Failed, 
                                                    Some(format!("GA error: {:?}", e))
                                                ).await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("❌ [COORDINATOR] Job {} data load failed: {:?}", job.id, e);
                                        let _ = job_queue_clone.update_job_status(
                                            &job.id, 
                                            JobStatus::Failed, 
                                            Some(format!("Data load error: {:?}", e))
                                        ).await;
                                    }
                                }
                            }
                            Ok(None) => {
                                // No jobs available, wait before polling again
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            }
                            Err(e) => {
                                eprintln!("❌ [COORDINATOR] Error polling for jobs: {}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            }
                        }
                    }
                });
            }

            // Build and start the API server with coordinator support
            let app = api::build_router(Arc::clone(&job_queue));
            let bind_addr = format!("{}:{}", host, port);
            let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
            
            println!("✅ Coordinator ready at http://{}", bind_addr);
            axum::serve(listener, app).await?;
        }
        
        Commands::PrepareData { 
            symbol: _symbol, 
            exchange: _exchange, 
            output_dir: _output_dir, 
            chunk_size: _chunk_size, 
            compute_features: _compute_features 
        } => {
            #[cfg(feature = "postgres")]
            {
                use diesel_async::pooled_connection::{AsyncDieselConnectionManager, deadpool::Pool};
                use diesel_async::AsyncPgConnection;
                use data_prep::{DataPrepConfig, prepare_data};
                use std::path::PathBuf;
                
                // Re-bind variables for use inside postgres block
                let symbol = _symbol;
                let exchange = _exchange;
                let output_dir = _output_dir;
                let chunk_size = _chunk_size;
                let compute_features = _compute_features;
                
                let database_url = std::env::var("DATABASE_URL")
                    .expect("DATABASE_URL must be set");

                println!("📊 Preparing data for backtesting...");
                println!("   Symbol: {}", symbol);
                println!("   Exchange: {}", exchange);
                println!("   Output: {}", output_dir);
                println!("   Chunk size: {}", chunk_size);
                println!("   Compute features: {}", compute_features);

                // Create database pool
                let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(&database_url);
                let pool = Pool::builder(manager)
                    .max_size(4)
                    .build()
                    .expect("Failed to create database pool");

                let config = DataPrepConfig {
                    symbol,
                    exchange,
                    output_dir: PathBuf::from(output_dir),
                    chunk_size,
                    compute_features,
                    start_time: None,
                    end_time: None,
                };

                match prepare_data(&pool, &config).await {
                    Ok(result) => {
                        println!("✅ Data preparation complete!");
                        println!("   Output file: {}", result.output_path.display());
                        println!("   Total ticks: {}", result.total_ticks);
                        println!("   File size: {} MB", result.file_size_bytes / 1_000_000);
                    }
                    Err(e) => {
                        eprintln!("❌ Data preparation failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            
            #[cfg(not(feature = "postgres"))]
            {
                eprintln!("❌ PrepareData requires the 'postgres' feature");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}