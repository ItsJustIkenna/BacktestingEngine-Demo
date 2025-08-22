//! Results and aggregation for walk-forward analysis

use chrono::NaiveDate;
use serde::{Serialize, Deserialize};
use metrics::PerformanceMetrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardWindowResult {
    pub train_start: NaiveDate,
    pub train_end: NaiveDate,
    pub test_start: NaiveDate,
    pub test_end: NaiveDate,
    pub best_params: serde_json::Value,
    pub test_metrics: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardSummary {
    pub window_results: Vec<WalkForwardWindowResult>,
    pub average_test_sharpe: f64,
    pub parameter_stability: f64,
    // Add more aggregate stats as needed
}
