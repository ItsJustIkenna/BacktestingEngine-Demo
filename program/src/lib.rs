//! BacktestingEngine Program Library
//! 
//! Main library entry point for the backtesting engine program.
//! Provides common functionality and logging infrastructure.

// pub mod logging_facade;  // Disabled - using simple println! for now
pub mod cli_visualizer;
pub mod demo;
pub mod config;

// Service modules
pub mod job_queue;
pub mod api;
pub mod worker;
pub mod streaming; // Memory-efficient data loading

// Re-export logging components for easy access
// pub use logging_facade::{
//     BACKTEST_LOGGER, STRATEGY_LOGGER, PORTFOLIO_LOGGER, RISK_LOGGER,
//     ORDERBOOK_LOGGER, DATALOADER_LOGGER, WALKFORWARD_LOGGER, 
//     GENETIC_LOGGER, METRICS_LOGGER, MAIN_LOGGER,
//     BacktestEngineLogger,
// };

// Logging macros (log_info, log_warn, log_error, log_debug, log_trace) are automatically
// exported at the crate root via #[macro_export] in logging_facade.rs



