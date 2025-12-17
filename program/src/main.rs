use clap::{Parser, Subcommand};
use std::sync::Arc;

// Use the library's modules (they have access to macros properly)
use program::job_queue::{JobQueue, PostgresQueue};
use program::worker;
use program::api;
use program::distributed::DistributedWorker;
use program::distributed::ga_worker::DistributedGAWorker;

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
            
            // Spawn GA workers (for chromosome evaluation)
            let mut handles = Vec::with_capacity(workers * 2);
            for i in 0..workers {
                let worker_id = format!("{}-ga-{}", prefix, i);
                let broker_clone = Arc::clone(&broker);
                #[cfg(feature = "postgres")]
                let pool_clone = Arc::clone(&db_pool);
                
                let handle = tokio::spawn(async move {
                    #[cfg(feature = "postgres")]
                    let worker = DistributedGAWorker::new(worker_id, broker_clone)
                        .with_db_pool(pool_clone);
                    #[cfg(not(feature = "postgres"))]
                    let worker = DistributedGAWorker::new(worker_id, broker_clone);
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
            
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            println!("📦 Connecting to database...");
            let postgres_queue = PostgresQueue::new(&database_url)
                .map_err(|e| format!("Failed to connect: {}", e))?;
            let job_queue: Arc<dyn JobQueue> = Arc::new(postgres_queue);

            if distributed {
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
                
                // Note: Job processing is handled by the main 'worker' deployment
                // The coordinator only provides API + broker coordination
            } else {
                println!("📡 Coordinator mode with local processing");
            }

            // Build and start the API server with coordinator support
            let app = api::build_router(Arc::clone(&job_queue));
            let bind_addr = format!("{}:{}", host, port);
            let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
            
            println!("✅ Coordinator ready at http://{}", bind_addr);
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}