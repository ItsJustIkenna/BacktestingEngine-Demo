//! Backtest Accuracy Validation Module
//!
//! This module provides infrastructure for comparing backtest predictions
//! against live execution results, enabling:
//! - Fill rate accuracy tracking
//! - Slippage prediction vs reality
//! - Latency assumption validation
//! - Fee estimation accuracy
//! - Overall PnL prediction accuracy
//!
//! This is critical for validating that backtest results are predictive
//! of actual trading performance.

use serde::{Deserialize, Serialize};

/// Individual execution comparison between backtest and live
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionComparison {
    /// Unique trade identifier
    pub trade_id: String,
    /// Symbol traded
    pub symbol: String,
    /// Timestamp of execution
    pub timestamp_ms: i64,
    /// Side (buy/sell)
    pub side: String,
    
    // Backtest predictions
    pub backtest_fill_price: f64,
    pub backtest_fill_qty: f64,
    pub backtest_fee: f64,
    pub backtest_latency_ms: i64,
    
    // Live execution results
    pub live_fill_price: f64,
    pub live_fill_qty: f64,
    pub live_fee: f64,
    pub live_latency_ms: i64,
    
    // Computed differences
    pub price_slippage_bps: f64,
    pub qty_fill_ratio: f64,
    pub fee_error_pct: f64,
    pub latency_error_ms: i64,
}

/// Aggregate accuracy metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyMetrics {
    /// Number of comparisons
    pub num_comparisons: usize,
    
    // Fill rate accuracy
    pub backtest_fill_rate: f64,
    pub live_fill_rate: f64,
    pub fill_rate_error_pct: f64,
    
    // Price accuracy
    pub mean_price_slippage_bps: f64,
    pub median_price_slippage_bps: f64,
    pub max_adverse_slippage_bps: f64,
    pub slippage_std_bps: f64,
    
    // Quantity accuracy
    pub mean_qty_fill_ratio: f64, // live_qty / backtest_qty
    pub partial_fill_pct: f64, // % of trades with < 100% fill
    
    // Fee accuracy
    pub mean_fee_error_pct: f64,
    pub total_fee_underestimate: f64,
    
    // Latency accuracy
    pub mean_latency_error_ms: f64,
    pub latency_underestimate_pct: f64, // % of trades where backtest latency < live
    
    // PnL accuracy
    pub backtest_total_pnl: f64,
    pub live_total_pnl: f64,
    pub pnl_tracking_error: f64, // (backtest - live) / live
    pub pnl_correlation: f64, // Correlation of per-trade PnL
    
    // Overall accuracy score (0-100)
    pub accuracy_score: f64,
    
    // Recommendations
    pub recommendations: Vec<String>,
}

/// Accuracy validation engine
pub struct AccuracyValidator {
    /// Comparison records
    comparisons: Vec<ExecutionComparison>,
    /// Running totals for backtest
    backtest_orders: usize,
    backtest_fills: usize,
    backtest_total_pnl: f64,
    /// Running totals for live
    live_orders: usize,
    live_fills: usize,
    live_total_pnl: f64,
    /// Per-trade PnL for correlation
    backtest_trade_pnl: Vec<f64>,
    live_trade_pnl: Vec<f64>,
}

impl AccuracyValidator {
    pub fn new() -> Self {
        Self {
            comparisons: Vec::new(),
            backtest_orders: 0,
            backtest_fills: 0,
            backtest_total_pnl: 0.0,
            live_orders: 0,
            live_fills: 0,
            live_total_pnl: 0.0,
            backtest_trade_pnl: Vec::new(),
            live_trade_pnl: Vec::new(),
        }
    }
    
    /// Record a backtest order attempt
    pub fn record_backtest_order(&mut self) {
        self.backtest_orders += 1;
    }
    
    /// Record a backtest fill
    pub fn record_backtest_fill(&mut self, pnl: f64) {
        self.backtest_fills += 1;
        self.backtest_total_pnl += pnl;
        self.backtest_trade_pnl.push(pnl);
    }
    
    /// Record a live order attempt
    pub fn record_live_order(&mut self) {
        self.live_orders += 1;
    }
    
    /// Record a live fill
    pub fn record_live_fill(&mut self, pnl: f64) {
        self.live_fills += 1;
        self.live_total_pnl += pnl;
        self.live_trade_pnl.push(pnl);
    }
    
    /// Add a matched execution comparison
    pub fn add_comparison(
        &mut self,
        trade_id: &str,
        symbol: &str,
        timestamp_ms: i64,
        side: &str,
        backtest_fill_price: f64,
        backtest_fill_qty: f64,
        backtest_fee: f64,
        backtest_latency_ms: i64,
        live_fill_price: f64,
        live_fill_qty: f64,
        live_fee: f64,
        live_latency_ms: i64,
    ) {
        // Calculate differences
        let price_slippage_bps = if backtest_fill_price > 0.0 {
            ((live_fill_price - backtest_fill_price) / backtest_fill_price) * 10000.0
        } else {
            0.0
        };
        
        // For buys, positive slippage = we paid more (bad)
        // For sells, negative slippage = we received less (bad)
        // Normalize so positive = adverse
        let price_slippage_bps = if side.to_lowercase() == "sell" {
            -price_slippage_bps
        } else {
            price_slippage_bps
        };
        
        let qty_fill_ratio = if backtest_fill_qty > 0.0 {
            live_fill_qty / backtest_fill_qty
        } else {
            0.0
        };
        
        let fee_error_pct = if backtest_fee > 0.0 {
            ((live_fee - backtest_fee) / backtest_fee) * 100.0
        } else {
            0.0
        };
        
        let latency_error_ms = live_latency_ms - backtest_latency_ms;
        
        self.comparisons.push(ExecutionComparison {
            trade_id: trade_id.to_string(),
            symbol: symbol.to_string(),
            timestamp_ms,
            side: side.to_string(),
            backtest_fill_price,
            backtest_fill_qty,
            backtest_fee,
            backtest_latency_ms,
            live_fill_price,
            live_fill_qty,
            live_fee,
            live_latency_ms,
            price_slippage_bps,
            qty_fill_ratio,
            fee_error_pct,
            latency_error_ms,
        });
    }
    
    /// Calculate accuracy metrics
    pub fn calculate_metrics(&self) -> AccuracyMetrics {
        if self.comparisons.is_empty() {
            return AccuracyMetrics::default();
        }
        
        let n = self.comparisons.len();
        
        // Fill rate
        let backtest_fill_rate = if self.backtest_orders > 0 {
            self.backtest_fills as f64 / self.backtest_orders as f64
        } else {
            0.0
        };
        
        let live_fill_rate = if self.live_orders > 0 {
            self.live_fills as f64 / self.live_orders as f64
        } else {
            0.0
        };
        
        let fill_rate_error_pct = if live_fill_rate > 0.0 {
            ((backtest_fill_rate - live_fill_rate) / live_fill_rate).abs() * 100.0
        } else {
            0.0
        };
        
        // Price slippage
        let slippages: Vec<f64> = self.comparisons.iter().map(|c| c.price_slippage_bps).collect();
        let mean_price_slippage_bps = slippages.iter().sum::<f64>() / n as f64;
        let median_price_slippage_bps = percentile(&slippages, 50.0);
        let max_adverse_slippage_bps = slippages.iter().cloned().fold(0.0, f64::max);
        let slippage_std_bps = std_dev(&slippages);
        
        // Quantity fill ratio
        let qty_ratios: Vec<f64> = self.comparisons.iter().map(|c| c.qty_fill_ratio).collect();
        let mean_qty_fill_ratio = qty_ratios.iter().sum::<f64>() / n as f64;
        let partial_fill_pct = (qty_ratios.iter().filter(|&&r| r < 1.0).count() as f64 / n as f64) * 100.0;
        
        // Fee accuracy
        let fee_errors: Vec<f64> = self.comparisons.iter().map(|c| c.fee_error_pct).collect();
        let mean_fee_error_pct = fee_errors.iter().sum::<f64>() / n as f64;
        let total_fee_underestimate: f64 = self.comparisons.iter()
            .map(|c| (c.live_fee - c.backtest_fee).max(0.0))
            .sum();
        
        // Latency accuracy
        let latency_errors: Vec<i64> = self.comparisons.iter().map(|c| c.latency_error_ms).collect();
        let mean_latency_error_ms = latency_errors.iter().sum::<i64>() as f64 / n as f64;
        let latency_underestimate_pct = (latency_errors.iter().filter(|&&e| e > 0).count() as f64 / n as f64) * 100.0;
        
        // PnL accuracy
        let pnl_tracking_error = if self.live_total_pnl.abs() > 0.0 {
            (self.backtest_total_pnl - self.live_total_pnl) / self.live_total_pnl.abs()
        } else {
            0.0
        };
        
        let pnl_correlation = correlation(&self.backtest_trade_pnl, &self.live_trade_pnl);
        
        // Calculate overall accuracy score (0-100)
        let accuracy_score = self.calculate_accuracy_score(
            fill_rate_error_pct,
            mean_price_slippage_bps.abs(),
            mean_qty_fill_ratio,
            mean_fee_error_pct.abs(),
            pnl_tracking_error.abs(),
            pnl_correlation,
        );
        
        // Generate recommendations
        let recommendations = self.generate_recommendations(
            fill_rate_error_pct,
            mean_price_slippage_bps,
            mean_qty_fill_ratio,
            mean_fee_error_pct,
            mean_latency_error_ms,
            pnl_tracking_error,
        );
        
        AccuracyMetrics {
            num_comparisons: n,
            backtest_fill_rate,
            live_fill_rate,
            fill_rate_error_pct,
            mean_price_slippage_bps,
            median_price_slippage_bps,
            max_adverse_slippage_bps,
            slippage_std_bps,
            mean_qty_fill_ratio,
            partial_fill_pct,
            mean_fee_error_pct,
            total_fee_underestimate,
            mean_latency_error_ms,
            latency_underestimate_pct,
            backtest_total_pnl: self.backtest_total_pnl,
            live_total_pnl: self.live_total_pnl,
            pnl_tracking_error,
            pnl_correlation,
            accuracy_score,
            recommendations,
        }
    }
    
    /// Calculate composite accuracy score
    fn calculate_accuracy_score(
        &self,
        fill_rate_error: f64,
        slippage_error: f64,
        qty_ratio: f64,
        fee_error: f64,
        pnl_tracking_error: f64,
        pnl_correlation: f64,
    ) -> f64 {
        // Weights for each component
        let weights = [
            (25.0, 1.0 - (fill_rate_error / 100.0).min(1.0)),     // Fill rate: 25%
            (20.0, 1.0 - (slippage_error / 50.0).min(1.0)),       // Slippage: 20% (50bps = 0)
            (15.0, qty_ratio.min(1.0)),                            // Fill qty: 15%
            (10.0, 1.0 - (fee_error / 50.0).min(1.0)),            // Fees: 10%
            (15.0, 1.0 - pnl_tracking_error.min(1.0)),            // PnL tracking: 15%
            (15.0, pnl_correlation.max(0.0)),                      // PnL correlation: 15%
        ];
        
        let weighted_sum: f64 = weights.iter().map(|(w, v)| w * v).sum();
        let total_weight: f64 = weights.iter().map(|(w, _)| w).sum();
        
        (weighted_sum / total_weight) * 100.0
    }
    
    /// Generate recommendations based on accuracy analysis
    fn generate_recommendations(
        &self,
        fill_rate_error: f64,
        mean_slippage: f64,
        qty_ratio: f64,
        fee_error: f64,
        latency_error: f64,
        pnl_tracking: f64,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();
        
        if fill_rate_error > 20.0 {
            recommendations.push(format!(
                "Fill rate overestimated by {:.1}%. Consider more conservative queue position modeling.",
                fill_rate_error
            ));
        }
        
        if mean_slippage > 5.0 {
            recommendations.push(format!(
                "Mean adverse slippage of {:.2} bps. Add explicit slippage model for order sizes.",
                mean_slippage
            ));
        }
        
        if qty_ratio < 0.9 {
            recommendations.push(format!(
                "Only {:.1}% of expected quantity filled. Enable partial fill modeling.",
                qty_ratio * 100.0
            ));
        }
        
        if fee_error > 20.0 {
            recommendations.push(format!(
                "Fee estimation off by {:.1}%. Verify maker/taker fee assumptions.",
                fee_error
            ));
        }
        
        if latency_error > 10.0 {
            recommendations.push(format!(
                "Backtest underestimates latency by {:.1}ms. Increase latency_mean_ms parameter.",
                latency_error
            ));
        }
        
        if pnl_tracking.abs() > 0.20 {
            recommendations.push(format!(
                "PnL tracking error of {:.1}%. Review overall simulation assumptions.",
                pnl_tracking * 100.0
            ));
        }
        
        if recommendations.is_empty() {
            recommendations.push("Backtest accuracy within acceptable bounds.".to_string());
        }
        
        recommendations
    }
    
    /// Export comparisons for external analysis
    pub fn export_comparisons(&self) -> &[ExecutionComparison] {
        &self.comparisons
    }
    
    /// Clear all data
    pub fn reset(&mut self) {
        self.comparisons.clear();
        self.backtest_orders = 0;
        self.backtest_fills = 0;
        self.backtest_total_pnl = 0.0;
        self.live_orders = 0;
        self.live_fills = 0;
        self.live_total_pnl = 0.0;
        self.backtest_trade_pnl.clear();
        self.live_trade_pnl.clear();
    }
}

impl Default for AccuracyValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AccuracyMetrics {
    fn default() -> Self {
        Self {
            num_comparisons: 0,
            backtest_fill_rate: 0.0,
            live_fill_rate: 0.0,
            fill_rate_error_pct: 0.0,
            mean_price_slippage_bps: 0.0,
            median_price_slippage_bps: 0.0,
            max_adverse_slippage_bps: 0.0,
            slippage_std_bps: 0.0,
            mean_qty_fill_ratio: 0.0,
            partial_fill_pct: 0.0,
            mean_fee_error_pct: 0.0,
            total_fee_underestimate: 0.0,
            mean_latency_error_ms: 0.0,
            latency_underestimate_pct: 0.0,
            backtest_total_pnl: 0.0,
            live_total_pnl: 0.0,
            pnl_tracking_error: 0.0,
            pnl_correlation: 0.0,
            accuracy_score: 0.0,
            recommendations: vec!["No comparison data available.".to_string()],
        }
    }
}

// Helper functions

fn percentile(data: &[f64], p: f64) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn std_dev(data: &[f64]) -> f64 {
    if data.len() < 2 {
        return 0.0;
    }
    let mean = data.iter().sum::<f64>() / data.len() as f64;
    let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (data.len() - 1) as f64;
    variance.sqrt()
}

fn correlation(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return 0.0;
    }
    
    let mean_x = x.iter().take(n).sum::<f64>() / n as f64;
    let mean_y = y.iter().take(n).sum::<f64>() / n as f64;
    
    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;
    
    for i in 0..n {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }
    
    let denom = (var_x * var_y).sqrt();
    if denom > 1e-10 {
        cov / denom
    } else {
        // If both series have zero variance (constant values), 
        // check if they're equal - if so, correlation is perfect (1.0)
        if var_x < 1e-10 && var_y < 1e-10 {
            // Both are constant - check if means are equal
            if (mean_x - mean_y).abs() < 1e-10 {
                1.0  // Perfect correlation when identical constants
            } else {
                0.0  // Different constants = undefined, treat as 0
            }
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_accuracy_calculation() {
        let mut validator = AccuracyValidator::new();
        
        // Simulate perfect accuracy
        for i in 0..100 {
            validator.add_comparison(
                &format!("trade_{}", i),
                "BTC/USD",
                i as i64 * 1000,
                "buy",
                50000.0,  // backtest price
                1.0,      // backtest qty
                5.0,      // backtest fee
                10,       // backtest latency
                50000.0,  // live price (same)
                1.0,      // live qty (same)
                5.0,      // live fee (same)
                10,       // live latency (same)
            );
            // Record PnL for correlation calculation (simulate some profit variance)
            let pnl = 10.0 + (i as f64) * 0.1;  // Varying PnL
            validator.record_backtest_fill(pnl);
            validator.record_live_fill(pnl);  // Same PnL = perfect correlation
        }
        
        validator.backtest_orders = 100;
        validator.live_orders = 100;
        
        let metrics = validator.calculate_metrics();
        
        assert_eq!(metrics.num_comparisons, 100);
        assert!((metrics.mean_price_slippage_bps).abs() < 0.01);
        assert!((metrics.mean_qty_fill_ratio - 1.0).abs() < 0.01);
        assert!(metrics.accuracy_score > 95.0, "Expected > 95.0, got {}", metrics.accuracy_score);
    }
    
    #[test]
    fn test_slippage_detection() {
        let mut validator = AccuracyValidator::new();
        
        // Simulate consistent slippage
        for i in 0..50 {
            validator.add_comparison(
                &format!("trade_{}", i),
                "ETH/USD",
                i as i64 * 1000,
                "buy",
                3000.0,   // backtest price
                1.0,
                3.0,
                10,
                3003.0,   // live price (10 bps slippage)
                1.0,
                3.0,
                15,       // live latency slightly higher
            );
        }
        
        let metrics = validator.calculate_metrics();
        
        assert!((metrics.mean_price_slippage_bps - 10.0).abs() < 0.5); // ~10 bps
        assert!(metrics.recommendations.len() > 0);
    }
}
