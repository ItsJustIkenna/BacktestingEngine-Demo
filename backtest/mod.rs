//! Backtest module root
//!
//! Orchestrates the simulation loop, integrates strategies, orderbook, portfolio, and risk management.

pub mod engine;
pub mod simulation;
pub mod report;
pub mod types;

// Re-export main engine for convenience
pub use engine::BacktestEngine;
