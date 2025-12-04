use clap::{Parser, Subcommand};
use std::sync::Arc;

// Use the library's modules (they have access to macros properly)
use program::job_queue::{JobQueue, PostgresQueue};
use program::worker;
use program::api;

#[derive(Parser)]
#[command(name = "backtesting-engine")]
#[command(version, about = "High-performance backtesting and optimization engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the API server and worker
    Serve {
        /// Host to bind to
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Port to bind to
        #[arg(long, default_value = "8080")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

            // Spawn worker tasks
            for i in 0..4 {
                let queue = Arc::clone(&job_queue);
                tokio::spawn(async move {
                    let worker = worker::Worker::new(i, queue);
                    worker.run().await;
                });
            }

            // Build and start the API server
            let app = api::build_router(Arc::clone(&job_queue));
            let bind_addr = format!("{}:{}", host, port);
            let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
            
            println!("✅ Server ready at http://{}", bind_addr);
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

