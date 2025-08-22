//! Unit tests for the performance metrics module

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sharpe_ratio() {
        let returns = [0.01, 0.02, 0.015, 0.005, 0.0];
        let risk_free = 0.001;
        let ratio = sharpe_ratio(&returns, risk_free);
        assert!(ratio.is_some());
    }

    #[test]
    fn test_profit_factor() {
        let pf = profit_factor(100.0, -50.0);
        assert_eq!(pf, Some(2.0));
    }

    #[test]
    fn test_net_profit() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(net_profit(&trades), 12.0);
    }

    #[test]
    fn test_gross_profit() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(gross_profit(&trades), 17.0);
    }

    #[test]
    fn test_gross_loss() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(gross_loss(&trades), -5.0);
    }

    #[test]
    fn test_average_trade_return() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(average_trade_return(&trades), Some(4.0));
    }

    #[test]
    fn test_median_trade_return() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(median_trade_return(&trades), Some(7.0));
        let trades_even = [10.0, -5.0, 7.0, 2.0];
        assert_eq!(median_trade_return(&trades_even), Some((2.0+7.0)/2.0));
    }

    #[test]
    fn test_number_of_trades() {
        let trades = [10.0, -5.0, 7.0];
        assert_eq!(number_of_trades(&trades), 3);
    }
}
