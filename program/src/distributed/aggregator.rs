//! Result Aggregator for distributed backtesting
//!
//! Combines chunk results into a final aggregated result,
//! calculating overall metrics like Sharpe ratio, max drawdown, etc.

use std::collections::HashMap;
use protocol::broker::messages::{BacktestChunkResult, BacktestAggregatedResult};

/// Aggregator for combining chunk results
pub struct ResultAggregator {
    job_id: String,
    total_chunks: i32,
    results: HashMap<i32, BacktestChunkResult>,
}

impl ResultAggregator {
    /// Create a new aggregator for a job
    pub fn new(job_id: impl Into<String>, total_chunks: i32) -> Self {
        Self {
            job_id: job_id.into(),
            total_chunks,
            results: HashMap::new(),
        }
    }
    
    /// Add a chunk result
    pub fn add_result(&mut self, result: BacktestChunkResult) {
        self.results.insert(result.chunk_id, result);
    }
    
    /// Check if all chunks are received
    pub fn is_complete(&self) -> bool {
        self.results.len() == self.total_chunks as usize
    }
    
    /// Get completion percentage
    pub fn completion_percentage(&self) -> f32 {
        (self.results.len() as f32 / self.total_chunks as f32) * 100.0
    }
    
    /// Get number of successful chunks
    pub fn successful_chunks(&self) -> usize {
        self.results.values().filter(|r| r.success).count()
    }
    
    /// Get number of failed chunks
    pub fn failed_chunks(&self) -> usize {
        self.results.values().filter(|r| !r.success).count()
    }
    
    /// Get running PnL (sum of all received chunks)
    pub fn running_pnl(&self) -> f64 {
        self.results.values().map(|r| r.net_pnl).sum()
    }
    
    /// Get running trade count
    pub fn running_trades(&self) -> i32 {
        self.results.values().map(|r| r.num_trades).sum()
    }
    
    /// Aggregate all results into final result
    /// 
    /// This combines metrics from all chunks:
    /// - PnL: summed
    /// - Trades: summed
    /// - Drawdown: maximum across chunks (conservative)
    /// - Sharpe: calculated from combined daily returns
    pub fn aggregate(&self) -> Result<BacktestAggregatedResult, AggregationError> {
        if !self.is_complete() {
            return Err(AggregationError::IncompleteResults {
                received: self.results.len(),
                expected: self.total_chunks as usize,
            });
        }
        
        // Sort results by chunk_id to ensure correct order
        let mut sorted_results: Vec<_> = self.results.values().collect();
        sorted_results.sort_by_key(|r| r.chunk_id);
        
        // Aggregate basic metrics
        let total_net_pnl: f64 = sorted_results.iter().map(|r| r.net_pnl).sum();
        let total_gross_profit: f64 = sorted_results.iter().map(|r| r.gross_profit).sum();
        let total_gross_loss: f64 = sorted_results.iter().map(|r| r.gross_loss).sum();
        let total_trades: i32 = sorted_results.iter().map(|r| r.num_trades).sum();
        let total_winning_trades: i32 = sorted_results.iter().map(|r| r.winning_trades).sum();
        let total_transaction_costs: f64 = sorted_results.iter().map(|r| r.transaction_costs).sum();
        let total_events: i64 = sorted_results.iter().map(|r| r.events_processed).sum();
        let total_processing_time_ms: i64 = sorted_results.iter().map(|r| r.processing_duration_ms).sum();
        
        // Max drawdown: find maximum across all chunks
        // This is conservative - actual max could be higher due to cross-chunk effects
        let max_drawdown = sorted_results.iter()
            .map(|r| r.max_drawdown)
            .fold(0.0f64, |a, b| a.max(b));
        
        // Win rate
        let win_rate = if total_trades > 0 {
            total_winning_trades as f32 / total_trades as f32
        } else {
            0.0
        };
        
        // Profit factor
        let profit_factor = if total_gross_loss.abs() > 1e-10 {
            (total_gross_profit / total_gross_loss.abs()) as f32
        } else if total_gross_profit > 0.0 {
            999.0 // Cap at high value if no losses
        } else {
            0.0
        };
        
        // Combine daily returns and calculate risk metrics
        let combined_returns = self.combine_daily_returns(&sorted_results);
        let (sharpe_ratio, sortino_ratio) = self.calculate_risk_metrics(&combined_returns);
        
        // Calmar ratio (annualized return / max drawdown)
        let calmar_ratio = if max_drawdown.abs() > 1e-10 {
            // Rough annualization (assume 252 trading days)
            let total_days = combined_returns.len() as f64;
            let annualization_factor = if total_days > 0.0 { 252.0 / total_days } else { 1.0 };
            let annualized_return = total_net_pnl * annualization_factor;
            (annualized_return / max_drawdown.abs()) as f32
        } else {
            0.0
        };
        
        // Count unique workers
        let unique_workers: std::collections::HashSet<_> = sorted_results.iter()
            .map(|r| r.worker_id.clone())
            .collect();
        
        Ok(BacktestAggregatedResult {
            job_id: self.job_id.clone(),
            total_net_pnl,
            total_gross_profit,
            total_gross_loss,
            total_trades,
            total_winning_trades,
            win_rate,
            profit_factor,
            max_drawdown,
            sharpe_ratio,
            sortino_ratio,
            calmar_ratio,
            total_transaction_costs,
            total_events,
            total_processing_time_ms,
            num_workers: unique_workers.len() as i32,
            chunks_processed: self.results.len() as i32,
            chunks_failed: self.failed_chunks() as i32,
        })
    }
    
    /// Combine daily returns from all chunks (in order)
    fn combine_daily_returns(&self, sorted_results: &[&BacktestChunkResult]) -> Vec<f64> {
        let mut all_returns = Vec::new();
        
        for result in sorted_results {
            if let Ok(returns) = serde_json::from_slice::<Vec<f64>>(&result.daily_returns) {
                all_returns.extend(returns);
            }
        }
        
        all_returns
    }
    
    /// Calculate Sharpe and Sortino ratios from daily returns
    fn calculate_risk_metrics(&self, returns: &[f64]) -> (f32, f32) {
        if returns.is_empty() {
            return (0.0, 0.0);
        }
        
        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;
        
        // Standard deviation for Sharpe
        let variance = returns.iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>() / n;
        let std_dev = variance.sqrt();
        
        // Downside deviation for Sortino (only negative returns)
        let downside_variance = returns.iter()
            .map(|r| if *r < 0.0 { r.powi(2) } else { 0.0 })
            .sum::<f64>() / n;
        let downside_dev = downside_variance.sqrt();
        
        // Risk-free rate assumption: 0% for simplicity
        let annualization_factor = 252.0_f64.sqrt();
        
        let sharpe = if std_dev > 1e-10 {
            ((mean / std_dev) * annualization_factor) as f32
        } else {
            0.0
        };
        
        let sortino = if downside_dev > 1e-10 {
            ((mean / downside_dev) * annualization_factor) as f32
        } else {
            sharpe // Fall back to Sharpe if no downside
        };
        
        (sharpe, sortino)
    }
}

/// Aggregation errors
#[derive(Debug)]
pub enum AggregationError {
    IncompleteResults {
        received: usize,
        expected: usize,
    },
    InvalidData(String),
}

impl std::fmt::Display for AggregationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AggregationError::IncompleteResults { received, expected } => {
                write!(f, "Incomplete results: received {} of {} chunks", received, expected)
            }
            AggregationError::InvalidData(msg) => {
                write!(f, "Invalid data: {}", msg)
            }
        }
    }
}

impl std::error::Error for AggregationError {}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_result(chunk_id: i32, net_pnl: f64, num_trades: i32) -> BacktestChunkResult {
        BacktestChunkResult {
            job_id: "test-job".to_string(),
            chunk_id,
            worker_id: format!("worker-{}", chunk_id % 2),
            success: true,
            error_message: String::new(),
            net_pnl,
            gross_profit: net_pnl.max(0.0),
            gross_loss: (-net_pnl).max(0.0),
            num_trades,
            winning_trades: (num_trades as f64 * 0.6) as i32,
            max_drawdown: 100.0,
            transaction_costs: 10.0,
            ending_position: 0.0,
            ending_cash: 10000.0 + net_pnl,
            daily_returns: serde_json::to_vec(&vec![0.01, -0.005, 0.02]).unwrap(),
            events_processed: 50000,
            processing_duration_ms: 1000,
            peak_memory_bytes: 256_000_000,
            monte_carlo_run: 0,
        }
    }
    
    #[test]
    fn test_aggregator_complete() {
        let mut aggregator = ResultAggregator::new("test-job", 3);
        
        aggregator.add_result(create_test_result(0, 100.0, 10));
        aggregator.add_result(create_test_result(1, 150.0, 15));
        aggregator.add_result(create_test_result(2, -50.0, 8));
        
        assert!(aggregator.is_complete());
        assert_eq!(aggregator.running_pnl(), 200.0);
        assert_eq!(aggregator.running_trades(), 33);
    }
    
    #[test]
    fn test_aggregator_incomplete() {
        let mut aggregator = ResultAggregator::new("test-job", 5);
        
        aggregator.add_result(create_test_result(0, 100.0, 10));
        aggregator.add_result(create_test_result(1, 150.0, 15));
        
        assert!(!aggregator.is_complete());
        assert_eq!(aggregator.completion_percentage(), 40.0);
    }
    
    #[test]
    fn test_aggregate_final_result() {
        let mut aggregator = ResultAggregator::new("test-job", 2);
        
        aggregator.add_result(create_test_result(0, 100.0, 10));
        aggregator.add_result(create_test_result(1, 150.0, 15));
        
        let result = aggregator.aggregate().unwrap();
        
        assert_eq!(result.total_net_pnl, 250.0);
        assert_eq!(result.total_trades, 25);
        assert_eq!(result.chunks_processed, 2);
        assert_eq!(result.num_workers, 2); // chunk 0 -> worker-0, chunk 1 -> worker-1
    }
}
