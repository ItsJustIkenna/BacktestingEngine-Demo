use clap::{Parser, Subcommand};
use std::sync::Arc;
use futures::FutureExt;

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
        #[arg(long, default_value_t = true)]
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
    /// Run backtest using Tardis CSV data (local file mode, no database needed for data)
    BacktestTardis {
        /// Trading symbol (e.g., "BTCUSD", "SOLUSD", "SUIUSD")
        #[arg(long)]
        symbol: String,
        
        /// Path to Tardis data directory containing {symbol}_trades.csv.gz files
        #[arg(long, default_value = "/data/tardis")]
        data_path: String,
        
        /// Initial capital in USD
        #[arg(long, default_value = "10000")]
        initial_capital: f64,
        
        /// Commission rate (e.g., 0.001 for 0.1%)
        #[arg(long, default_value = "0.001")]
        commission_rate: f64,
        
        /// Strategy type
        #[arg(long, default_value = "avellaneda_stoikov")]
        strategy: String,
        
        /// Enable genetic optimization
        #[arg(long, default_value = "false")]
        optimize: bool,
        
        /// Population size for genetic optimization
        #[arg(long, default_value = "30")]
        population_size: usize,
        
        /// Number of generations for genetic optimization
        #[arg(long, default_value = "15")]
        generations: usize,
        
        /// Output results to JSON file
        #[arg(long)]
        output: Option<String>,
    },
    /// Run momentum strategy backtest using Tardis CSV data
    BacktestMomentum {
        /// Trading symbol (e.g., "BTCUSD", "SOLUSD", "SUIUSD")
        #[arg(long)]
        symbol: String,
        
        /// Path to Tardis data directory containing {symbol}_trades.csv.gz files
        #[arg(long, default_value = "/data/tardis")]
        data_path: String,
        
        /// Initial capital in USD
        #[arg(long, default_value = "10000")]
        initial_capital: f64,
        
        /// Taker fee rate (e.g., 0.0016 for 0.16%)
        #[arg(long, default_value = "0.0016")]
        taker_fee: f64,
        
        /// Maker fee rate (e.g., 0.0016 for 0.16%)
        #[arg(long, default_value = "0.0016")]
        maker_fee: f64,
        
        /// Momentum threshold percentage to trigger entry (e.g., 0.003 for 0.3%)
        #[arg(long, default_value = "0.003")]
        momentum_threshold: f64,
        
        /// Momentum lookback window in milliseconds
        #[arg(long, default_value = "10000")]
        momentum_lookback_ms: i64,
        
        /// Take profit percentage (e.g., 0.015 for 1.5%)
        #[arg(long, default_value = "0.015")]
        take_profit_pct: f64,
        
        /// Stop loss percentage (e.g., 0.008 for 0.8%)
        #[arg(long, default_value = "0.008")]
        stop_loss_pct: f64,
        
        /// Position size as percentage of capital
        #[arg(long, default_value = "0.02")]
        position_size_pct: f64,
        
        /// Enable genetic optimization
        #[arg(long, default_value = "false")]
        optimize: bool,
        
        /// Population size for genetic optimization
        #[arg(long, default_value = "50")]
        population_size: usize,
        
        /// Number of generations for genetic optimization
        #[arg(long, default_value = "25")]
        generations: usize,
        
        /// Output results to JSON file
        #[arg(long)]
        output: Option<String>,
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
                let handle = tokio::spawn(async move {
                    println!("🔄 Starting job processing loop...");
                    eprintln!("🔄 [COORDINATOR] Job processing loop started (stderr)");
                    
                    // Log first poll immediately so we know the loop is running
                    eprintln!("🔍 [COORDINATOR] First poll starting NOW - ready to claim jobs");
                    
                    let mut poll_count = 0u64;
                    
                    // Catch panics in the job processing loop
                    let result = std::panic::AssertUnwindSafe(async {
                        loop {
                        poll_count += 1;
                        if poll_count % 30 == 1 {  // Log every minute (30 * 2s sleep)
                            eprintln!("🔍 [COORDINATOR] Polling for jobs... (poll #{})", poll_count);
                        }
                        
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
                                    initial_30d_volume: job.params.initial_30d_volume,
                                    initial_inventory_pct: job.params.initial_inventory_pct,
                                    cache_ttl_seconds: 14400, // 4 hours - enough for long GA runs + final evaluation
                                    // Simplified: TRAIN for GA, VALIDATE (remaining) for walk-forward
                                    train_ratio: job.params.train_ratio,
                                    validation_ratio: job.params.test_ratio + job.params.validate_ratio, // Combined
                                    fitness_weights: job.params.fitness_weights.clone(),
                                    monte_carlo_runs: job.params.monte_carlo_runs,
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
                                    
                                    // Track parsed generation for database update
                                    let mut parsed_gen: Option<(i32, i32)> = None;
                                    
                                    // Try to parse generation info (handles both "Generation 5/10:" and "Generation 5/10 complete")
                                    if det.starts_with("Generation ") {
                                        let gen_str = det.trim_start_matches("Generation ");
                                        // Find where the generation part ends (either ":" or " complete")
                                        let gen_part = gen_str.split(|c| c == ':' || c == ' ').next().unwrap_or("");
                                        if let Some((current, total)) = gen_part.split_once('/') {
                                            if let (Ok(cur), Ok(tot)) = (current.trim().parse::<i32>(), total.trim().parse::<i32>()) {
                                                phase_details["current_generation"] = serde_json::json!(cur);
                                                phase_details["total_generations"] = serde_json::json!(tot);
                                                parsed_gen = Some((cur, tot));
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
                                        
                                        // Also update the actual database fields for current_generation/total_generations
                                        // This ensures the UI shows "Gen X of Y" correctly
                                        if let Some((cur, tot)) = parsed_gen {
                                            let _ = jq.update_generation(&jid, cur, tot).await;
                                        }
                                    });
                                });
                                
                                // Transition to data_loading phase
                                let _ = job_queue_clone.update_phase(&job.id, "data_loading", serde_json::json!({
                                    "step": "requesting_workers",
                                    "symbol": &job.params.symbol,
                                    "exchange": &job.params.exchange
                                })).await;
                                let _ = job_queue_clone.update_progress(&job.id, 5.0).await;
                                
                                // First request workers to load data (also sets data splits from worker tick_count)
                                eprintln!("📤 [COORDINATOR] Requesting data load for job {}", job.id);
                                match coordinator.request_data_load().await {
                                    Ok(cache_key) => {
                                        eprintln!("✅ [COORDINATOR] Data loaded with cache_key: {}", cache_key);
                                        
                                        // Now run the distributed GA
                                        eprintln!("🚀 [COORDINATOR] Starting GA run at {:?}", chrono::Utc::now());
                                        match coordinator.run().await {
                                            Ok((best_chromosome, best_fitness)) => {
                                                eprintln!("✅ [COORDINATOR] Job {} GA completed at {:?}: fitness={:.4}", job.id, chrono::Utc::now(), best_fitness);
                                                eprintln!("🏆 [COORDINATOR] Best chromosome: risk_aversion={:.4}, order_size_pct={:.4}, inventory_target={:.4}",
                                                    best_chromosome.risk_aversion, best_chromosome.order_size_pct, best_chromosome.inventory_target);
                                                eprintln!("🔄 [COORDINATOR] Sending final evaluation request (MC={}, WF={})...",
                                                    job.params.monte_carlo_enabled, job.params.walk_forward_enabled);
                                                
                                                // Update generation to total/total before final analysis phase
                                                // This ensures UI shows "Generation 3 of 3" (or whatever the total is) during final analysis
                                                let total_gens = job.params.generations as i32;
                                                let _ = job_queue_clone.update_generation(&job.id, total_gens, total_gens).await;
                                                
                                                // Transition to final_analysis phase with proper details
                                                let _ = job_queue_clone.update_phase(&job.id, "final_analysis", serde_json::json!({
                                                    "step": "starting",
                                                    "monte_carlo_enabled": job.params.monte_carlo_enabled,
                                                    "walk_forward_enabled": job.params.walk_forward_enabled,
                                                    "best_fitness": best_fitness,
                                                    "current_generation": total_gens,
                                                    "total_generations": total_gens
                                                })).await;
                                                eprintln!("📊 [COORDINATOR] Transitioned to final_analysis phase (gen {}/{})", total_gens, total_gens);
                                                
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
                                                                    ..Default::default()
                                                                })
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Parse equity curve
                                                        let equity_curve: Vec<f64> = serde_json::from_slice(&eval_result.equity_curve)
                                                            .unwrap_or_default();
                                                        
                                                        // DEBUG: Log received equity curve for tracing
                                                        eprintln!("🔍 [COORDINATOR] RESULT_DEBUG: equity_curve.len()={}, net_pnl={:.2}, num_trades={}",
                                                            equity_curve.len(), eval_result.net_pnl, eval_result.num_trades);
                                                        if !equity_curve.is_empty() {
                                                            eprintln!("🔍 [COORDINATOR] RESULT_DEBUG: equity_curve first={:.2}, last={:.2}",
                                                                equity_curve.first().copied().unwrap_or(0.0),
                                                                equity_curve.last().copied().unwrap_or(0.0));
                                                            // Check for suspicious negative starting equity
                                                            if let Some(&first) = equity_curve.first() {
                                                                if first < 0.0 {
                                                                    eprintln!("🚨 [COORDINATOR] RESULT_ERROR: Equity curve starts NEGATIVE ({:.2})!", first);
                                                                    eprintln!("🚨 [COORDINATOR] RESULT_ERROR: This indicates stale binary or wrong code path!");
                                                                }
                                                            }
                                                        }
                                                        
                                                        // Parse closed trades data to get individual trade P&Ls
                                                        let trade_returns: Vec<f64> = serde_json::from_slice(&eval_result.closed_trades_data)
                                                            .unwrap_or_default();
                                                        
                                                        // Calculate best/worst trade from trade returns
                                                        let best_trade = trade_returns.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                                                        let worst_trade = trade_returns.iter().cloned().fold(f64::INFINITY, f64::min);
                                                        let best_trade = if best_trade.is_finite() { Some(best_trade) } else { None };
                                                        let worst_trade = if worst_trade.is_finite() { Some(worst_trade) } else { None };
                                                        
                                                        // Calculate Sortino ratio (only penalizes downside volatility)
                                                        let sortino_ratio = if equity_curve.len() > 1 {
                                                            let returns: Vec<f64> = equity_curve.windows(2)
                                                                .filter_map(|w| if w[0] > 0.0 { Some((w[1] - w[0]) / w[0]) } else { None })
                                                                .collect();
                                                            if !returns.is_empty() {
                                                                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                                                                let downside_variance: f64 = returns.iter()
                                                                    .filter(|&&r| r < 0.0)
                                                                    .map(|r| r.powi(2))
                                                                    .sum::<f64>() / returns.len() as f64;
                                                                let downside_std = downside_variance.sqrt();
                                                                if downside_std > 1e-10 {
                                                                    Some((mean / downside_std) * (252.0_f64).sqrt())
                                                                } else if mean > 0.0 {
                                                                    Some(10.0) // All positive returns
                                                                } else {
                                                                    Some(0.0)
                                                                }
                                                            } else {
                                                                None
                                                            }
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Calculate Calmar ratio (annualized return / max drawdown)
                                                        // First calculate annualized return and volatility
                                                        let (annualized_return, volatility) = if equity_curve.len() > 1 {
                                                            let returns: Vec<f64> = equity_curve.windows(2)
                                                                .filter_map(|w| if w[0] > 0.0 { Some((w[1] - w[0]) / w[0]) } else { None })
                                                                .collect();
                                                            if !returns.is_empty() {
                                                                let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
                                                                let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
                                                                let std = variance.sqrt();
                                                                // Annualize: assume ~100 samples per day (equity curve is sampled)
                                                                let samples_per_day = (returns.len() as f64 / 1.0).max(1.0); // Conservative estimate
                                                                let annualization = (252.0 * samples_per_day.min(100.0)).sqrt();
                                                                let ann_return = mean * 252.0 * samples_per_day.min(100.0);
                                                                let ann_vol = std * annualization;
                                                                (Some(ann_return), Some(ann_vol))
                                                            } else {
                                                                (None, None)
                                                            }
                                                        } else {
                                                            (None, None)
                                                        };
                                                        
                                                        let calmar_ratio = if let Some(ann_ret) = annualized_return {
                                                            if eval_result.max_drawdown > 0.001 {
                                                                Some(ann_ret / eval_result.max_drawdown)
                                                            } else if ann_ret > 0.0 {
                                                                Some(10.0)
                                                            } else {
                                                                Some(0.0)
                                                            }
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Calculate average trade return
                                                        let avg_trade_return = if !trade_returns.is_empty() {
                                                            Some(trade_returns.iter().sum::<f64>() / trade_returns.len() as f64)
                                                        } else if eval_result.num_trades > 0 {
                                                            Some(eval_result.net_pnl / eval_result.num_trades as f64)
                                                        } else {
                                                            None
                                                        };
                                                        
                                                        // Calculate VaR (95%) and Expected Shortfall from equity curve
                                                        let (value_at_risk_95, expected_shortfall_95) = if equity_curve.len() > 10 {
                                                            let mut dollar_returns: Vec<f64> = equity_curve.windows(2)
                                                                .map(|w| w[1] - w[0])
                                                                .collect();
                                                            dollar_returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                                            let var_index = (dollar_returns.len() as f64 * 0.05).floor() as usize;
                                                            let var_index = var_index.min(dollar_returns.len() - 1);
                                                            let var_95 = -dollar_returns[var_index]; // Positive loss amount
                                                            let tail_count = var_index.max(1);
                                                            let es_95: f64 = -dollar_returns[..tail_count].iter().sum::<f64>() / tail_count as f64;
                                                            (Some(var_95.max(0.0)), Some(es_95.max(0.0)))
                                                        } else {
                                                            (None, None)
                                                        };
                                                        
                                                        // Calculate current drawdown and average drawdown from equity curve
                                                        let (current_drawdown, avg_drawdown) = if equity_curve.len() > 1 {
                                                            let mut peak = equity_curve[0];
                                                            let mut dd_sum = 0.0;
                                                            let mut current_dd = 0.0;
                                                            for &equity in &equity_curve {
                                                                if equity > peak { peak = equity; }
                                                                let dd = if peak > 0.0 { (peak - equity) / peak } else { 0.0 };
                                                                dd_sum += dd;
                                                                current_dd = dd;
                                                            }
                                                            (Some(current_dd), Some(dd_sum / equity_curve.len() as f64))
                                                        } else {
                                                            (None, None)
                                                        };
                                                        
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
                                                                // Core AS parameters
                                                                "risk_aversion": best_chromosome.risk_aversion,
                                                                "order_size_pct": best_chromosome.order_size_pct,
                                                                "inventory_target": best_chromosome.inventory_target,
                                                                
                                                                // Timing parameters
                                                                "window_ms": best_chromosome.window_ms,
                                                                "min_quote_lifetime_ms": best_chromosome.min_quote_lifetime_ms,
                                                                "time_horizon_minutes": best_chromosome.time_horizon_minutes,
                                                                
                                                                // Spread and sizing parameters
                                                                "volatility_cap": best_chromosome.volatility_cap,
                                                                "market_depth_k": best_chromosome.market_depth_k,
                                                                "min_spread_bps": best_chromosome.min_spread_bps,
                                                                "fee_spread_multiplier": best_chromosome.fee_spread_multiplier,
                                                                
                                                                // Risk control parameters
                                                                "max_trades_per_minute": best_chromosome.max_trades_per_minute,
                                                                
                                                                // Dynamic adjustment parameters
                                                                "volatility_adjustment_factor": best_chromosome.volatility_adjustment_factor,
                                                                "inventory_penalty_factor": best_chromosome.inventory_penalty_factor,
                                                                "trend_sensitivity": best_chromosome.trend_sensitivity,
                                                                "flow_sensitivity": best_chromosome.flow_sensitivity,
                                                                "adverse_selection_sensitivity": best_chromosome.adverse_selection_sensitivity,
                                                                
                                                                // Exchange info
                                                                "exchange": job.params.exchange,
                                                                "symbol": job.params.symbol,
                                                                
                                                                // Optimization metadata
                                                                "fitness": best_fitness,
                                                                "optimization_source": "distributed_genetic_algorithm",
                                                            })),
                                                            start_date: job.params.start_date.clone(),
                                                            end_date: job.params.end_date.clone(),
                                                            closed_trades: Some(eval_result.closed_trades as usize),
                                                            open_positions: None,
                                                            realized_pnl: Some(eval_result.net_pnl),
                                                            unrealized_pnl: None,
                                                            annualized_return,
                                                            volatility,
                                                            sortino_ratio,
                                                            calmar_ratio,
                                                            avg_trade_return,
                                                            best_trade: if eval_result.best_trade_pnl != 0.0 { Some(eval_result.best_trade_pnl) } else { best_trade },
                                                            worst_trade: if eval_result.worst_trade_pnl != 0.0 { Some(eval_result.worst_trade_pnl) } else { worst_trade },
                                                            avg_time_in_trade: if eval_result.avg_time_in_trade_ms > 0.0 { Some(eval_result.avg_time_in_trade_ms / 1000.0) } else { None }, // Convert ms to seconds
                                                            value_at_risk_95,
                                                            expected_shortfall_95,
                                                            max_drawdown_duration_days: None, // Would need tracking in worker
                                                            current_drawdown,
                                                            avg_drawdown,
                                                            total_orders: Some(eval_result.total_orders),
                                                            filled_orders: Some(eval_result.num_trades),
                                                            cancelled_orders: None,
                                                            avg_slippage: None, // Market making - minimal slippage
                                                            total_commission_paid: if eval_result.total_fees > 0.0 { Some(eval_result.total_fees) } else { None },
                                                            monte_carlo,
                                                            walk_forward,
                                                            inventory_metrics: if eval_result.avg_inventory != 0.0 || eval_result.max_inventory != 0.0 {
                                                                Some(program::job_queue::InventorySummary {
                                                                    total_volume: eval_result.total_volume,
                                                                    avg_inventory_value: eval_result.avg_inventory,
                                                                    turnover_per_day: 0.0, // Not tracked separately
                                                                    buy_fills: 0, // Not tracked separately
                                                                    sell_fills: 0, // Not tracked separately
                                                                    buy_volume: eval_result.total_volume / 2.0, // Approximate
                                                                    sell_volume: eval_result.total_volume / 2.0, // Approximate
                                                                    imbalance_ratio: 0.0, // Not tracked
                                                                    ..Default::default()
                                                                })
                                                            } else { None },
                                                            execution_metrics: if eval_result.avg_fill_rate > 0.0 {
                                                                Some(program::job_queue::ExecutionSummary {
                                                                    orders_placed: eval_result.total_orders as usize,
                                                                    orders_filled: eval_result.num_trades as usize,
                                                                    orders_cancelled: 0,
                                                                    total_fills: eval_result.num_trades as usize,
                                                                    fill_rate: eval_result.avg_fill_rate,
                                                                    cancellation_rate: 0.0,
                                                                    avg_fills_per_order: 1.0,
                                                                    avg_time_to_fill_seconds: eval_result.avg_latency_ms / 1000.0,
                                                                    median_time_to_fill_seconds: eval_result.avg_latency_ms / 1000.0,
                                                                    bid_orders: 0,
                                                                    ask_orders: 0,
                                                                    bid_fills: 0,
                                                                    ask_fills: 0,
                                                                    bid_fill_rate: 0.0,
                                                                    ask_fill_rate: 0.0,
                                                                    avg_quote_spread_bps: 0.0,
                                                                    round_trip_win_rate: Some(eval_result.win_rate),
                                                                    total_round_trips: (eval_result.num_trades / 2) as usize,
                                                                    profitable_round_trips: ((eval_result.num_trades / 2) as f64 * eval_result.win_rate) as usize,
                                                                    spread_capture_rate: None,
                                                                    adverse_selection_rate: None,
                                                                    avg_pnl_per_round_trip: if eval_result.num_trades > 0 { Some(eval_result.avg_trade_pnl * 2.0) } else { None },
                                                                })
                                                            } else { None },
                                                            risk_halt_reason: None,
                                                            metrics_profile: None,
                                                            is_portfolio: false,
                                                            portfolio_metrics: None,
                                                            routing_metrics: None,
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
                                                                // Core AS parameters
                                                                "risk_aversion": best_chromosome.risk_aversion,
                                                                "order_size_pct": best_chromosome.order_size_pct,
                                                                "inventory_target": best_chromosome.inventory_target,
                                                                
                                                                // Timing parameters
                                                                "window_ms": best_chromosome.window_ms,
                                                                "min_quote_lifetime_ms": best_chromosome.min_quote_lifetime_ms,
                                                                "time_horizon_minutes": best_chromosome.time_horizon_minutes,
                                                                
                                                                // Spread and sizing parameters
                                                                "volatility_cap": best_chromosome.volatility_cap,
                                                                "market_depth_k": best_chromosome.market_depth_k,
                                                                "min_spread_bps": best_chromosome.min_spread_bps,
                                                                "fee_spread_multiplier": best_chromosome.fee_spread_multiplier,
                                                                
                                                                // Risk control parameters
                                                                "max_trades_per_minute": best_chromosome.max_trades_per_minute,
                                                                
                                                                // Dynamic adjustment parameters
                                                                "volatility_adjustment_factor": best_chromosome.volatility_adjustment_factor,
                                                                "inventory_penalty_factor": best_chromosome.inventory_penalty_factor,
                                                                "trend_sensitivity": best_chromosome.trend_sensitivity,
                                                                "flow_sensitivity": best_chromosome.flow_sensitivity,
                                                                "adverse_selection_sensitivity": best_chromosome.adverse_selection_sensitivity,
                                                                
                                                                // Exchange info
                                                                "exchange": job.params.exchange,
                                                                "symbol": job.params.symbol,
                                                                
                                                                // Optimization metadata
                                                                "fitness": best_fitness,
                                                                "optimization_source": "distributed_genetic_algorithm",
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
                                                            is_portfolio: false,
                                                            portfolio_metrics: None,
                                                            routing_metrics: None,
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
                    }).catch_unwind().await;
                    
                    match result {
                        Ok(_) => eprintln!("⚠️ [COORDINATOR] Job processing loop exited normally (unexpected)"),
                        Err(panic_info) => {
                            let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "Unknown panic".to_string()
                            };
                            eprintln!("💥 [COORDINATOR] Job processing loop PANICKED: {}", panic_msg);
                        }
                    }
                });
                
                // Monitor the job processing task
                tokio::spawn(async move {
                    if let Err(e) = handle.await {
                        eprintln!("💥 [COORDINATOR] Job processing task failed: {:?}", e);
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
            compute_features: _compute_features,
        } => {
            #[cfg(feature = "postgres")]
            {
                use diesel_async::pooled_connection::{AsyncDieselConnectionManager, deadpool::Pool};
                use diesel_async::AsyncPgConnection;
                use data_prep::{DataPrepConfig, prepare_orderbook_data};
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
                    symbol: symbol.clone(),
                    exchange: exchange.clone(),
                    output_dir: PathBuf::from(&output_dir),
                    chunk_size,
                    compute_features,
                    start_time: None,
                    end_time: None,
                };

                // Prepare unified binary file with ALL event types (New/Modify/Cancel/Trade)
                println!("\n📚 Preparing unified order book data (.bin)...");
                match prepare_orderbook_data(&pool, &config).await {
                    Ok(result) => {
                        println!("✅ Data preparation complete!");
                        println!("   Output file: {}", result.output_path.display());
                        println!("   Total events: {}", result.total_ticks);
                        println!("   File size: {} MB", result.file_size_bytes / 1_000_000);
                    }
                    Err(e) => {
                        eprintln!("❌ Data preparation failed: {}", e);
                        std::process::exit(1);
                    }
                }
                
                println!("\n🎉 Data preparation complete!");
            }
            
            #[cfg(not(feature = "postgres"))]
            {
                eprintln!("❌ PrepareData requires the 'postgres' feature");
                std::process::exit(1);
            }
        }
        
        Commands::BacktestTardis { 
            symbol, 
            data_path, 
            initial_capital, 
            commission_rate,
            strategy,
            optimize,
            population_size,
            generations,
            output,
        } => {
            use dataloader::tardis::TardisLoader;
            use dataloader::Tick;
            use backtest::realistic_simulation::{RealisticSimulator, RealisticASParams, OrderBookEvent, EventType, Side};
            use config::ExchangeFeeConfig;
            
            println!("📊 Tardis CSV Backtester");
            println!("   Symbol: {}", symbol);
            println!("   Data path: {}", data_path);
            println!("   Initial capital: ${}", initial_capital);
            println!("   Commission rate: {:.4}%", commission_rate * 100.0);
            println!("   Strategy: {}", strategy);
            println!("   Optimize: {}", optimize);
            
            // Suppress unused variable warnings for future use
            let _ = (population_size, generations);
            
            // Construct file paths - try multiple naming conventions
            let trades_path = format!("{}/{}_trades.csv.gz", data_path, symbol);
            
            // Try standard naming first, then Tardis book_snapshot_25 format
            let orderbook_candidates = [
                format!("{}/{}_orderbook.csv.gz", data_path, symbol),
                format!("{}/{}_book_snapshot_25.csv.gz", data_path, symbol),
            ];
            let orderbook_path = orderbook_candidates
                .iter()
                .find(|p| std::path::Path::new(p).exists())
                .cloned()
                .unwrap_or_else(|| orderbook_candidates[0].clone());
            
            println!("\n📂 Loading data files:");
            println!("   Trades: {}", trades_path);
            println!("   Orderbook: {}", orderbook_path);
            
            // Load trades data
            let start = std::time::Instant::now();
            let trades = match TardisLoader::load_trades(&trades_path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("❌ Failed to load trades: {}", e);
                    eprintln!("   Make sure the file exists: {}", trades_path);
                    std::process::exit(1);
                }
            };
            let trades_load_time = start.elapsed();
            println!("✅ Loaded {} trades in {:.2}s", trades.len(), trades_load_time.as_secs_f64());
            
            // Load orderbook snapshots
            let start = std::time::Instant::now();
            let orderbook_snapshots = match TardisLoader::load_orderbook(&orderbook_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("❌ Failed to load orderbook: {}", e);
                    eprintln!("   Make sure the file exists: {}", orderbook_path);
                    std::process::exit(1);
                }
            };
            let ob_load_time = start.elapsed();
            println!("✅ Loaded {} orderbook snapshots in {:.2}s", orderbook_snapshots.len(), ob_load_time.as_secs_f64());
            
            if trades.is_empty() {
                eprintln!("❌ No trades found in file");
                std::process::exit(1);
            }
            
            if orderbook_snapshots.is_empty() {
                eprintln!("❌ No orderbook snapshots found in file");
                std::process::exit(1);
            }
            
            // Show data range
            let first_trade = &trades[0];
            let last_trade = &trades[trades.len() - 1];
            let first_ob = &orderbook_snapshots[0];
            let last_ob = &orderbook_snapshots[orderbook_snapshots.len() - 1];
            
            println!("\n📊 Data Summary:");
            println!("   Trade date range: {} to {}", 
                first_trade.timestamp(), 
                last_trade.timestamp());
            println!("   Orderbook date range: {} to {}", 
                first_ob.timestamp(), 
                last_ob.timestamp());
            println!("   Trade price range: ${:.2} - ${:.2}",
                trades.iter().map(|t| t.price).fold(f64::MAX, f64::min),
                trades.iter().map(|t| t.price).fold(f64::MIN, f64::max));
            
            // Show orderbook quality metrics
            let bid = first_ob.best_bid();
            let ask = first_ob.best_ask();
            if bid > 0.0 && ask > 0.0 {
                println!("   First snapshot spread: {:.2} bps ({:.4} / {:.4})",
                    first_ob.spread_bps(), bid, ask);
            }
            println!("   Avg snapshots per trade: {:.1}", 
                orderbook_snapshots.len() as f64 / trades.len().max(1) as f64);
            
            // Convert orderbook snapshots + trades to unified OrderBookEvent stream
            println!("\n🔄 Converting to OrderBookEvent stream (diffing {} snapshots + {} trades)...",
                orderbook_snapshots.len(), trades.len());
            let convert_start = std::time::Instant::now();
            
            let tardis_events = TardisLoader::to_order_events(&orderbook_snapshots, &trades);
            
            // Convert TardisOrderEvent to OrderBookEvent (used by simulator)
            let events: Vec<OrderBookEvent> = tardis_events
                .into_iter()
                .map(|e| {
                    let event_type = match e.event_type {
                        dataloader::tardis::TardisEventType::New => EventType::New,
                        dataloader::tardis::TardisEventType::Modify => EventType::Modify,
                        dataloader::tardis::TardisEventType::Cancel => EventType::Cancel,
                        dataloader::tardis::TardisEventType::Trade => EventType::Trade,
                    };
                    let side = match e.side {
                        dataloader::tick::Side::Buy => Side::Buy,
                        dataloader::tick::Side::Sell => Side::Sell,
                    };
                    OrderBookEvent {
                        timestamp_ms: e.timestamp_ms,
                        event_type,
                        order_id: e.order_id,
                        side,
                        price: e.price,
                        quantity: e.quantity,
                    }
                })
                .collect();
            
            let convert_time = convert_start.elapsed();
            println!("✅ Generated {} events in {:.2}s", events.len(), convert_time.as_secs_f64());
            
            // Show event type breakdown
            let mut new_count = 0;
            let mut modify_count = 0;
            let mut cancel_count = 0;
            let mut trade_count = 0;
            for e in &events {
                match e.event_type {
                    EventType::New => new_count += 1,
                    EventType::Modify => modify_count += 1,
                    EventType::Cancel => cancel_count += 1,
                    EventType::Trade => trade_count += 1,
                }
            }
            println!("   NEW: {}, MODIFY: {}, CANCEL: {}, TRADE: {}",
                new_count, modify_count, cancel_count, trade_count);
            
            // Set up simulation
            println!("\n🎯 Running simulation...");
            let mut fee_config = ExchangeFeeConfig::binance_us();
            // Override with user-specified commission rate
            fee_config.maker_fee = commission_rate;
            fee_config.taker_fee = commission_rate;
            
            let simulator = RealisticSimulator::with_exchange_config(initial_capital, fee_config);
            
            // Default simulation parameters (can be optimized later)
            let params = RealisticASParams {
                risk_aversion: 0.5,
                inventory_target: 0.0,
                order_size_pct: 0.02,
                min_spread_bps: 10.0,
                volatility_cap: 0.05,
                time_horizon_minutes: 60.0,
                maker_fee_rate: commission_rate,
                taker_fee_rate: commission_rate,
                initial_inventory_pct: 0.0,
                ..Default::default()
            };
            
            let sim_start = std::time::Instant::now();
            let result = simulator.run(&events, &params);
            let sim_time = sim_start.elapsed();
            
            println!("\n📈 Backtest Results");
            println!("   ─────────────────────────────────");
            println!("   Simulation time: {:.2}s", sim_time.as_secs_f64());
            println!("   Total PnL: ${:.2}", result.total_pnl);
            println!("   Sharpe Ratio: {:.4}", result.sharpe_ratio);
            println!("   Max Drawdown: {:.2}%", result.max_drawdown * 100.0);
            println!("   Total Trades: {}", result.num_trades);
            println!("   Win Rate: {:.1}%", result.win_rate * 100.0);
            println!("   Profit Factor: {:.2}", result.profit_factor);
            println!("   ─────────────────────────────────");
            
            // Inventory metrics
            println!("\n📦 Inventory Metrics");
            println!("   Starting: ${:.2}", result.inventory_metrics.starting_inventory_value);
            println!("   Ending: ${:.2}", result.inventory_metrics.ending_inventory_value);
            println!("   Final Equity: ${:.2}", result.inventory_metrics.final_equity);
            
            // Execution metrics
            println!("\n⚡ Execution Metrics");
            println!("   Fill Rate: {:.2}%", result.execution_metrics.fill_rate * 100.0);
            println!("   Avg Latency: {:.2}ms", result.execution_metrics.avg_latency_ms);
            println!("   Avg Slippage: {:.2}bps", result.execution_metrics.avg_slippage_bps);
            
            // Save results to file if requested
            if let Some(output_path) = output {
                let output_json = serde_json::json!({
                    "symbol": symbol,
                    "exchange": "binanceus",
                    "data_source": "tardis",
                    "data_path": data_path,
                    "initial_capital": initial_capital,
                    "commission_rate": commission_rate,
                    "strategy": strategy,
                    "results": {
                        "total_pnl": result.total_pnl,
                        "sharpe_ratio": result.sharpe_ratio,
                        "max_drawdown": result.max_drawdown,
                        "num_trades": result.num_trades,
                        "win_rate": result.win_rate,
                        "profit_factor": result.profit_factor,
                        "final_equity": result.inventory_metrics.final_equity,
                    },
                    "execution_metrics": {
                        "fill_rate": result.execution_metrics.fill_rate,
                        "avg_latency_ms": result.execution_metrics.avg_latency_ms,
                        "avg_slippage_bps": result.execution_metrics.avg_slippage_bps,
                    },
                    "equity_curve": result.equity_curve,
                });
                
                match std::fs::write(&output_path, serde_json::to_string_pretty(&output_json).unwrap()) {
                    Ok(_) => println!("\n💾 Results saved to: {}", output_path),
                    Err(e) => eprintln!("\n⚠️ Failed to save results: {}", e),
                }
            }
            
            if optimize {
                println!("\n⚠️ Genetic optimization for Tardis data is not yet implemented in CLI mode.");
                println!("   Use the API with data_source='tardis' for optimization.");
            }
            
            println!("\n✅ Backtest complete!");
        }
        
        Commands::BacktestMomentum {
            symbol,
            data_path,
            initial_capital,
            taker_fee,
            maker_fee,
            momentum_threshold,
            momentum_lookback_ms,
            take_profit_pct,
            stop_loss_pct,
            position_size_pct,
            optimize,
            population_size,
            generations,
            output,
        } => {
            use dataloader::tardis::TardisLoader;
            use dataloader::Tick;
            use backtest::realistic_simulation::{OrderBookEvent, EventType, Side};
            use backtest::momentum_simulation::{MomentumSimulator, MomentumParams};
            use genetic::{MomentumChromosome, Chromosome};
            
            println!("🚀 Momentum Strategy Backtester");
            println!("   Symbol: {}", symbol);
            println!("   Data path: {}", data_path);
            println!("   Initial capital: ${:.2}", initial_capital);
            println!("   Fees: taker={:.2}bps, maker={:.2}bps", taker_fee * 10000.0, maker_fee * 10000.0);
            println!("   Momentum threshold: {:.2}%", momentum_threshold * 100.0);
            println!("   Take profit: {:.2}%, Stop loss: {:.2}%", take_profit_pct * 100.0, stop_loss_pct * 100.0);
            println!("   Optimize: {}", optimize);
            
            // Construct file path
            let trades_path = format!("{}/{}_trades.csv.gz", data_path, symbol);
            
            println!("\n📂 Loading trades data: {}", trades_path);
            
            // Load trades data
            let start = std::time::Instant::now();
            let trades = match TardisLoader::load_trades(&trades_path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("❌ Failed to load trades: {}", e);
                    eprintln!("   Make sure the file exists: {}", trades_path);
                    std::process::exit(1);
                }
            };
            let trades_load_time = start.elapsed();
            println!("✅ Loaded {} trades in {:.2}s", trades.len(), trades_load_time.as_secs_f64());
            
            if trades.is_empty() {
                eprintln!("❌ No trades found in file");
                std::process::exit(1);
            }
            
            // Show data range
            let first_trade = &trades[0];
            let last_trade = &trades[trades.len() - 1];
            
            println!("\n📊 Data Summary:");
            println!("   Date range: {} to {}", first_trade.timestamp(), last_trade.timestamp());
            println!("   Price range: ${:.2} - ${:.2}",
                trades.iter().map(|t| t.price).fold(f64::MAX, f64::min),
                trades.iter().map(|t| t.price).fold(f64::MIN, f64::max));
            
            // Convert trades to OrderBookEvent (momentum only needs trades, not full orderbook)
            println!("\n🔄 Converting {} trades to event stream...", trades.len());
            let events: Vec<OrderBookEvent> = trades
                .iter()
                .map(|t| {
                    let side = match t.side {
                        dataloader::tick::Side::Buy => Side::Buy,
                        dataloader::tick::Side::Sell => Side::Sell,
                    };
                    OrderBookEvent {
                        timestamp_ms: t.timestamp().timestamp_millis(),
                        event_type: EventType::Trade,
                        order_id: t.trade_id.to_string(),
                        side,
                        price: t.price,
                        quantity: t.quantity(),
                    }
                })
                .collect();
            
            println!("✅ Generated {} trade events", events.len());
            
            if optimize {
                // Genetic optimization mode
                println!("\n🧬 Starting Genetic Optimization");
                println!("   Population: {}", population_size);
                println!("   Generations: {}", generations);
                
                use rand::{SeedableRng, Rng};
                let mut rng = rand::rngs::StdRng::seed_from_u64(42);
                
                // Initialize population
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
                
                for gen in 0..generations {
                    let gen_start = std::time::Instant::now();
                    
                    // Evaluate population
                    let mut fitness_scores: Vec<(usize, f64)> = population
                        .iter()
                        .enumerate()
                        .map(|(i, chromo)| {
                            let result = simulator.run_chromosome(&events, chromo);
                            
                            // Comprehensive fitness function
                            let mut fitness = 0.0;
                            
                            // Base metrics (weighted)
                            fitness += result.sharpe_ratio * 0.20;
                            fitness += result.sortino_ratio * 0.15;
                            fitness += result.profit_factor.min(3.0) * 0.15;
                            fitness += result.win_rate * 0.10;
                            fitness += result.net_pnl_pct * 10.0 * 0.25; // Scale PnL %
                            fitness += result.calmar_ratio.min(5.0) * 0.15;
                            
                            // Penalties
                            if result.total_trades < 20 {
                                fitness *= 0.5; // Too few trades
                            }
                            if result.max_drawdown_pct > 0.15 {
                                fitness *= 0.7; // High drawdown
                            }
                            if result.net_pnl < 0.0 {
                                fitness *= 0.3; // Losing strategy
                            }
                            
                            (i, fitness)
                        })
                        .collect();
                    
                    // Sort by fitness (descending)
                    fitness_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    
                    let gen_best = fitness_scores[0].1;
                    if gen_best > best_fitness {
                        best_fitness = gen_best;
                        best_chromo = Some(population[fitness_scores[0].0].clone());
                    }
                    
                    let gen_time = gen_start.elapsed();
                    println!("   Gen {:3}/{}: best={:.4}, avg={:.4} ({:.1}s)",
                        gen + 1, generations, gen_best,
                        fitness_scores.iter().map(|(_, f)| f).sum::<f64>() / population_size as f64,
                        gen_time.as_secs_f64());
                    
                    // Selection + Crossover + Mutation
                    let elite_count = (population_size as f64 * 0.1).ceil() as usize;
                    let mut new_population: Vec<MomentumChromosome> = Vec::with_capacity(population_size);
                    
                    // Keep elites
                    for &(idx, _) in fitness_scores.iter().take(elite_count) {
                        new_population.push(population[idx].clone());
                    }
                    
                    // Tournament selection + crossover for rest
                    while new_population.len() < population_size {
                        // Tournament selection
                        let parent1_idx = fitness_scores[rng.gen_range(0..population_size / 2)].0;
                        let parent2_idx = fitness_scores[rng.gen_range(0..population_size / 2)].0;
                        
                        let mut child = population[parent1_idx].crossover(&population[parent2_idx], &mut rng);
                        child.mutate(&mut rng, 0.15); // 15% mutation rate
                        child.taker_fee_rate = taker_fee;
                        child.maker_fee_rate = maker_fee;
                        new_population.push(child);
                    }
                    
                    population = new_population;
                }
                
                // Final evaluation with best chromosome
                if let Some(best) = best_chromo {
                    println!("\n🏆 Best Chromosome Found:");
                    println!("   Momentum threshold: {:.3}%", best.momentum_threshold_pct * 100.0);
                    println!("   Momentum lookback: {}ms", best.momentum_lookback_ms);
                    println!("   Take profit: {:.2}%", best.take_profit_pct * 100.0);
                    println!("   Stop loss: {:.2}%", best.stop_loss_pct * 100.0);
                    println!("   Position size: {:.1}%", best.position_size_pct * 100.0);
                    println!("   Trailing stop: {}", if best.trailing_stop_enabled { "enabled" } else { "disabled" });
                    println!("   Volume spike threshold: {:.1}x", best.volume_spike_threshold);
                    println!("   Imbalance threshold: {:.2}", best.imbalance_threshold);
                    
                    let result = simulator.run_chromosome(&events, &best);
                    
                    println!("\n📈 Final Results:");
                    println!("   ─────────────────────────────────");
                    println!("   Net PnL: ${:.2} ({:.2}%)", result.net_pnl, result.net_pnl_pct * 100.0);
                    println!("   Sharpe Ratio: {:.4}", result.sharpe_ratio);
                    println!("   Sortino Ratio: {:.4}", result.sortino_ratio);
                    println!("   Max Drawdown: {:.2}%", result.max_drawdown_pct * 100.0);
                    println!("   Total Trades: {}", result.total_trades);
                    println!("   Win Rate: {:.1}%", result.win_rate * 100.0);
                    println!("   Profit Factor: {:.2}", result.profit_factor);
                    println!("   Total Fees: ${:.2}", result.total_fees);
                    println!("   ─────────────────────────────────");
                    
                    // Save to file if requested
                    if let Some(output_path) = output {
                        let output_json = serde_json::json!({
                            "symbol": symbol,
                            "strategy": "momentum",
                            "optimized": true,
                            "generations": generations,
                            "population_size": population_size,
                            "best_chromosome": {
                                "momentum_threshold_pct": best.momentum_threshold_pct,
                                "momentum_lookback_ms": best.momentum_lookback_ms,
                                "take_profit_pct": best.take_profit_pct,
                                "stop_loss_pct": best.stop_loss_pct,
                                "position_size_pct": best.position_size_pct,
                                "trailing_stop_enabled": best.trailing_stop_enabled,
                                "trailing_stop_pct": best.trailing_stop_pct,
                                "volume_spike_threshold": best.volume_spike_threshold,
                                "imbalance_threshold": best.imbalance_threshold,
                            },
                            "results": {
                                "net_pnl": result.net_pnl,
                                "net_pnl_pct": result.net_pnl_pct,
                                "sharpe_ratio": result.sharpe_ratio,
                                "sortino_ratio": result.sortino_ratio,
                                "max_drawdown_pct": result.max_drawdown_pct,
                                "total_trades": result.total_trades,
                                "win_rate": result.win_rate,
                                "profit_factor": result.profit_factor,
                                "total_fees": result.total_fees,
                            },
                        });
                        
                        match std::fs::write(&output_path, serde_json::to_string_pretty(&output_json).unwrap()) {
                            Ok(_) => println!("\n💾 Results saved to: {}", output_path),
                            Err(e) => eprintln!("\n⚠️ Failed to save results: {}", e),
                        }
                    }
                }
            } else {
                // Single run mode with specified parameters
                let params = MomentumParams {
                    momentum_threshold_pct: momentum_threshold,
                    momentum_lookback_ms,
                    take_profit_pct,
                    stop_loss_pct,
                    position_size_pct,
                    taker_fee_rate: taker_fee,
                    maker_fee_rate: maker_fee,
                    ..Default::default()
                };
                
                println!("\n🎯 Running single simulation...");
                let sim_start = std::time::Instant::now();
                let simulator = MomentumSimulator::new(initial_capital);
                let result = simulator.run(&events, &params);
                let sim_time = sim_start.elapsed();
                
                println!("\n📈 Backtest Results");
                println!("   ─────────────────────────────────");
                println!("   Simulation time: {:.2}s", sim_time.as_secs_f64());
                println!("   Net PnL: ${:.2} ({:.2}%)", result.net_pnl, result.net_pnl_pct * 100.0);
                println!("   Sharpe Ratio: {:.4}", result.sharpe_ratio);
                println!("   Sortino Ratio: {:.4}", result.sortino_ratio);
                println!("   Max Drawdown: {:.2}%", result.max_drawdown_pct * 100.0);
                println!("   Total Trades: {}", result.total_trades);
                println!("   Winning Trades: {}", result.winning_trades);
                println!("   Losing Trades: {}", result.losing_trades);
                println!("   Win Rate: {:.1}%", result.win_rate * 100.0);
                println!("   Profit Factor: {:.2}", result.profit_factor);
                println!("   ─────────────────────────────────");
                
                println!("\n💰 P&L Breakdown");
                println!("   Gross Profit: ${:.2}", result.gross_profit);
                println!("   Gross Loss: ${:.2}", result.gross_loss);
                println!("   Total Fees: ${:.2}", result.total_fees);
                println!("   Best Trade: ${:.2}", result.best_trade_pnl);
                println!("   Worst Trade: ${:.2}", result.worst_trade_pnl);
                println!("   Avg Trade: ${:.2}", result.avg_trade_pnl);
                
                println!("\n📊 Signal Statistics");
                println!("   Long Signals: {}", result.total_long_signals);
                println!("   Short Signals: {}", result.total_short_signals);
                println!("   Filtered by Volatility: {}", result.signals_filtered_by_volatility);
                println!("   Filtered by Regime: {}", result.signals_filtered_by_regime);
                
                // Save to file if requested
                if let Some(output_path) = output {
                    let output_json = serde_json::json!({
                        "symbol": symbol,
                        "strategy": "momentum",
                        "optimized": false,
                        "parameters": {
                            "momentum_threshold_pct": momentum_threshold,
                            "momentum_lookback_ms": momentum_lookback_ms,
                            "take_profit_pct": take_profit_pct,
                            "stop_loss_pct": stop_loss_pct,
                            "position_size_pct": position_size_pct,
                            "taker_fee": taker_fee,
                            "maker_fee": maker_fee,
                        },
                        "results": {
                            "net_pnl": result.net_pnl,
                            "net_pnl_pct": result.net_pnl_pct,
                            "sharpe_ratio": result.sharpe_ratio,
                            "sortino_ratio": result.sortino_ratio,
                            "max_drawdown_pct": result.max_drawdown_pct,
                            "total_trades": result.total_trades,
                            "winning_trades": result.winning_trades,
                            "losing_trades": result.losing_trades,
                            "win_rate": result.win_rate,
                            "profit_factor": result.profit_factor,
                            "gross_profit": result.gross_profit,
                            "gross_loss": result.gross_loss,
                            "total_fees": result.total_fees,
                        },
                        "signals": {
                            "total_long": result.total_long_signals,
                            "total_short": result.total_short_signals,
                            "filtered_volatility": result.signals_filtered_by_volatility,
                            "filtered_regime": result.signals_filtered_by_regime,
                        },
                    });
                    
                    match std::fs::write(&output_path, serde_json::to_string_pretty(&output_json).unwrap()) {
                        Ok(_) => println!("\n💾 Results saved to: {}", output_path),
                        Err(e) => eprintln!("\n⚠️ Failed to save results: {}", e),
                    }
                }
            }
            
            println!("\n✅ Momentum backtest complete!");
        }
    }

    Ok(())
}