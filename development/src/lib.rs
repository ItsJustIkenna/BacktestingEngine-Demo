//! Development Speed Optimization System
//!
//! This module provides tools to dramatically speed up BacktestingEngine development
//! through smart caching, parallel processing, and optimized workflows.

pub mod simple;

pub use simple::*;

/// Initialize development optimization
pub async fn init() -> anyhow::Result<()> {
    simple::init()
}
