//! Distributed Backtesting System
//! 
//! This module implements a fully distributed backtesting architecture using
//! MessageBrokerEngine for coordination. It enables horizontal scaling by
//! splitting large backtests into time-based chunks that are processed by
//! independent workers.
//!
//! # Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         BACKTEST COORDINATOR                            │
//! │  (Receives job, creates chunks, publishes to broker, aggregates results)│
//! └───────────────────────┬───────────────────────┬─────────────────────────┘
//!                         │ Publishes             │ Subscribes
//!                         ▼                       ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    MESSAGE BROKER ENGINE                                │
//! │  Topics:                                                                │
//! │    backtest.chunks.{job_id}      → Workers subscribe                    │
//! │    backtest.results.{job_id}     → Coordinator subscribes               │
//! │    backtest.progress.{job_id}    → Progress updates                     │
//! │    backtest.cancel.{job_id}      → Cancellation requests                │
//! └─────────────────────────────────────────────────────────────────────────┘
//!         ▲                ▲                ▲                ▲
//!    ┌────┴───┐       ┌────┴───┐       ┌────┴───┐       ┌────┴───┐
//!    │Worker 1│       │Worker 2│       │Worker 3│       │Worker N│
//!    │Chunk 1 │       │Chunk 2 │       │Chunk 3 │       │Chunk N │
//!    └────────┘       └────────┘       └────────┘       └────────┘
//! ```
//!
//! # Benefits
//! - **Memory Efficient**: Each worker loads only ~200K events (~250MB) instead of 6.6M (~8GB)
//! - **Horizontally Scalable**: Add more workers to process faster
//! - **Fault Tolerant**: Failed chunks can be retried without restarting entire job
//! - **Real-time Progress**: Progress updates via pub/sub messaging
//!
//! # Usage
//! ```ignore
//! // Coordinator side (API server)
//! let coordinator = DistributedCoordinator::new(broker, db);
//! let result = coordinator.run_distributed_backtest(job).await?;
//!
//! // Worker side
//! let worker = DistributedWorker::new(broker, db, worker_id);
//! worker.run().await; // Runs forever, processing chunks
//! ```

pub mod chunking;
pub mod coordinator;
pub mod worker;
pub mod aggregator;
pub mod broker_client;
pub mod ga_coordinator;
pub mod ga_worker;

pub use chunking::{ChunkCalculator, BacktestChunkInfo, ChunkingStrategy};
pub use coordinator::DistributedCoordinator;
pub use worker::DistributedWorker;
pub use aggregator::ResultAggregator;
pub use broker_client::BacktestBrokerClient;
pub use ga_coordinator::{DistributedGACoordinator, DistributedGAConfig};
pub use ga_worker::DistributedGAWorker;

/// Default chunk size in events
pub const DEFAULT_CHUNK_SIZE: usize = 200_000;

/// Default chunk duration in hours (fallback if event count unknown)
pub const DEFAULT_CHUNK_HOURS: i64 = 24;

/// Maximum chunks per job (to prevent runaway)
pub const MAX_CHUNKS_PER_JOB: usize = 1000;

/// Topic patterns for distributed backtesting
pub mod topics {
    /// Topic for publishing chunks: backtest.chunks.{job_id}
    pub fn chunks(job_id: &str) -> String {
        format!("backtest.chunks.{}", job_id)
    }
    
    /// Topic for chunk results: backtest.results.{job_id}
    pub fn results(job_id: &str) -> String {
        format!("backtest.results.{}", job_id)
    }
    
    /// Topic for progress updates: backtest.progress.{job_id}
    pub fn progress(job_id: &str) -> String {
        format!("backtest.progress.{}", job_id)
    }
    
    /// Topic for cancellation: backtest.cancel.{job_id}
    pub fn cancel(job_id: &str) -> String {
        format!("backtest.cancel.{}", job_id)
    }
    
    /// Wildcard topic for all chunks (workers subscribe to this)
    pub fn all_chunks() -> String {
        "backtest.chunks.*".to_string()
    }
    
    /// Wildcard topic for all results (coordinator subscribes)
    pub fn all_results() -> String {
        "backtest.results.*".to_string()
    }
    
    // ========================================================================
    // DISTRIBUTED GA TOPICS
    // ========================================================================
    
    /// Topic for data broadcast to workers: ga.data.broadcast
    pub fn ga_data_broadcast() -> String {
        "ga.data.broadcast".to_string()
    }
    
    /// Topic for data cache acknowledgments: ga.data.ack
    pub fn ga_data_ack() -> String {
        "ga.data.ack".to_string()
    }
    
    /// Topic for chromosome evaluation requests: ga.chromosome.eval.{job_id}
    pub fn ga_chromosome_eval(job_id: &str) -> String {
        format!("ga.chromosome.eval.{}", job_id)
    }
    
    /// Topic for chromosome results: ga.chromosome.result.{job_id}
    pub fn ga_chromosome_result(job_id: &str) -> String {
        format!("ga.chromosome.result.{}", job_id)
    }
    
    /// Wildcard topic for all chromosome evals (workers subscribe)
    pub fn ga_all_chromosome_evals() -> String {
        "ga.chromosome.eval.*".to_string()
    }
}
