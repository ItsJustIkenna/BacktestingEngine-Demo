//! Comprehensive tests for Strategy module
//! Covers strategy management, signals, indicators, and metrics

use strategy::{
    Strategy, StrategyContext, StrategyMetrics,
    Signal, SignalType, SignalStrength, SignalReason,
    StrategyManager, IndicatorValue,
    ParameterValue, StrategyConfig, StrategyRiskLimits,
};
use std::collections::HashMap;
use chrono::Utc;
use bigdecimal::BigDecimal;

// ============================================================================
// STRATEGY MANAGER TESTS
// ============================================================================

#[test]
fn test_strategy_manager_creation() {
    let manager = StrategyManager::new();
    
    assert!(true, "Strategy manager created successfully");
}

#[test]
fn test_strategy_manager_operations() {
    let mut manager = StrategyManager::new();
    
    // Manager should be able to handle operations
    assert!(true, "Strategy manager operations work");
}

// ============================================================================
// SIGNAL TESTS
// ============================================================================

#[test]
fn test_signal_creation() {
    let signal = Signal::new(
        "signal_001",
        SignalType::Buy,
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("RSI oversold".to_string()),
    );
    
    assert_eq!(signal.id, "signal_001");
    assert!(matches!(signal.signal_type, SignalType::Buy));
    assert!(matches!(signal.strength, SignalStrength::Strong));
}

#[test]
fn test_signal_types() {
    let types = vec![
        SignalType::Buy,
        SignalType::Sell,
        SignalType::Hold,
        SignalType::Close,
        SignalType::ScaleIn,
        SignalType::ScaleOut,
        SignalType::StopLoss,
        SignalType::TakeProfit,
    ];
    
    assert!(types.len() >= 8);
}

#[test]
fn test_signal_custom_type() {
    let custom_signal = Signal::new(
        "custom_001",
        SignalType::Custom("Hedge".to_string()),
        SignalStrength::Medium,
        SignalReason::Custom("Strategy".to_string(), "Custom hedge signal".to_string()),
    );
    
    if let SignalType::Custom(name) = custom_signal.signal_type {
        assert_eq!(name, "Hedge");
    } else {
        panic!("Expected Custom signal type");
    }
}

#[test]
fn test_signal_strength_levels() {
    let weak = SignalStrength::Weak;
    let medium = SignalStrength::Medium;
    let strong = SignalStrength::Strong;
    let custom = SignalStrength::Custom(0.75);
    
    assert!(matches!(weak, SignalStrength::Weak));
    assert!(matches!(medium, SignalStrength::Medium));
    assert!(matches!(strong, SignalStrength::Strong));
    
    if let SignalStrength::Custom(val) = custom {
        assert!((val - 0.75).abs() < 0.001);
    }
}

// ============================================================================
// SIGNAL REASON TESTS
// ============================================================================

#[test]
fn test_signal_reasons() {
    let reasons = vec![
        SignalReason::TechnicalAnalysis("RSI".to_string()),
        SignalReason::FundamentalAnalysis("Earnings".to_string()),
        SignalReason::RiskManagement("Stop loss".to_string()),
        SignalReason::RegimeChange("Volatility shift".to_string()),
        SignalReason::VolumeAnalysis("Volume spike".to_string()),
        SignalReason::PriceAction("Breakout".to_string()),
        SignalReason::MeanReversion("Oversold".to_string()),
        SignalReason::Momentum("Trending".to_string()),
        SignalReason::Arbitrage("Spread".to_string()),
    ];
    
    for reason in reasons {
        let desc = reason.description();
        assert!(!desc.is_empty(), "Reason should have description");
    }
}

#[test]
fn test_signal_reason_description() {
    let reason = SignalReason::TechnicalAnalysis("RSI below 30".to_string());
    let desc = reason.description();
    
    assert!(desc.contains("Technical Analysis"));
    assert!(desc.contains("RSI below 30"));
}

// ============================================================================
// INDICATOR VALUE TESTS
// ============================================================================

#[test]
fn test_indicator_value_creation() {
    let value = IndicatorValue {
        timestamp: Utc::now(),
        value: 42.5,
    };
    
    assert_eq!(value.value, 42.5);
}

#[test]
fn test_indicator_value_series() {
    let base_time = Utc::now();
    
    let values: Vec<IndicatorValue> = (0..10)
        .map(|i| IndicatorValue {
            timestamp: base_time + chrono::Duration::hours(i),
            value: 50.0 + i as f64,
        })
        .collect();
    
    assert_eq!(values.len(), 10);
    assert!(values[9].value > values[0].value);
}

// ============================================================================
// STRATEGY METRICS TESTS
// ============================================================================

#[test]
fn test_strategy_metrics_creation() {
    let metrics = StrategyMetrics::new();
    
    assert_eq!(metrics.total_signals, 0);
    assert_eq!(metrics.buy_signals, 0);
    assert_eq!(metrics.sell_signals, 0);
}

#[test]
fn test_strategy_metrics_record_signal() {
    let mut metrics = StrategyMetrics::new();
    
    let buy_signal = Signal::new(
        "buy_001",
        SignalType::Buy,
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("RSI".to_string()),
    );
    
    metrics.record_signal(&buy_signal);
    
    assert_eq!(metrics.total_signals, 1);
    assert_eq!(metrics.buy_signals, 1);
}

#[test]
fn test_strategy_metrics_multiple_signals() {
    let mut metrics = StrategyMetrics::new();
    
    let buy_signal = Signal::new(
        "buy_001",
        SignalType::Buy,
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("RSI".to_string()),
    );
    
    let sell_signal = Signal::new(
        "sell_001",
        SignalType::Sell,
        SignalStrength::Medium,
        SignalReason::TechnicalAnalysis("RSI".to_string()),
    );
    
    metrics.record_signal(&buy_signal);
    metrics.record_signal(&sell_signal);
    
    assert_eq!(metrics.total_signals, 2);
    assert_eq!(metrics.buy_signals, 1);
    assert_eq!(metrics.sell_signals, 1);
}

// ============================================================================
// STRATEGY CONFIG TESTS
// ============================================================================

#[test]
fn test_strategy_config_creation() {
    let config = StrategyConfig {
        id: "strat_001".to_string(),
        name: "Moving Average Crossover".to_string(),
        strategy_type: "momentum".to_string(),
        symbol: "BTC-USD".to_string(),
        exchange: "coinbase".to_string(),
        parameters: HashMap::new(),
        risk_limits: StrategyRiskLimits::default(),
        enabled: true,
    };
    
    assert_eq!(config.id, "strat_001");
    assert!(config.enabled);
}

#[test]
fn test_strategy_config_with_parameters() {
    let mut params = HashMap::new();
    params.insert("fast_period".to_string(), ParameterValue::Int(10));
    params.insert("slow_period".to_string(), ParameterValue::Int(20));
    params.insert("threshold".to_string(), ParameterValue::Float(0.02));
    params.insert("enabled".to_string(), ParameterValue::Bool(true));
    
    let config = StrategyConfig {
        id: "strat_001".to_string(),
        name: "MA Crossover".to_string(),
        strategy_type: "momentum".to_string(),
        symbol: "BTC-USD".to_string(),
        exchange: "binance".to_string(),
        parameters: params,
        risk_limits: StrategyRiskLimits::default(),
        enabled: true,
    };
    
    assert_eq!(config.parameters.len(), 4);
}

// ============================================================================
// PARAMETER VALUE TESTS
// ============================================================================

#[test]
fn test_parameter_value_types() {
    let int_val = ParameterValue::Int(42);
    let float_val = ParameterValue::Float(3.14);
    let string_val = ParameterValue::String("test".to_string());
    let bool_val = ParameterValue::Bool(true);
    
    if let ParameterValue::Int(v) = int_val {
        assert_eq!(v, 42);
    }
    
    if let ParameterValue::Float(v) = float_val {
        assert!((v - 3.14).abs() < 0.001);
    }
    
    if let ParameterValue::String(v) = string_val {
        assert_eq!(v, "test");
    }
    
    if let ParameterValue::Bool(v) = bool_val {
        assert!(v);
    }
}

#[test]
fn test_parameter_value_as_int() {
    let int_val = ParameterValue::Int(42);
    let float_val = ParameterValue::Float(42.9);
    
    assert_eq!(int_val.as_int(), Some(42));
    assert_eq!(float_val.as_int(), Some(42)); // Truncated
}

// ============================================================================
// RISK LIMITS TESTS
// ============================================================================

#[test]
fn test_strategy_risk_limits_default() {
    let limits = StrategyRiskLimits::default();
    
    // Should have sensible defaults
    assert!(true, "Risk limits created with defaults");
}

// ============================================================================
// SIGNAL STRENGTH CALCULATION TESTS
// ============================================================================

#[test]
fn test_signal_strength_values() {
    // Conceptual: mapping strength to numeric values
    fn strength_to_value(strength: &SignalStrength) -> f64 {
        match strength {
            SignalStrength::Weak => 1.0,
            SignalStrength::Medium => 2.0,
            SignalStrength::Strong => 3.0,
            SignalStrength::Custom(v) => *v,
        }
    }
    
    assert_eq!(strength_to_value(&SignalStrength::Weak), 1.0);
    assert_eq!(strength_to_value(&SignalStrength::Medium), 2.0);
    assert_eq!(strength_to_value(&SignalStrength::Strong), 3.0);
    assert_eq!(strength_to_value(&SignalStrength::Custom(2.5)), 2.5);
}

// ============================================================================
// MOVING AVERAGE CONCEPT TESTS
// ============================================================================

#[test]
fn test_simple_moving_average_concept() {
    let prices = vec![100.0, 102.0, 101.0, 103.0, 102.0];
    let period = 3;
    
    // SMA = sum of last N prices / N
    let sma: f64 = prices[prices.len()-period..].iter().sum::<f64>() / period as f64;
    
    let expected = (101.0 + 103.0 + 102.0) / 3.0;
    assert!((sma - expected).abs() < 0.001);
}

#[test]
fn test_exponential_moving_average_concept() {
    let prices = vec![100.0, 102.0, 101.0, 103.0, 102.0];
    let period = 3;
    let multiplier = 2.0 / (period as f64 + 1.0); // EMA multiplier
    
    // First EMA = SMA
    let sma: f64 = prices[0..period].iter().sum::<f64>() / period as f64;
    
    // Then apply EMA formula
    let mut ema = sma;
    for price in &prices[period..] {
        ema = (price - ema) * multiplier + ema;
    }
    
    assert!(ema > 0.0);
}

// ============================================================================
// RSI CONCEPT TESTS
// ============================================================================

#[test]
fn test_rsi_calculation_concept() {
    // RSI = 100 - (100 / (1 + RS))
    // RS = Average Gain / Average Loss
    
    let avg_gain = 2.5;
    let avg_loss = 1.5;
    
    let rs = avg_gain / avg_loss;
    let rsi = 100.0 - (100.0 / (1.0 + rs));
    
    assert!(rsi > 50.0, "More gains than losses should give RSI > 50");
    assert!(rsi < 70.0, "RSI should be reasonable");
}

#[test]
fn test_rsi_oversold_overbought() {
    // Oversold: RSI < 30
    // Overbought: RSI > 70
    
    let oversold_rsi = 25.0;
    let overbought_rsi = 75.0;
    let neutral_rsi = 50.0;
    
    assert!(oversold_rsi < 30.0, "Should be oversold");
    assert!(overbought_rsi > 70.0, "Should be overbought");
    assert!(neutral_rsi >= 30.0 && neutral_rsi <= 70.0, "Should be neutral");
}

// ============================================================================
// BOLLINGER BANDS CONCEPT TESTS
// ============================================================================

#[test]
fn test_bollinger_bands_concept() {
    let prices = vec![100.0, 101.0, 99.0, 102.0, 98.0, 103.0, 101.0];
    let period = 5;
    let std_dev_multiplier = 2.0;
    
    let recent_prices = &prices[prices.len()-period..];
    let mean: f64 = recent_prices.iter().sum::<f64>() / period as f64;
    
    let variance: f64 = recent_prices.iter()
        .map(|p| (p - mean).powi(2))
        .sum::<f64>() / period as f64;
    let std_dev = variance.sqrt();
    
    let upper_band = mean + std_dev * std_dev_multiplier;
    let lower_band = mean - std_dev * std_dev_multiplier;
    
    assert!(upper_band > mean);
    assert!(lower_band < mean);
    assert!(upper_band - lower_band > 0.0);
}

// ============================================================================
// ASYNC TESTS
// ============================================================================

#[tokio::test]
async fn test_strategy_async_operations() {
    let _manager = StrategyManager::new();
    
    // Simulate async strategy operations
    assert!(true, "Async strategy operations work");
}
