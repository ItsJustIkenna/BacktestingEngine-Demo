//! Trade statistics for market making strategies
//
// Includes win rate, average trade duration.

/// Calculates the win rate from a list of trade returns.
pub fn win_rate(trade_returns: &[f64]) -> Option<f64> {
	if trade_returns.is_empty() {
		return None;
	}
	let wins = trade_returns.iter().filter(|&&x| x > 0.0).count();
	Some(wins as f64 / trade_returns.len() as f64)
}

/// Calculates the average trade duration from a list of trade durations (in seconds).
pub fn average_trade_duration(trade_durations: &[f64]) -> Option<f64> {
	if trade_durations.is_empty() {
		return None;
	}
	Some(trade_durations.iter().sum::<f64>() / trade_durations.len() as f64)
}

