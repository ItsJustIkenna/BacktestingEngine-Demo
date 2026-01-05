//! Backtesting Engine Library
//!
//! Core simulation and analysis modules for the backtesting platform.

// Logging must be defined first since other modules use it
#[macro_use]
pub mod logging_facade;

// Core modules
pub mod types;
pub mod simulation;
pub mod engine;
pub mod html_export;
pub mod monte_carlo;
pub mod realistic_simulation;
pub mod momentum_simulation;
pub mod statistical_significance;
pub mod portfolio_simulation;
pub mod lightweight_sim;
pub mod latency;
pub mod simd_utils;
pub mod portfolio_risk;
pub mod accuracy_validation;

// Re-export commonly used types at crate root
pub use types::{BacktestResult, BacktestEvent};
pub use engine::{BacktestEngine, BacktestResults, WalkForwardSummary, AnalysisMetadata, ProgressCallback};
pub use html_export::HtmlExporter;
pub use simulation::SimulationLoop;
pub use monte_carlo::{MonteCarloResult, run_monte_carlo_simulation};
pub use realistic_simulation::{
    RealisticSimulator, 
    RealisticASParams,
    RealisticSimResult,
    OrderBookEvent, 
    EventType, 
    Side,
    FillModelType,
    ticks_to_trade_events,
};
#[cfg(feature = "postgres")]
pub use realistic_simulation::l2_snapshots_to_events;
pub use statistical_significance::{
    SignificanceResult, 
    compute_sharpe_significance,
    expected_max_sharpe_under_null,
};

// Momentum strategy simulation
pub use momentum_simulation::{
    MomentumSimulator,
    MomentumParams,
    MomentumSimResult,
    MomentumSignal,
    Position,
    RollingWindow,
};

