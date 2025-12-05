//! BacktestingEngine Program Library
//! 
//! Main library entry point for the backtesting engine program.
//! Provides common functionality and logging infrastructure.

#[macro_use]
pub mod logging_facade;  // Ultra logger integration for comprehensive logging
pub mod cli_visualizer;
pub mod demo;
pub mod config;

// Service modules
pub mod job_queue;
pub mod job_broker; // Message broker-based job distribution
pub mod api;
pub mod worker;
pub mod streaming; // Memory-efficient data loading
pub mod progress_broadcaster; // Real-time progress streaming via broadcast channels

// Re-export logging components for easy access
pub use logging_facade::{
    BACKTEST_LOGGER, STRATEGY_LOGGER, PORTFOLIO_LOGGER, RISK_LOGGER,
    ORDERBOOK_LOGGER, DATALOADER_LOGGER, WALKFORWARD_LOGGER, 
    GENETIC_LOGGER, METRICS_LOGGER, MAIN_LOGGER,
    BacktestEngineLogger,
};

// Re-export progress broadcaster for WebSocket integration
pub use progress_broadcaster::{ProgressBroadcaster, ProgressUpdate, get_progress_broadcaster};

// Re-export job broker for job distribution
pub use job_broker::{JobBroker, JobMessage, JobReceiver, JobPriority, get_job_broker};

// Logging macros (log_info, log_warn, log_error, log_debug, log_trace) are automatically
// exported at the crate root via #[macro_export] in logging_facade.rs



