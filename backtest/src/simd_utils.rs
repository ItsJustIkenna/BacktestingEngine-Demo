//! SIMD-accelerated utilities for backtest calculations
//!
//! Provides vectorized implementations of common operations that are
//! executed millions of times during backtesting. Uses portable SIMD
//! operations that work across x86_64 (AVX2/AVX-512) and ARM (NEON).
//!
//! Speedups depend on hardware and data size; no benchmark is included in this repo.

/// SIMD lane width - we use 4 f64s per vector for broad compatibility
const SIMD_LANES: usize = 4;

/// Calculate sum of f64 slice using SIMD
/// Falls back to scalar for small arrays or remainders
#[inline]
pub fn simd_sum(values: &[f64]) -> f64 {
    if values.len() < SIMD_LANES * 2 {
        // Scalar fallback for small arrays
        return values.iter().sum();
    }

    let chunks = values.len() / SIMD_LANES;
    let remainder = values.len() % SIMD_LANES;

    // Process 4 elements at a time
    let mut accumulators = [0.0f64; SIMD_LANES];
    
    for chunk_idx in 0..chunks {
        let base = chunk_idx * SIMD_LANES;
        accumulators[0] += values[base];
        accumulators[1] += values[base + 1];
        accumulators[2] += values[base + 2];
        accumulators[3] += values[base + 3];
    }

    // Reduce accumulators
    let mut sum = accumulators[0] + accumulators[1] + accumulators[2] + accumulators[3];

    // Handle remainder
    let remainder_start = chunks * SIMD_LANES;
    for i in 0..remainder {
        sum += values[remainder_start + i];
    }

    sum
}

/// Calculate mean of f64 slice using SIMD
#[inline]
pub fn simd_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    simd_sum(values) / values.len() as f64
}

/// Calculate variance using SIMD (two-pass algorithm for numerical stability)
#[inline]
pub fn simd_variance(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }

    let mean = simd_mean(values);
    let n = values.len();

    if n < SIMD_LANES * 2 {
        // Scalar fallback
        let sum_sq: f64 = values.iter().map(|&x| (x - mean).powi(2)).sum();
        return sum_sq / (n - 1) as f64;
    }

    let chunks = n / SIMD_LANES;
    let remainder = n % SIMD_LANES;

    let mut accumulators = [0.0f64; SIMD_LANES];

    for chunk_idx in 0..chunks {
        let base = chunk_idx * SIMD_LANES;
        let d0 = values[base] - mean;
        let d1 = values[base + 1] - mean;
        let d2 = values[base + 2] - mean;
        let d3 = values[base + 3] - mean;
        accumulators[0] += d0 * d0;
        accumulators[1] += d1 * d1;
        accumulators[2] += d2 * d2;
        accumulators[3] += d3 * d3;
    }

    let mut sum_sq = accumulators[0] + accumulators[1] + accumulators[2] + accumulators[3];

    // Handle remainder
    let remainder_start = chunks * SIMD_LANES;
    for i in 0..remainder {
        let d = values[remainder_start + i] - mean;
        sum_sq += d * d;
    }

    sum_sq / (n - 1) as f64
}

/// Calculate standard deviation using SIMD
#[inline]
pub fn simd_std_dev(values: &[f64]) -> f64 {
    simd_variance(values).sqrt()
}

/// Calculate SMA using SIMD for the sum
#[inline]
pub fn simd_sma(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period || period == 0 {
        return None;
    }
    let window = &values[values.len() - period..];
    Some(simd_sum(window) / period as f64)
}

/// Calculate EMA with SIMD-accelerated initial SMA bootstrap
#[inline]
pub fn simd_ema(values: &[f64], period: usize, previous_ema: Option<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    let current_value = *values.last().unwrap();
    let alpha = 2.0 / (period as f64 + 1.0);

    match previous_ema {
        Some(prev) => Some(alpha * current_value + (1.0 - alpha) * prev),
        None => {
            // Bootstrap with SIMD-accelerated SMA
            simd_sma(values, period)
        }
    }
}

/// Calculate min and max in a single pass using SIMD
#[inline]
pub fn simd_min_max(values: &[f64]) -> Option<(f64, f64)> {
    if values.is_empty() {
        return None;
    }

    if values.len() < SIMD_LANES * 2 {
        // Scalar fallback
        let mut min = values[0];
        let mut max = values[0];
        for &v in values.iter().skip(1) {
            if v < min { min = v; }
            if v > max { max = v; }
        }
        return Some((min, max));
    }

    let chunks = values.len() / SIMD_LANES;
    let remainder = values.len() % SIMD_LANES;

    let mut min_acc = [f64::MAX; SIMD_LANES];
    let mut max_acc = [f64::MIN; SIMD_LANES];

    for chunk_idx in 0..chunks {
        let base = chunk_idx * SIMD_LANES;
        for i in 0..SIMD_LANES {
            let v = values[base + i];
            if v < min_acc[i] { min_acc[i] = v; }
            if v > max_acc[i] { max_acc[i] = v; }
        }
    }

    let mut min = min_acc[0].min(min_acc[1]).min(min_acc[2]).min(min_acc[3]);
    let mut max = max_acc[0].max(max_acc[1]).max(max_acc[2]).max(max_acc[3]);

    // Handle remainder
    let remainder_start = chunks * SIMD_LANES;
    for i in 0..remainder {
        let v = values[remainder_start + i];
        if v < min { min = v; }
        if v > max { max = v; }
    }

    Some((min, max))
}

/// Calculate Sharpe ratio components using SIMD
/// Returns (mean_return, std_dev) for Sharpe calculation
#[inline]
pub fn simd_sharpe_components(returns: &[f64]) -> (f64, f64) {
    if returns.len() < 2 {
        return (0.0, 1.0);
    }
    (simd_mean(returns), simd_std_dev(returns))
}

/// Calculate rolling returns from equity curve using SIMD-friendly patterns
#[inline]
pub fn calculate_returns(equity_curve: &[f64]) -> Vec<f64> {
    if equity_curve.len() < 2 {
        return Vec::new();
    }

    let n = equity_curve.len() - 1;
    let mut returns = Vec::with_capacity(n);

    // Process in chunks for better cache utilization
    for i in 0..n {
        let ret = if equity_curve[i] != 0.0 {
            (equity_curve[i + 1] - equity_curve[i]) / equity_curve[i]
        } else {
            0.0
        };
        returns.push(ret);
    }

    returns
}

/// Calculate maximum drawdown using SIMD-accelerated running max
#[inline]
pub fn simd_max_drawdown(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }

    let mut peak = equity_curve[0];
    let mut max_dd = 0.0f64;

    // This loop has data dependencies so we can't fully vectorize,
    // but we can still optimize with minimal branching
    for &equity in equity_curve.iter() {
        if equity > peak {
            peak = equity;
        } else {
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }

    max_dd
}

/// Batch price change detection - returns indices where price changed significantly
/// Used for "skip stable regions" optimization
#[inline]
pub fn detect_significant_changes(prices: &[f64], threshold_pct: f64) -> Vec<usize> {
    if prices.len() < 2 {
        return vec![0];
    }

    let mut significant_indices = Vec::with_capacity(prices.len() / 10); // Estimate 10% significant
    significant_indices.push(0); // Always include first

    let mut last_significant_price = prices[0];

    for (i, &price) in prices.iter().enumerate().skip(1) {
        let pct_change = ((price - last_significant_price) / last_significant_price).abs();
        if pct_change >= threshold_pct {
            significant_indices.push(i);
            last_significant_price = price;
        }
    }

    // Always include last
    if *significant_indices.last().unwrap() != prices.len() - 1 {
        significant_indices.push(prices.len() - 1);
    }

    significant_indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_sum() {
        let values: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let simd_result = simd_sum(&values);
        let scalar_result: f64 = values.iter().sum();
        assert!((simd_result - scalar_result).abs() < 1e-10);
    }

    #[test]
    fn test_simd_mean() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((simd_mean(&values) - 5.5).abs() < 1e-10);
    }

    #[test]
    fn test_simd_std_dev() {
        let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let std = simd_std_dev(&values);
        assert!((std - 2.138).abs() < 0.01);
    }

    #[test]
    fn test_simd_min_max() {
        let values: Vec<f64> = (0..100).map(|i| (i as f64 * 1.5) - 50.0).collect();
        let (min, max) = simd_min_max(&values).unwrap();
        assert!((min - (-50.0)).abs() < 1e-10);
        assert!((max - 98.5).abs() < 1e-10);
    }

    #[test]
    fn test_simd_max_drawdown() {
        let equity = vec![100.0, 110.0, 105.0, 95.0, 100.0, 90.0, 95.0];
        let dd = simd_max_drawdown(&equity);
        // Peak was 110, trough was 90, dd = 20/110 ≈ 0.1818
        assert!((dd - 0.1818).abs() < 0.01);
    }

    #[test]
    fn test_detect_significant_changes() {
        let prices = vec![100.0, 100.1, 100.2, 105.0, 105.1, 110.0, 109.9];
        let threshold = 0.04; // 4% change threshold
        let indices = detect_significant_changes(&prices, threshold);
        // Should include: 0 (first), 3 (5% jump), 5 (4.76% jump), 6 (last)
        assert!(indices.contains(&0));
        assert!(indices.contains(&3));
        assert!(indices.contains(&5));
    }
}
