//! Unit tests for the risk metrics module

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_drawdown() {
        let equity = [100.0, 120.0, 110.0, 90.0, 130.0, 80.0];
        let mdd = max_drawdown(&equity);
        assert!(mdd.is_some());
        assert!((mdd.unwrap() - 0.3846).abs() < 0.01); // 50/130
    }

    #[test]
    fn test_volatility() {
        let returns = [0.01, 0.02, 0.015, 0.005, 0.0];
        let vol = volatility(&returns);
        assert!(vol.is_some());
    }
}
