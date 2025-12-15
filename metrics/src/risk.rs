//! Risk metrics for market making strategies
//!
//! Includes maximum drawdown, volatility, Sortino ratio, and Calmar ratio.
//! Uses SIMD-friendly patterns for improved performance on large datasets.

/// SIMD lane width for vectorized operations
const SIMD_LANES: usize = 4;

/// Calculates the maximum drawdown from a list of equity values.
/// Returns the maximum drawdown as a decimal (0.0 to 1.0).
/// If equity goes negative (which indicates a bug), clamps to 1.0 (100% loss).
pub fn max_drawdown(equity_curve: &[f64]) -> Option<f64> {
	if equity_curve.is_empty() {
		return None;
	}
	let mut max_drawdown = 0.0;
	let mut peak = equity_curve[0];
	for &value in equity_curve {
		if value > peak {
			peak = value;
		}
		// Guard against division by zero or negative peak
		if peak <= 0.0 {
			continue;
		}
		let drawdown = (peak - value) / peak;
		// Clamp drawdown to valid range [0.0, 1.0]
		let drawdown = drawdown.clamp(0.0, 1.0);
		if drawdown > max_drawdown {
			max_drawdown = drawdown;
		}
	}
	Some(max_drawdown)
}

/// Calculates the volatility (standard deviation of returns).
/// Uses SIMD-accelerated computation for large datasets.
pub fn volatility(returns: &[f64]) -> Option<f64> {
	if returns.is_empty() {
		return None;
	}
	simd_std_dev(returns)
}

/// Calculates the downside deviation (standard deviation of negative returns only).
/// Used in Sortino ratio calculation.
pub fn downside_deviation(returns: &[f64], target_return: f64) -> Option<f64> {
	if returns.is_empty() {
		return None;
	}
	
	let downside_returns: Vec<f64> = returns
		.iter()
		.filter(|&&r| r < target_return)
		.map(|&r| (r - target_return).powi(2))
		.collect();
	
	if downside_returns.is_empty() {
		return Some(0.0); // No downside returns
	}
	
	let sum: f64 = downside_returns.iter().sum();
	Some((sum / returns.len() as f64).sqrt())
}

/// Calculates the Sortino ratio.
/// Sortino = (Mean Return - Risk Free Rate) / Downside Deviation
/// Unlike Sharpe, only penalizes downside volatility.
pub fn sortino_ratio(returns: &[f64], risk_free_rate: f64) -> Option<f64> {
	if returns.is_empty() {
		return None;
	}
	
	let mean_return = mean(returns)?;
	let dd = downside_deviation(returns, risk_free_rate)?;
	
	if dd == 0.0 {
		// No downside deviation - if positive returns, infinite ratio; else undefined
		if mean_return > risk_free_rate {
			return Some(f64::MAX);
		}
		return None;
	}
	
	Some((mean_return - risk_free_rate) / dd)
}

/// Calculates the Calmar ratio.
/// Calmar = Annualized Return / Maximum Drawdown
/// Measures return relative to worst-case drawdown.
pub fn calmar_ratio(annualized_return: f64, max_drawdown: f64) -> Option<f64> {
	if max_drawdown == 0.0 {
		if annualized_return > 0.0 {
			return Some(f64::MAX);
		}
		return None;
	}
	
	Some(annualized_return / max_drawdown)
}

/// Calculates the current drawdown from peak.
pub fn current_drawdown(equity_curve: &[f64]) -> Option<f64> {
	if equity_curve.is_empty() {
		return None;
	}
	
	let peak = equity_curve.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
	let current = *equity_curve.last()?;
	
	if peak <= 0.0 {
		return None;
	}
	
	Some(((peak - current) / peak).clamp(0.0, 1.0))
}

/// Calculates the average drawdown over the equity curve.
pub fn average_drawdown(equity_curve: &[f64]) -> Option<f64> {
	if equity_curve.is_empty() {
		return None;
	}
	
	let mut peak = equity_curve[0];
	let mut total_dd = 0.0;
	let mut count = 0;
	
	for &value in equity_curve {
		if value > peak {
			peak = value;
		}
		if peak > 0.0 {
			let dd = (peak - value) / peak;
			total_dd += dd.clamp(0.0, 1.0);
			count += 1;
		}
	}
	
	if count == 0 {
		return None;
	}
	
	Some(total_dd / count as f64)
}

// ============================================================================
// SIMD-accelerated helper functions
// ============================================================================

/// SIMD-accelerated sum
#[inline]
fn simd_sum(values: &[f64]) -> f64 {
	if values.len() < SIMD_LANES * 2 {
		return values.iter().sum();
	}

	let chunks = values.len() / SIMD_LANES;
	let remainder = values.len() % SIMD_LANES;

	let mut accumulators = [0.0f64; SIMD_LANES];
	
	for chunk_idx in 0..chunks {
		let base = chunk_idx * SIMD_LANES;
		accumulators[0] += values[base];
		accumulators[1] += values[base + 1];
		accumulators[2] += values[base + 2];
		accumulators[3] += values[base + 3];
	}

	let mut sum = accumulators[0] + accumulators[1] + accumulators[2] + accumulators[3];

	let remainder_start = chunks * SIMD_LANES;
	for i in 0..remainder {
		sum += values[remainder_start + i];
	}

	sum
}

/// SIMD-accelerated mean
#[inline]
fn simd_mean(data: &[f64]) -> Option<f64> {
	if data.is_empty() {
		return None;
	}
	Some(simd_sum(data) / data.len() as f64)
}

/// SIMD-accelerated standard deviation
#[inline]
fn simd_std_dev(data: &[f64]) -> Option<f64> {
	if data.is_empty() {
		return None;
	}
	if data.len() == 1 {
		// Single value has zero volatility
		return Some(0.0);
	}

	let mean = simd_mean(data)?;
	let n = data.len();

	if n < SIMD_LANES * 2 {
		let var = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
		return Some(var.sqrt());
	}

	let chunks = n / SIMD_LANES;
	let remainder = n % SIMD_LANES;

	let mut accumulators = [0.0f64; SIMD_LANES];

	for chunk_idx in 0..chunks {
		let base = chunk_idx * SIMD_LANES;
		let d0 = data[base] - mean;
		let d1 = data[base + 1] - mean;
		let d2 = data[base + 2] - mean;
		let d3 = data[base + 3] - mean;
		accumulators[0] += d0 * d0;
		accumulators[1] += d1 * d1;
		accumulators[2] += d2 * d2;
		accumulators[3] += d3 * d3;
	}

	let mut sum_sq = accumulators[0] + accumulators[1] + accumulators[2] + accumulators[3];

	let remainder_start = chunks * SIMD_LANES;
	for i in 0..remainder {
		let d = data[remainder_start + i] - mean;
		sum_sq += d * d;
	}

	Some((sum_sq / n as f64).sqrt())
}

/// Legacy helper for compatibility (now uses SIMD internally)
fn mean(data: &[f64]) -> Option<f64> {
	simd_mean(data)
}

