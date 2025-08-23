//! Performance metrics for market making strategies
//
// Includes Sharpe ratio, profit factor, net/gross profit, average/median trade return, number of trades.

/// Calculates the Sharpe ratio given returns and risk-free rate.
pub fn sharpe_ratio(returns: &[f64], risk_free_rate: f64) -> Option<f64> {
	if returns.is_empty() {
		return None;
	}
	let mean_return = mean(returns)?;
	let stddev = std_dev(returns)?;
	if stddev == 0.0 {
		return None;
	}
	Some((mean_return - risk_free_rate) / stddev)
}

/// Calculates the profit factor given gross profit and gross loss.
pub fn profit_factor(gross_profit: f64, gross_loss: f64) -> Option<f64> {
	if gross_loss.abs() < std::f64::EPSILON {
		return None;
	}
	Some(gross_profit / gross_loss.abs())
}

/// Calculates net profit from a list of trade returns.
pub fn net_profit(trade_returns: &[f64]) -> f64 {
	trade_returns.iter().sum()
}

/// Calculates gross profit from a list of trade returns.
pub fn gross_profit(trade_returns: &[f64]) -> f64 {
	trade_returns.iter().filter(|&&x| x > 0.0).sum()
}

/// Calculates gross loss from a list of trade returns.
pub fn gross_loss(trade_returns: &[f64]) -> f64 {
	trade_returns.iter().filter(|&&x| x < 0.0).sum()
}

/// Calculates the average trade return.
pub fn average_trade_return(trade_returns: &[f64]) -> Option<f64> {
	mean(trade_returns)
}

/// Calculates the median trade return.
pub fn median_trade_return(trade_returns: &[f64]) -> Option<f64> {
	if trade_returns.is_empty() {
		return None;
	}
	let mut sorted = trade_returns.to_vec();
	sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
	let mid = sorted.len() / 2;
	if sorted.len() % 2 == 0 {
		Some((sorted[mid - 1] + sorted[mid]) / 2.0)
	} else {
		Some(sorted[mid])
	}
}

/// Returns the number of trades.
pub fn number_of_trades(trade_returns: &[f64]) -> usize {
	trade_returns.len()
}

/// Helper: mean of a slice
fn mean(data: &[f64]) -> Option<f64> {
	if data.is_empty() {
		return None;
	}
	Some(data.iter().sum::<f64>() / data.len() as f64)
}

/// Helper: standard deviation of a slice
fn std_dev(data: &[f64]) -> Option<f64> {
	let mean = mean(data)?;
	let var = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / data.len() as f64;
	Some(var.sqrt())
}

