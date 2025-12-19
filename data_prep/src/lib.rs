//! Data Preparation Library for Backtesting
//!
//! This crate provides the "librarian" functionality:
//! 1. Stream historical data from PostgreSQL in chunks
//! 2. Convert heavy HistoricalOrder structs to lightweight SimulationTick
//! 3. Optionally precompute features (volatility, EMA, etc.)
//! 4. Write to memory-mappable binary files
//!
//! # Architecture
//!
//! ```text
//!                    ┌─────────────────────────────────────────┐
//!                    │          Data Prep Job                  │
//!                    │                                         │
//!                    │  ┌─────────────┐    ┌───────────────┐  │
//!   PostgreSQL ─────►│  │ DB Streamer │───►│ Binary Writer │──┼──► .bin file
//!   (7M rows)        │  │ (10K chunks)│    │               │  │    (280 MB)
//!                    │  └─────────────┘    └───────────────┘  │
//!                    │         │                              │
//!                    │         ▼                              │
//!                    │  ┌─────────────┐                       │
//!                    │  │  Feature    │ (optional)            │
//!                    │  │  Computer   │                       │
//!                    │  └─────────────┘                       │
//!                    └─────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use data_prep::{DataPrepConfig, prepare_data};
//!
//! let config = DataPrepConfig {
//!     symbol: "BTC/USD".to_string(),
//!     exchange: "Kraken".to_string(),
//!     output_dir: PathBuf::from("/data/historical"),
//!     chunk_size: 10_000,
//!     compute_features: true,
//! };
//!
//! prepare_data(&db_pool, &config).await?;
//! ```

pub mod binary_format;
pub mod db_streamer;
pub mod feature_compute;
pub mod tick;

// Re-export main types
pub use binary_format::{BinaryFileHeader, BinaryFileWriter, BinaryFileReader, SlicedTickSource, HEADER_SIZE};
pub use db_streamer::DatabaseStreamer;
pub use feature_compute::FeatureComputer;
pub use tick::SimulationTick;

use std::path::PathBuf;
use anyhow::Result;

/// Configuration for data preparation job
#[derive(Debug, Clone)]
pub struct DataPrepConfig {
    /// Trading symbol (e.g., "BTC/USD")
    pub symbol: String,
    /// Exchange name (e.g., "Kraken")
    pub exchange: String,
    /// Output directory for binary files
    pub output_dir: PathBuf,
    /// Number of rows to fetch per database query
    pub chunk_size: usize,
    /// Whether to precompute features (volatility, EMA, etc.)
    pub compute_features: bool,
    /// Start time filter (optional)
    pub start_time: Option<chrono::DateTime<chrono::Utc>>,
    /// End time filter (optional)
    pub end_time: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for DataPrepConfig {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            output_dir: PathBuf::from("/data/historical"),
            chunk_size: 10_000,
            compute_features: false,
            start_time: None,
            end_time: None,
        }
    }
}

/// Main entry point: prepare data for backtesting
#[cfg(feature = "postgres")]
pub async fn prepare_data(
    pool: &diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>,
    config: &DataPrepConfig,
) -> Result<PrepareResult> {
    use tracing::info;
    
    info!(
        symbol = %config.symbol,
        exchange = %config.exchange,
        chunk_size = config.chunk_size,
        "Starting data preparation"
    );
    
    // Create output directory if needed
    std::fs::create_dir_all(&config.output_dir)?;
    
    // Generate output filename
    let filename = format!(
        "{}_{}.bin",
        config.symbol.replace('/', "_"),
        config.exchange
    );
    let output_path = config.output_dir.join(&filename);
    
    // Create streamer and writer
    let streamer = DatabaseStreamer::new(pool.clone(), config.chunk_size);
    let mut writer = BinaryFileWriter::create(&output_path)?;
    
    // Stream and convert
    let mut total_ticks = 0u64;
    let mut chunks_processed = 0u64;
    
    let stream = streamer.stream_orders(
        &config.symbol,
        &config.exchange,
        config.start_time,
        config.end_time,
    );
    
    use futures::StreamExt;
    tokio::pin!(stream);
    while let Some(chunk_result) = stream.next().await {
        let orders = chunk_result?;
        if orders.is_empty() {
            break;
        }
        
        // Convert to SimulationTick
        let ticks: Vec<SimulationTick> = orders
            .iter()
            .map(SimulationTick::from)
            .collect();
        
        total_ticks += ticks.len() as u64;
        chunks_processed += 1;
        
        // Write chunk
        writer.write_ticks(&ticks)?;
        
        if chunks_processed % 100 == 0 {
            info!(
                chunks = chunks_processed,
                ticks = total_ticks,
                "Progress"
            );
        }
    }
    
    // Finalize file
    let header = writer.finalize()?;
    
    // Optionally compute features (second pass)
    if config.compute_features {
        info!("Computing features (second pass)...");
        let computer = FeatureComputer::default();
        computer.compute_and_update(&output_path)?;
    }
    
    info!(
        total_ticks = total_ticks,
        file_size_mb = header.file_size_bytes / 1_000_000,
        path = %output_path.display(),
        "Data preparation complete"
    );
    
    Ok(PrepareResult {
        output_path,
        total_ticks,
        file_size_bytes: header.file_size_bytes,
    })
}

/// Result of data preparation
#[derive(Debug)]
pub struct PrepareResult {
    pub output_path: PathBuf,
    pub total_ticks: u64,
    pub file_size_bytes: u64,
}
