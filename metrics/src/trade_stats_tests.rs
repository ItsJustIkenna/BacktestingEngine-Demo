//! Unit tests for the trade statistics metrics module

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_rate() {
        let trades = [10.0, -5.0, 7.0, -2.0, 0.0];
        let win = win_rate(&trades);
        assert!(win.is_some());
        assert!((win.unwrap() - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_average_trade_duration() {
        let durations = [10.0, 20.0, 30.0];
        let avg = average_trade_duration(&durations);
        assert_eq!(avg, Some(20.0));
    }
}
