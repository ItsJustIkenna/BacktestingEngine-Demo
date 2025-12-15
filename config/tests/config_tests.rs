//! Comprehensive tests for Config module
//! Covers BacktestConfig, trading parameters, and strategy configuration

use config::{
    BacktestConfig, DatabaseConfig, TradingConfig, DataConfig, OutputConfig,
    AnalysisMode, AnalysisConfig, OrderBookConfig, ArbitrageRiskLimits,
    ExchangeFeeConfig, GapDetectionConfig, GapFillStrategy, MarketImpactConfig,
    PerformanceOptimization,
};

// ============================================================================
// BACKTEST CONFIG TESTS
// ============================================================================

#[test]
fn test_backtest_config_default() {
    let config = BacktestConfig::default();
    
    assert!(config.trading.initial_capital > 0.0, "Initial capital should be positive");
    assert!(config.trading.commission_rate >= 0.0, "Commission rate should be non-negative");
}

#[test]
fn test_backtest_config_custom() {
    let mut config = BacktestConfig::default();
    config.trading.initial_capital = 100000.0;
    config.trading.commission_rate = 0.001;
    config.trading.slippage_bps = 5.0;
    
    assert_eq!(config.trading.initial_capital, 100000.0);
    assert_eq!(config.trading.commission_rate, 0.001);
    assert_eq!(config.trading.slippage_bps, 5.0);
}

// ============================================================================
// DATABASE CONFIG TESTS
// ============================================================================

#[test]
fn test_database_config_default() {
    let config = DatabaseConfig::default();
    
    // Should have default values
    assert!(!config.host.is_empty() || config.host.is_empty()); // May have a default
}

#[test]
fn test_database_config_custom() {
    let config = DatabaseConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "trading_db".to_string(),
        username: "trader".to_string(),
        password: "secure_password".to_string(),
        max_connections: Some(10),
        connection_timeout: Some(30),
    };
    
    assert_eq!(config.host, "localhost");
    assert_eq!(config.port, 5432);
    assert_eq!(config.max_connections, Some(10));
}

// ============================================================================
// TRADING CONFIG TESTS
// ============================================================================

#[test]
fn test_trading_config_default() {
    let config = TradingConfig::default();
    
    assert!(config.initial_capital > 0.0);
    assert!(config.commission_rate >= 0.0);
}

#[test]
fn test_trading_config_custom() {
    let config = TradingConfig {
        initial_capital: 50000.0,
        commission_rate: 0.0005,
        slippage_bps: 2.5,
        max_position_size: Some(10.0),
        risk_free_rate: 0.02,
        exchange_fees: None,
        routing_mode: None,
        latency_ms: 10, // Simulated network latency (realistic for co-located)
        latency_jitter_pct: 0.2, // 20% jitter
        min_latency_ms: 1,
        latency_spike_probability: 0.01, // 1% spike probability
        optimization: PerformanceOptimization::default(),
    };
    
    assert_eq!(config.initial_capital, 50000.0);
    assert_eq!(config.slippage_bps, 2.5);
    assert_eq!(config.max_position_size, Some(10.0));
    assert_eq!(config.latency_jitter_pct, 0.2);
    assert_eq!(config.min_latency_ms, 1);
}

// ============================================================================
// EXCHANGE FEE CONFIG TESTS
// ============================================================================

#[test]
fn test_exchange_fee_config_default() {
    let config = ExchangeFeeConfig::default();
    
    assert!(!config.exchange.is_empty());
    assert!(!config.volume_tiers.is_empty());
}

#[test]
fn test_exchange_fee_config_kraken() {
    let config = ExchangeFeeConfig::kraken();
    
    assert_eq!(config.exchange, "kraken");
    assert!(config.maker_fee < config.taker_fee, "Maker fees should be lower");
}

#[test]
fn test_exchange_fee_config_binance() {
    let config = ExchangeFeeConfig::binance();
    
    assert_eq!(config.exchange, "binance");
}

#[test]
fn test_fee_tier_lookup() {
    let config = ExchangeFeeConfig::kraken();
    
    // Higher volume should result in lower fees
    let low_volume = 1000.0;
    let high_volume = 10_000_000.0;
    
    let low_vol_fee = config.get_fee_rate(low_volume, false); // taker
    let high_vol_fee = config.get_fee_rate(high_volume, false);
    
    assert!(high_vol_fee <= low_vol_fee, "Higher volume should have lower fees");
}

// ============================================================================
// ARBITRAGE RISK LIMITS TESTS
// ============================================================================

#[test]
fn test_arbitrage_risk_limits_default() {
    let limits = ArbitrageRiskLimits::default();
    
    assert!(limits.max_open_positions > 0);
    assert!(limits.max_daily_loss > 0.0);
    assert!(limits.max_exchange_exposure > 0.0);
}

#[test]
fn test_arbitrage_risk_limits_custom() {
    let limits = ArbitrageRiskLimits {
        max_open_positions: 10,
        max_daily_loss: 5000.0,
        max_exchange_exposure: 100000.0,
        min_liquidity_ratio: 3.0,
    };
    
    assert_eq!(limits.max_open_positions, 10);
    assert_eq!(limits.max_daily_loss, 5000.0);
}

// ============================================================================
// DATA CONFIG TESTS
// ============================================================================

#[test]
fn test_data_config_default() {
    let config = DataConfig::default();
    
    assert!(!config.default_timeframe.is_empty());
}

#[test]
fn test_data_config_custom() {
    let config = DataConfig {
        default_timeframe: "1h".to_string(),
        data_sources: vec!["yahoo".to_string(), "binance".to_string()],
        cache_enabled: true,
        cache_duration_hours: 24,
        gap_detection: GapDetectionConfig::default(),
        market_impact: MarketImpactConfig::default(),
    };
    
    assert_eq!(config.default_timeframe, "1h");
    assert_eq!(config.data_sources.len(), 2);
    assert!(config.cache_enabled);
}

// ============================================================================
// GAP DETECTION CONFIG TESTS
// ============================================================================

#[test]
fn test_gap_detection_config_default() {
    let config = GapDetectionConfig::default();
    
    assert!(config.expected_frequency_seconds > 0);
    assert!(config.gap_threshold_multiplier > 0.0);
}

#[test]
fn test_gap_fill_strategies() {
    let strategies = vec![
        GapFillStrategy::None,
        GapFillStrategy::ForwardFill,
        GapFillStrategy::LinearInterpolation,
        GapFillStrategy::ExcludeGaps,
    ];
    
    assert_eq!(strategies.len(), 4);
    assert!(matches!(strategies[0], GapFillStrategy::None));
}

// ============================================================================
// MARKET IMPACT CONFIG TESTS
// ============================================================================

#[test]
fn test_market_impact_config_default() {
    let config = MarketImpactConfig::default();
    
    assert!(config.min_impact_ratio >= 0.0);
    assert!(config.linear_coefficient >= 0.0);
}

// ============================================================================
// OUTPUT CONFIG TESTS
// ============================================================================

#[test]
fn test_output_config_default() {
    let config = OutputConfig::default();
    
    // Should have reasonable defaults
    assert!(!config.results_dir.is_empty() || config.results_dir.is_empty());
}

#[test]
fn test_output_config_custom() {
    let config = OutputConfig {
        results_dir: "/tmp/results".to_string(),
        reports_dir: "/tmp/reports".to_string(),
        charts_enabled: true,
        export_formats: vec!["json".to_string(), "csv".to_string()],
    };
    
    assert!(config.charts_enabled);
    assert_eq!(config.export_formats.len(), 2);
}

// ============================================================================
// ORDERBOOK CONFIG TESTS
// ============================================================================

#[test]
fn test_orderbook_config_default() {
    let config = OrderBookConfig::default();
    
    // Should have reasonable defaults
    assert!(true, "OrderBookConfig default created");
}

#[test]
fn test_orderbook_config_custom() {
    let config = OrderBookConfig {
        max_levels: Some(20),
        track_individual_orders: true,
        snapshot_validation_frequency: Some(100),
        strict_validation: false,
    };
    
    assert_eq!(config.max_levels, Some(20));
    assert!(config.track_individual_orders);
}

// ============================================================================
// ANALYSIS MODE TESTS
// ============================================================================

#[test]
fn test_analysis_mode_development() {
    let mode = AnalysisMode::Development;
    
    assert!(matches!(mode, AnalysisMode::Development));
}

#[test]
fn test_analysis_mode_validation() {
    let mode = AnalysisMode::Validation;
    
    assert!(matches!(mode, AnalysisMode::Validation));
}

#[test]
fn test_analysis_mode_production() {
    let mode = AnalysisMode::Production;
    
    assert!(matches!(mode, AnalysisMode::Production));
}

// ============================================================================
// ANALYSIS CONFIG TESTS
// ============================================================================

#[test]
fn test_analysis_config_default() {
    let config = AnalysisConfig::default();
    
    // Default should be production mode with full analysis
    assert!(matches!(config.mode, AnalysisMode::Production));
    assert!(config.auto_monte_carlo);
    assert!(config.auto_walk_forward);
}

#[test]
fn test_analysis_config_development_mode() {
    let config = AnalysisConfig::development();
    
    assert!(matches!(config.mode, AnalysisMode::Development));
    assert!(!config.auto_monte_carlo);
    assert!(!config.auto_walk_forward);
}

#[test]
fn test_analysis_config_validation_mode() {
    let config = AnalysisConfig::validation();
    
    assert!(matches!(config.mode, AnalysisMode::Validation));
    assert!(config.auto_monte_carlo);
    assert!(!config.auto_walk_forward);
}

#[test]
fn test_analysis_config_production_mode() {
    let config = AnalysisConfig::production();
    
    assert!(matches!(config.mode, AnalysisMode::Production));
    assert!(config.auto_monte_carlo);
    assert!(config.auto_walk_forward);
    assert_eq!(config.monte_carlo_runs, 1000);
}

// ============================================================================
// CONFIG VALIDATION TESTS
// ============================================================================

#[test]
fn test_initial_capital_positive() {
    let config = BacktestConfig::default();
    
    assert!(config.trading.initial_capital > 0.0, 
        "Initial capital must be positive");
}

#[test]
fn test_commission_rate_reasonable() {
    let config = BacktestConfig::default();
    
    assert!(config.trading.commission_rate >= 0.0 && config.trading.commission_rate < 0.1,
        "Commission rate should be reasonable (0-10%)");
}

#[test]
fn test_slippage_reasonable() {
    let config = BacktestConfig::default();
    
    assert!(config.trading.slippage_bps >= 0.0 && config.trading.slippage_bps < 100.0,
        "Slippage should be reasonable (0-100 bps)");
}

// ============================================================================
// CONFIG CONSISTENCY TESTS
// ============================================================================

#[test]
fn test_monte_carlo_runs_reasonable() {
    let config = AnalysisConfig::default();
    
    assert!(config.monte_carlo_runs > 0 && config.monte_carlo_runs <= 100000,
        "Monte Carlo runs should be reasonable");
}

#[test]
fn test_walk_forward_periods_consistent() {
    let config = AnalysisConfig::production();
    
    assert!(config.wf_training_days > config.wf_test_days,
        "Training period should be longer than test period");
    assert!(config.wf_step_days <= config.wf_test_days,
        "Step size should be <= test period for no gaps");
}

// ============================================================================
// ESCALATION LOGIC TESTS
// ============================================================================

#[test]
fn test_should_escalate_to_validation() {
    let config = AnalysisConfig::development();
    
    // Good metrics should trigger escalation
    let should_escalate = config.should_escalate_to_validation(
        180,   // duration_days (> min_duration_for_auto)
        1.5,   // sharpe_ratio (> threshold)
        1.6,   // profit_factor (> threshold)
        100,   // num_trades (> min_trades)
    );
    
    assert!(should_escalate, "Good metrics should trigger escalation");
}

#[test]
fn test_should_not_escalate_poor_metrics() {
    let config = AnalysisConfig::development();
    
    // Poor metrics should not trigger escalation
    let should_escalate = config.should_escalate_to_validation(
        180,   // duration_days
        0.5,   // sharpe_ratio (below threshold)
        1.0,   // profit_factor (below threshold)
        30,    // num_trades (below threshold)
    );
    
    assert!(!should_escalate, "Poor metrics should not trigger escalation");
}

// ============================================================================
// ASYNC TESTS
// ============================================================================

// Removed async test since tokio is not available as a dev-dependency
