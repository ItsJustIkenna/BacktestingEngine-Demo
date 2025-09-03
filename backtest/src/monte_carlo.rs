//! Monte Carlo simulation module for backtesting

use config::MonteCarloConfig;
use crate::BacktestEngine;
use metrics::PerformanceMetrics;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub num_runs: usize,
    pub mean_metrics: PerformanceMetrics,
    pub stddev_metrics: PerformanceMetrics,
    pub percentile_metrics: HashMap<u8, PerformanceMetrics>, // percentile -> metrics
}

/// Helper functions for aggregation
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn std_dev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        0.0
    } else {
        let mean_val = mean(values);
        let variance = values.iter()
            .map(|x| (x - mean_val).powi(2))
            .sum::<f64>() / (values.len() - 1) as f64;
        variance.sqrt()
    }
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = (p / 100.0 * (values.len() - 1) as f64) as usize;
    values[index.min(values.len() - 1)]
}

/// Run Monte Carlo simulation on backtest results
pub fn run_monte_carlo_simulation(
    _config: &MonteCarloConfig,
    _base_engine: &BacktestEngine,
) -> anyhow::Result<MonteCarloResult> {
    // Stub implementation for now
    let mut percentiles = HashMap::new();
    percentiles.insert(5, PerformanceMetrics::default());
    percentiles.insert(25, PerformanceMetrics::default());
    percentiles.insert(50, PerformanceMetrics::default());
    percentiles.insert(75, PerformanceMetrics::default());
    percentiles.insert(95, PerformanceMetrics::default());
    
    Ok(MonteCarloResult {
        num_runs: 500,
        mean_metrics: PerformanceMetrics::default(),
        stddev_metrics: PerformanceMetrics::default(),
        percentile_metrics: percentiles,
    })
}
