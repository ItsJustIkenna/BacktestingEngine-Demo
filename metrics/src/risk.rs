//! Risk metrics for market making strategies
//
// Includes maximum drawdown, volatility.

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
pub fn volatility(returns: &[f64]) -> Option<f64> {
	if returns.is_empty() {
		return None;
	}
	let mean = mean(returns)?;
	let var = returns.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / returns.len() as f64;
	Some(var.sqrt())
}

fn mean(data: &[f64]) -> Option<f64> {
	if data.is_empty() {
		return None;
	}
	Some(data.iter().sum::<f64>() / data.len() as f64)
}

