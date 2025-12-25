//! Comprehensive tests for metrics calculations
//! Covers performance metrics, risk metrics, and trade statistics

use metrics::{
    sharpe_ratio, profit_factor, net_profit, gross_profit, gross_loss,
    average_trade_return, median_trade_return, number_of_trades,
    max_drawdown, volatility, win_rate, average_trade_duration,
    PerformanceMetrics,
};

// ============================================================================
// SHARPE RATIO TESTS
// ============================================================================

#[test]
fn test_sharpe_ratio_positive_returns() {
    // Consistent positive returns with low volatility = high Sharpe
    let returns = vec![0.01, 0.02, 0.015, 0.018, 0.012, 0.022, 0.011, 0.019];
    let risk_free_rate = 0.001;
    
    let sharpe = sharpe_ratio(&returns, risk_free_rate).unwrap();
    assert!(sharpe > 0.0, "Sharpe should be positive for consistent positive returns");
    assert!(sharpe > 2.0, "High positive returns with low vol should have high Sharpe");
}

#[test]
fn test_sharpe_ratio_negative_returns() {
    // Consistent negative returns = negative Sharpe
    let returns = vec![-0.01, -0.02, -0.015, -0.018, -0.012];
    let risk_free_rate = 0.001;
    
    let sharpe = sharpe_ratio(&returns, risk_free_rate).unwrap();
    assert!(sharpe < 0.0, "Sharpe should be negative for consistent losses");
}

#[test]
fn test_sharpe_ratio_mixed_returns() {
    // Mixed returns around zero
    let returns = vec![0.01, -0.01, 0.02, -0.02, 0.005, -0.005];
    let risk_free_rate = 0.0;
    
    let sharpe = sharpe_ratio(&returns, risk_free_rate).unwrap();
    // Mean is approximately 0, so Sharpe should be close to 0
    assert!(sharpe.abs() < 0.5, "Sharpe should be near zero for mean-zero returns");
}

#[test]
fn test_sharpe_ratio_empty_returns() {
    let returns: Vec<f64> = vec![];
    let result = sharpe_ratio(&returns, 0.01);
    assert!(result.is_none(), "Sharpe should return None for empty returns");
}

#[test]
fn test_sharpe_ratio_zero_volatility() {
    // All identical returns = zero stddev = None (undefined)
    let returns = vec![0.01, 0.01, 0.01, 0.01];
    let result = sharpe_ratio(&returns, 0.0);
    assert!(result.is_none(), "Sharpe should return None when volatility is zero");
}

#[test]
fn test_sharpe_ratio_single_return() {
    let returns = vec![0.05];
    // Single return has undefined std dev
    let result = sharpe_ratio(&returns, 0.01);
    // Depending on implementation, this may return None or compute with 1 element
    // Our implementation should handle gracefully
    assert!(result.is_none() || result.unwrap().is_finite());
}

#[test]
fn test_sharpe_ratio_with_risk_free_rate() {
    let returns = vec![0.02, 0.02, 0.02, 0.02, 0.02];
    
    // With risk_free_rate = 0.01, excess return = 0.01
    let sharpe_with_rf = sharpe_ratio(&returns, 0.01);
    // With risk_free_rate = 0.02, excess return = 0.00
    let sharpe_zero_excess = sharpe_ratio(&returns, 0.02);
    
    // This case has zero volatility, so both should be None
    assert!(sharpe_with_rf.is_none());
    assert!(sharpe_zero_excess.is_none());
}

// ============================================================================
// PROFIT FACTOR TESTS
// ============================================================================

#[test]
fn test_profit_factor_profitable_strategy() {
    let gross_p = 10000.0;
    let gross_l = -5000.0; // losses are negative
    
    let pf = profit_factor(gross_p, gross_l).unwrap();
    assert!((pf - 2.0).abs() < 0.001, "Profit factor should be 2.0");
}

#[test]
fn test_profit_factor_losing_strategy() {
    let gross_p = 3000.0;
    let gross_l = -9000.0;
    
    let pf = profit_factor(gross_p, gross_l).unwrap();
    assert!((pf - 0.333).abs() < 0.01, "Profit factor should be ~0.33");
}

#[test]
fn test_profit_factor_breakeven() {
    let gross_p = 5000.0;
    let gross_l = -5000.0;
    
    let pf = profit_factor(gross_p, gross_l).unwrap();
    assert!((pf - 1.0).abs() < 0.001, "Profit factor should be 1.0 at breakeven");
}

#[test]
fn test_profit_factor_zero_loss() {
    // No losses with profits = perfect profit factor (capped at 10.0)
    // Previously returned None which caused fitness overflow issues
    let gross_p = 5000.0;
    let gross_l = 0.0;
    
    let result = profit_factor(gross_p, gross_l);
    assert!(result.is_some(), "Profit factor should be Some(10.0) when there are no losses but profits exist");
    assert!((result.unwrap() - 10.0).abs() < 0.001, "Profit factor should be capped at 10.0 for all-wins case");
}

#[test]
fn test_profit_factor_zero_profit() {
    let gross_p = 0.0;
    let gross_l = -5000.0;
    
    let pf = profit_factor(gross_p, gross_l).unwrap();
    assert!((pf - 0.0).abs() < 0.001, "Profit factor should be 0 with no profits");
}

// ============================================================================
// NET PROFIT TESTS
// ============================================================================

#[test]
fn test_net_profit_positive() {
    let trade_returns = vec![100.0, -50.0, 200.0, -30.0, 150.0];
    let net = net_profit(&trade_returns);
    assert!((net - 370.0).abs() < 0.001, "Net profit should be 370");
}

#[test]
fn test_net_profit_negative() {
    let trade_returns = vec![-100.0, -50.0, 20.0, -30.0];
    let net = net_profit(&trade_returns);
    assert!((net - (-160.0)).abs() < 0.001, "Net profit should be -160");
}

#[test]
fn test_net_profit_empty() {
    let trade_returns: Vec<f64> = vec![];
    let net = net_profit(&trade_returns);
    assert!((net - 0.0).abs() < 0.001, "Net profit of empty trades should be 0");
}

#[test]
fn test_net_profit_single_trade() {
    let trade_returns = vec![500.0];
    let net = net_profit(&trade_returns);
    assert!((net - 500.0).abs() < 0.001, "Net profit should equal single trade");
}

// ============================================================================
// GROSS PROFIT/LOSS TESTS
// ============================================================================

#[test]
fn test_gross_profit_mixed_returns() {
    let trade_returns = vec![100.0, -50.0, 200.0, -30.0, 150.0];
    let gp = gross_profit(&trade_returns);
    assert!((gp - 450.0).abs() < 0.001, "Gross profit should be 450 (100+200+150)");
}

#[test]
fn test_gross_loss_mixed_returns() {
    let trade_returns = vec![100.0, -50.0, 200.0, -30.0, 150.0];
    let gl = gross_loss(&trade_returns);
    assert!((gl - (-80.0)).abs() < 0.001, "Gross loss should be -80 (-50-30)");
}

#[test]
fn test_gross_profit_all_losses() {
    let trade_returns = vec![-100.0, -50.0, -30.0];
    let gp = gross_profit(&trade_returns);
    assert!((gp - 0.0).abs() < 0.001, "Gross profit should be 0 when all losses");
}

#[test]
fn test_gross_loss_all_profits() {
    let trade_returns = vec![100.0, 50.0, 30.0];
    let gl = gross_loss(&trade_returns);
    assert!((gl - 0.0).abs() < 0.001, "Gross loss should be 0 when all profits");
}

// ============================================================================
// AVERAGE TRADE RETURN TESTS
// ============================================================================

#[test]
fn test_average_trade_return_basic() {
    let trade_returns = vec![10.0, 20.0, 30.0, 40.0];
    let avg = average_trade_return(&trade_returns).unwrap();
    assert!((avg - 25.0).abs() < 0.001, "Average should be 25");
}

#[test]
fn test_average_trade_return_mixed() {
    let trade_returns = vec![100.0, -50.0, 50.0];
    let avg = average_trade_return(&trade_returns).unwrap();
    assert!((avg - 33.333).abs() < 0.01, "Average should be ~33.33");
}

#[test]
fn test_average_trade_return_empty() {
    let trade_returns: Vec<f64> = vec![];
    let result = average_trade_return(&trade_returns);
    assert!(result.is_none(), "Average should be None for empty returns");
}

// ============================================================================
// MEDIAN TRADE RETURN TESTS
// ============================================================================

#[test]
fn test_median_trade_return_odd_count() {
    let trade_returns = vec![10.0, 30.0, 20.0, 50.0, 40.0];
    let median = median_trade_return(&trade_returns).unwrap();
    assert!((median - 30.0).abs() < 0.001, "Median of [10,20,30,40,50] should be 30");
}

#[test]
fn test_median_trade_return_even_count() {
    let trade_returns = vec![10.0, 20.0, 30.0, 40.0];
    let median = median_trade_return(&trade_returns).unwrap();
    assert!((median - 25.0).abs() < 0.001, "Median of [10,20,30,40] should be 25");
}

#[test]
fn test_median_trade_return_single() {
    let trade_returns = vec![42.0];
    let median = median_trade_return(&trade_returns).unwrap();
    assert!((median - 42.0).abs() < 0.001, "Median of single element should be that element");
}

#[test]
fn test_median_trade_return_empty() {
    let trade_returns: Vec<f64> = vec![];
    let result = median_trade_return(&trade_returns);
    assert!(result.is_none(), "Median should be None for empty returns");
}

#[test]
fn test_median_trade_return_with_negatives() {
    let trade_returns = vec![-50.0, -10.0, 0.0, 20.0, 100.0];
    let median = median_trade_return(&trade_returns).unwrap();
    assert!((median - 0.0).abs() < 0.001, "Median should be 0");
}

// ============================================================================
// NUMBER OF TRADES TEST
// ============================================================================

#[test]
fn test_number_of_trades() {
    let trade_returns = vec![10.0, 20.0, 30.0, -10.0, -20.0];
    let count = number_of_trades(&trade_returns);
    assert_eq!(count, 5, "Should count all trades");
}

#[test]
fn test_number_of_trades_empty() {
    let trade_returns: Vec<f64> = vec![];
    let count = number_of_trades(&trade_returns);
    assert_eq!(count, 0, "Empty trades should return 0");
}

// ============================================================================
// MAX DRAWDOWN TESTS
// ============================================================================

#[test]
fn test_max_drawdown_simple() {
    // Equity: 100 -> 110 -> 90 -> 95
    // Peak at 110, trough at 90, drawdown = (110-90)/110 = 18.18%
    let equity_curve = vec![100.0, 110.0, 90.0, 95.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    assert!((dd - 0.1818).abs() < 0.01, "Max drawdown should be ~18.18%");
}

#[test]
fn test_max_drawdown_no_drawdown() {
    // Monotonically increasing equity curve
    let equity_curve = vec![100.0, 110.0, 120.0, 130.0, 140.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    assert!((dd - 0.0).abs() < 0.001, "No drawdown for monotonic increase");
}

#[test]
fn test_max_drawdown_complete_loss() {
    // 100% loss scenario
    let equity_curve = vec![100.0, 50.0, 10.0, 0.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    assert!((dd - 1.0).abs() < 0.001, "Complete loss should be 100% drawdown");
}

#[test]
fn test_max_drawdown_recovery() {
    // Drawdown followed by recovery to new highs
    let equity_curve = vec![100.0, 120.0, 80.0, 90.0, 150.0, 130.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    // Max DD is from 120 to 80 = 33.33%, or from 150 to 130 = 13.33%
    // Max is 33.33%
    assert!((dd - 0.3333).abs() < 0.01, "Max drawdown should be ~33.33%");
}

#[test]
fn test_max_drawdown_empty() {
    let equity_curve: Vec<f64> = vec![];
    let result = max_drawdown(&equity_curve);
    assert!(result.is_none(), "Max drawdown should be None for empty curve");
}

#[test]
fn test_max_drawdown_single_point() {
    let equity_curve = vec![100.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    assert!((dd - 0.0).abs() < 0.001, "Single point should have 0 drawdown");
}

#[test]
fn test_max_drawdown_multiple_drawdowns() {
    // Multiple drawdowns, should find the largest
    let equity_curve = vec![100.0, 90.0, 95.0, 80.0, 85.0, 70.0, 75.0];
    let dd = max_drawdown(&equity_curve).unwrap();
    // From peak 100 to trough 70 = 30%
    assert!((dd - 0.30).abs() < 0.01, "Should find max 30% drawdown");
}

// ============================================================================
// VOLATILITY TESTS
// ============================================================================

#[test]
fn test_volatility_high() {
    // High volatility returns
    let returns = vec![0.10, -0.08, 0.12, -0.10, 0.15, -0.12];
    let vol = volatility(&returns).unwrap();
    assert!(vol > 0.10, "High swing returns should have high volatility");
}

#[test]
fn test_volatility_low() {
    // Low volatility returns
    let returns = vec![0.01, 0.012, 0.011, 0.009, 0.010, 0.011];
    let vol = volatility(&returns).unwrap();
    assert!(vol < 0.005, "Stable returns should have low volatility");
}

#[test]
fn test_volatility_zero() {
    // All identical returns
    let returns = vec![0.02, 0.02, 0.02, 0.02];
    let vol = volatility(&returns).unwrap();
    assert!((vol - 0.0).abs() < 0.0001, "Identical returns should have zero volatility");
}

#[test]
fn test_volatility_empty() {
    let returns: Vec<f64> = vec![];
    let result = volatility(&returns);
    assert!(result.is_none(), "Volatility should be None for empty returns");
}

#[test]
fn test_volatility_single_value() {
    let returns = vec![0.05];
    let vol = volatility(&returns).unwrap();
    assert!((vol - 0.0).abs() < 0.0001, "Single value should have zero volatility");
}

// ============================================================================
// WIN RATE TESTS
// ============================================================================

#[test]
fn test_win_rate_all_wins() {
    let trade_returns = vec![10.0, 20.0, 5.0, 15.0];
    let wr = win_rate(&trade_returns).unwrap();
    assert!((wr - 1.0).abs() < 0.001, "All winning trades should have 100% win rate");
}

#[test]
fn test_win_rate_all_losses() {
    let trade_returns = vec![-10.0, -20.0, -5.0];
    let wr = win_rate(&trade_returns).unwrap();
    assert!((wr - 0.0).abs() < 0.001, "All losing trades should have 0% win rate");
}

#[test]
fn test_win_rate_mixed() {
    let trade_returns = vec![10.0, -5.0, 20.0, -10.0, 15.0]; // 3 wins, 2 losses
    let wr = win_rate(&trade_returns).unwrap();
    assert!((wr - 0.60).abs() < 0.001, "3/5 wins should be 60% win rate");
}

#[test]
fn test_win_rate_with_zero_returns() {
    // Zero returns are not wins
    let trade_returns = vec![10.0, 0.0, -5.0, 0.0, 15.0]; // 2 wins, 1 loss, 2 zeros
    let wr = win_rate(&trade_returns).unwrap();
    assert!((wr - 0.40).abs() < 0.001, "2/5 wins should be 40% win rate");
}

#[test]
fn test_win_rate_empty() {
    let trade_returns: Vec<f64> = vec![];
    let result = win_rate(&trade_returns);
    assert!(result.is_none(), "Win rate should be None for empty trades");
}

// ============================================================================
// AVERAGE TRADE DURATION TESTS
// ============================================================================

#[test]
fn test_average_trade_duration_basic() {
    let durations = vec![3600.0, 7200.0, 1800.0, 5400.0]; // in seconds
    let avg = average_trade_duration(&durations).unwrap();
    assert!((avg - 4500.0).abs() < 0.001, "Average duration should be 4500 seconds");
}

#[test]
fn test_average_trade_duration_empty() {
    let durations: Vec<f64> = vec![];
    let result = average_trade_duration(&durations);
    assert!(result.is_none(), "Average duration should be None for empty durations");
}

#[test]
fn test_average_trade_duration_single() {
    let durations = vec![3600.0];
    let avg = average_trade_duration(&durations).unwrap();
    assert!((avg - 3600.0).abs() < 0.001, "Single duration should equal itself");
}

// ============================================================================
// PERFORMANCE METRICS STRUCT TESTS
// ============================================================================

#[test]
fn test_performance_metrics_from_trade_data() {
    let trade_returns = vec![0.02, -0.01, 0.03, 0.01, -0.005, 0.015];
    let equity_curve = vec![10000.0, 10200.0, 10100.0, 10400.0, 10500.0, 10450.0, 10600.0];
    let trade_durations = vec![3600.0, 7200.0, 5400.0, 3600.0, 1800.0, 4500.0];
    let risk_free_rate = 0.001;
    
    let metrics = PerformanceMetrics::from_trade_data(
        &trade_returns,
        &equity_curve,
        &trade_durations,
        risk_free_rate,
    );
    
    // Verify all fields are populated
    assert!(metrics.sharpe_ratio.is_finite());
    assert!(metrics.profit_factor.is_finite());
    // Net profit = sum of returns: 0.02 - 0.01 + 0.03 + 0.01 - 0.005 + 0.015 = 0.06
    // Use slightly larger tolerance for floating point
    assert!((metrics.net_profit - 0.06).abs() < 0.001, 
        "Expected net_profit ~0.06, got {}", metrics.net_profit);
    assert!(metrics.gross_profit > 0.0);
    assert!(metrics.gross_loss < 0.0);
    assert!(metrics.average_trade_return.is_finite());
    assert!(metrics.median_trade_return.is_finite());
    assert_eq!(metrics.number_of_trades, 6);
    assert!(metrics.max_drawdown >= 0.0);
    assert!(metrics.volatility >= 0.0);
    assert!(metrics.win_rate >= 0.0 && metrics.win_rate <= 1.0);
    assert!(metrics.average_trade_duration > 0.0);
}

#[test]
fn test_performance_metrics_default() {
    let metrics = PerformanceMetrics::default();
    
    assert!((metrics.sharpe_ratio - 0.0).abs() < 0.001);
    assert!((metrics.profit_factor - 0.0).abs() < 0.001);
    assert!((metrics.net_profit - 0.0).abs() < 0.001);
    assert_eq!(metrics.number_of_trades, 0);
}

#[test]
fn test_performance_metrics_from_empty_data() {
    let trade_returns: Vec<f64> = vec![];
    let equity_curve: Vec<f64> = vec![];
    let trade_durations: Vec<f64> = vec![];
    let risk_free_rate = 0.01;
    
    let metrics = PerformanceMetrics::from_trade_data(
        &trade_returns,
        &equity_curve,
        &trade_durations,
        risk_free_rate,
    );
    
    // Should handle empty data gracefully with default values
    assert!((metrics.net_profit - 0.0).abs() < 0.001);
    assert_eq!(metrics.number_of_trades, 0);
}

#[test]
fn test_performance_metrics_transaction_costs() {
    let mut metrics = PerformanceMetrics::default();
    metrics.total_commission = 100.0;
    metrics.total_slippage = 50.0;
    metrics.total_market_impact = 25.0;
    
    let total_costs = metrics.total_commission + metrics.total_slippage + metrics.total_market_impact;
    assert!((total_costs - 175.0).abs() < 0.001);
}
