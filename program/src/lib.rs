//! BacktestingEngine Program Library
//! 
//! Main library entry point for the backtesting engine program.
//! Provides common functionality and logging infrastructure.

pub mod logging_facade;
pub mod cli_visualizer;
pub mod demo;
pub mod config;

// Re-export logging components for easy access
pub use logging_facade::{
    BACKTEST_LOGGER, STRATEGY_LOGGER, PORTFOLIO_LOGGER, RISK_LOGGER,
    ORDERBOOK_LOGGER, DATALOADER_LOGGER, WALKFORWARD_LOGGER, 
    GENETIC_LOGGER, METRICS_LOGGER, MAIN_LOGGER,
    BacktestEngineLogger,
};

// Logging macros are defined in logging_facade.rs and don't need re-export
