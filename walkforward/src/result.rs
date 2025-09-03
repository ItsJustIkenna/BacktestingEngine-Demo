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
    pub out_of_sample_performance_ratio: f64,
    pub num_windows: usize,
    pub best_window_performance: f64,
    pub worst_window_performance: f64,
    pub consistency_score: f64,
}

impl WalkForwardSummary {
    pub fn new(window_results: Vec<WalkForwardWindowResult>) -> Self {
        let num_windows = window_results.len();
        
        let sharpe_ratios: Vec<f64> = window_results.iter()
            .map(|w| w.test_metrics.sharpe_ratio)
            .collect();
            
        let average_test_sharpe = if !sharpe_ratios.is_empty() {
            sharpe_ratios.iter().sum::<f64>() / sharpe_ratios.len() as f64
        } else {
            0.0
        };
        
        let best_window_performance = sharpe_ratios.iter()
            .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            
        let worst_window_performance = sharpe_ratios.iter()
            .fold(f64::INFINITY, |a, &b| a.min(b));
        
        // Parameter stability: measure consistency of optimal parameters
        let parameter_stability = Self::calculate_parameter_stability(&window_results);
        
        // Out-of-sample performance ratio: average vs best single window
        let out_of_sample_performance_ratio = if best_window_performance > 0.0 {
            average_test_sharpe / best_window_performance
        } else {
            0.0
        };
        
        // Consistency score: based on standard deviation of returns
        let consistency_score = if sharpe_ratios.len() > 1 {
            let variance = sharpe_ratios.iter()
                .map(|x| (x - average_test_sharpe).powi(2))
                .sum::<f64>() / (sharpe_ratios.len() - 1) as f64;
            let std_dev = variance.sqrt();
            (1.0 / (1.0 + std_dev)).max(0.0).min(1.0) // Normalize to 0-1
        } else {
            1.0
        };
        
        Self {
            window_results,
            average_test_sharpe,
            parameter_stability,
            out_of_sample_performance_ratio,
            num_windows,
            best_window_performance,
            worst_window_performance,
            consistency_score,
        }
    }
    
    fn calculate_parameter_stability(window_results: &[WalkForwardWindowResult]) -> f64 {
        // Simplified parameter stability calculation
        // In practice, you'd compare actual parameter values across windows
        if window_results.len() < 2 {
            return 1.0;
        }
        
        // For now, use performance consistency as a proxy for parameter stability
        let performances: Vec<f64> = window_results.iter()
            .map(|w| w.test_metrics.sharpe_ratio)
            .collect();
            
        if performances.is_empty() {
            return 0.0;
        }
        
        let mean = performances.iter().sum::<f64>() / performances.len() as f64;
        let variance = performances.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / performances.len() as f64;
        let coefficient_of_variation = if mean != 0.0 {
            variance.sqrt() / mean.abs()
        } else {
            f64::INFINITY
        };
        
        // Convert to stability score (lower CV = higher stability)
        (1.0 / (1.0 + coefficient_of_variation)).max(0.0).min(1.0)
    }
}
