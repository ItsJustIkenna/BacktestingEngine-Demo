use walkforward::{WalkForwardConfig, WalkForwardAnalyzer, TradingMetrics};
use chrono::{Utc, TimeZone};

#[tokio::test]
async fn test_walk_forward_config_creation() {
    let start_date = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let end_date = Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap();
    
    let config = WalkForwardConfig::new(
        start_date,
        end_date,
        std::time::Duration::from_secs(30 * 24 * 3600), // 30 days
        std::time::Duration::from_secs(10 * 24 * 3600), // 10 days
        5
    );
    
    assert_eq!(config.start_date, start_date);
    assert_eq!(config.end_date, end_date);
    assert_eq!(config.optimization_window_size, std::time::Duration::from_secs(30 * 24 * 3600));
    assert_eq!(config.test_window_size, std::time::Duration::from_secs(10 * 24 * 3600));
    assert_eq!(config.max_parameters, 5);
}

#[tokio::test]
async fn test_walk_forward_analyzer_creation() {
    let start_date = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let end_date = Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap();
    
    let config = WalkForwardConfig::new(
        start_date,
        end_date,
        std::time::Duration::from_secs(30 * 24 * 3600),
        std::time::Duration::from_secs(10 * 24 * 3600),
        5
    );
    
    let backtest_config = config::BacktestConfig::default();
    let analyzer = WalkForwardAnalyzer::new(config, backtest_config);
    
    assert_eq!(analyzer.config.max_parameters, 5);
}

#[tokio::test]
async fn test_trading_metrics_creation() {
    let metrics = TradingMetrics::new();
    
    assert_eq!(metrics.total_return, 0.0);
    assert_eq!(metrics.max_drawdown, 0.0);
    assert_eq!(metrics.sharpe_ratio, 0.0);
    assert_eq!(metrics.win_rate, 0.0);
    assert_eq!(metrics.total_trades, 0);
}

#[tokio::test]
async fn test_window_generation() {
    let start_date = Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
    let end_date = Utc.with_ymd_and_hms(2023, 6, 1, 0, 0, 0).unwrap();
    
    let config = WalkForwardConfig::new(
        start_date,
        end_date,
        std::time::Duration::from_secs(60 * 24 * 3600), // 60 days
        std::time::Duration::from_secs(30 * 24 * 3600), // 30 days
        3
    );
    
    let backtest_config = config::BacktestConfig::default();
    let analyzer = WalkForwardAnalyzer::new(config, backtest_config);
    let windows = analyzer.generate_windows();
    
    // Should generate at least one window
    assert!(!windows.is_empty());
    
    // Verify window structure
    for window in &windows {
        assert!(window.optimization_window.start_date < window.optimization_window.end_date);
        assert!(window.test_window.start_date < window.test_window.end_date);
        assert!(window.optimization_window.end_date <= window.test_window.start_date);
    }
}

#[tokio::test]
async fn test_serialization() {
    let metrics = TradingMetrics {
        total_return: 100.0,
        max_drawdown: 10.0,
        sharpe_ratio: 1.5,
        win_rate: 0.6,
        profit_factor: 1.8,
        total_trades: 50,
        avg_trade_duration_hours: 24.0,
        volatility: 0.15,
    };
    
    // Test JSON serialization
    let json = serde_json::to_string(&metrics).unwrap();
    let deserialized: TradingMetrics = serde_json::from_str(&json).unwrap();
    
    assert_eq!(metrics.total_return, deserialized.total_return);
    assert_eq!(metrics.max_drawdown, deserialized.max_drawdown);
    assert_eq!(metrics.sharpe_ratio, deserialized.sharpe_ratio);
    assert_eq!(metrics.win_rate, deserialized.win_rate);
    assert_eq!(metrics.total_trades, deserialized.total_trades);
}
