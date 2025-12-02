use clap::{Parser, Subcommand};

mod api;
mod job_queue;
mod streaming;
mod worker;

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
    ultra_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { host, port } => {
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            let postgres_pool = job_queue::postgres::PostgresQueue::new(&database_url).await?;

            // Spawn the worker task
            let worker_pool = postgres_pool.clone();
            tokio::spawn(async move {
                if let Err(e) = worker::run(worker_pool).await {
                    eprintln!("Worker error: {:?}", e);
                }
            });

            // Start the API server
            println!("Starting server on {}:{}", host, port);
            api::serve(&host, port, postgres_pool).await?;
        }
    }

    Ok(())
}