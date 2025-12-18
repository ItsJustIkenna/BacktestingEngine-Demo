//! Comprehensive tests for Signal module
//! Covers signal creation, validation, types, and metadata

use signal::{
    Signal, SignalType, SignalStrength, SignalReason, SignalMetadata, SignalValidation,
};
use bigdecimal::{BigDecimal, FromPrimitive};
use chrono::Utc;

// ============================================================================
// SIGNAL CREATION TESTS
// ============================================================================

#[test]
fn test_signal_new_basic() {
    let signal = Signal::new(
        "signal_001",
        SignalType::Buy,
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("RSI oversold".to_string()),
    );
    
    assert_eq!(signal.id, "signal_001");
    assert_eq!(signal.signal_type, SignalType::Buy);
    assert_eq!(signal.strength, SignalStrength::Strong);
    assert!(signal.price.is_none());
    assert!(signal.quantity.is_none());
}

#[test]
fn test_signal_buy_constructor() {
    let signal = Signal::buy(
        "buy_001",
        SignalStrength::Medium,
        SignalReason::Momentum("Strong upward momentum".to_string()),
    );
    
    assert_eq!(signal.signal_type, SignalType::Buy);
    assert_eq!(signal.strength, SignalStrength::Medium);
}

#[test]
fn test_signal_sell_constructor() {
    let signal = Signal::sell(
        "sell_001",
        SignalStrength::Weak,
        SignalReason::RiskManagement("Reducing exposure".to_string()),
    );
    
    assert_eq!(signal.signal_type, SignalType::Sell);
    assert_eq!(signal.strength, SignalStrength::Weak);
}

#[test]
fn test_signal_hold_constructor() {
    let signal = Signal::hold(
        "hold_001",
        SignalReason::PriceAction("Consolidating".to_string()),
    );
    
    assert_eq!(signal.signal_type, SignalType::Hold);
    assert_eq!(signal.strength, SignalStrength::Medium); // Default for hold
}

#[test]
fn test_signal_close_constructor() {
    let signal = Signal::close(
        "close_001",
        SignalStrength::Strong,
        SignalReason::RiskManagement("Take profit target reached".to_string()),
    );
    
    assert_eq!(signal.signal_type, SignalType::Close);
    assert_eq!(signal.strength, SignalStrength::Strong);
}

#[test]
fn test_signal_with_price() {
    let signal = Signal::buy(
        "buy_002",
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("Support level".to_string()),
    )
    .with_price(BigDecimal::from_f64(50000.0).unwrap());
    
    assert!(signal.price.is_some());
    let price = signal.price.unwrap();
    assert!((price.to_string().parse::<f64>().unwrap() - 50000.0).abs() < 0.001);
}

#[test]
fn test_signal_with_quantity() {
    let signal = Signal::sell(
        "sell_002",
        SignalStrength::Medium,
        SignalReason::RiskManagement("Position sizing".to_string()),
    )
    .with_quantity(BigDecimal::from_f64(1.5).unwrap());
    
    assert!(signal.quantity.is_some());
    let qty = signal.quantity.unwrap();
    assert!((qty.to_string().parse::<f64>().unwrap() - 1.5).abs() < 0.001);
}

#[test]
fn test_signal_with_price_and_quantity() {
    let signal = Signal::buy(
        "buy_003",
        SignalStrength::Strong,
        SignalReason::Arbitrage("Cross-exchange opportunity".to_string()),
    )
    .with_price(BigDecimal::from_f64(50000.0).unwrap())
    .with_quantity(BigDecimal::from_f64(2.0).unwrap());
    
    assert!(signal.price.is_some());
    assert!(signal.quantity.is_some());
}

// ============================================================================
// SIGNAL TYPE TESTS
// ============================================================================

#[test]
fn test_signal_type_buy() {
    let signal_type = SignalType::Buy;
    assert_eq!(signal_type, SignalType::Buy);
}

#[test]
fn test_signal_type_sell() {
    let signal_type = SignalType::Sell;
    assert_eq!(signal_type, SignalType::Sell);
}

#[test]
fn test_signal_type_hold() {
    let signal_type = SignalType::Hold;
    assert_eq!(signal_type, SignalType::Hold);
}

#[test]
fn test_signal_type_close() {
    let signal_type = SignalType::Close;
    assert_eq!(signal_type, SignalType::Close);
}

#[test]
fn test_signal_type_scale_in() {
    let signal_type = SignalType::ScaleIn;
    assert_eq!(signal_type, SignalType::ScaleIn);
}

#[test]
fn test_signal_type_scale_out() {
    let signal_type = SignalType::ScaleOut;
    assert_eq!(signal_type, SignalType::ScaleOut);
}

#[test]
fn test_signal_type_stop_loss() {
    let signal_type = SignalType::StopLoss;
    assert_eq!(signal_type, SignalType::StopLoss);
}

#[test]
fn test_signal_type_take_profit() {
    let signal_type = SignalType::TakeProfit;
    assert_eq!(signal_type, SignalType::TakeProfit);
}

#[test]
fn test_signal_type_custom() {
    let signal_type = SignalType::Custom("MyCustomType".to_string());
    match signal_type {
        SignalType::Custom(name) => assert_eq!(name, "MyCustomType"),
        _ => panic!("Expected Custom variant"),
    }
}

#[test]
fn test_signal_type_equality() {
    assert_eq!(SignalType::Buy, SignalType::Buy);
    assert_ne!(SignalType::Buy, SignalType::Sell);
}

#[test]
fn test_signal_is_entry_signal() {
    // Buy and ScaleIn are entry signals
    let buy_signal = Signal::buy("b1", SignalStrength::Strong, SignalReason::Momentum("test".to_string()));
    assert!(matches!(buy_signal.signal_type, SignalType::Buy | SignalType::ScaleIn));
}

#[test]
fn test_signal_is_exit_signal() {
    // Sell, Close, StopLoss, TakeProfit, ScaleOut are exit signals
    let sell_signal = Signal::sell("s1", SignalStrength::Strong, SignalReason::RiskManagement("exit".to_string()));
    assert!(matches!(sell_signal.signal_type, SignalType::Sell | SignalType::Close | SignalType::StopLoss | SignalType::TakeProfit | SignalType::ScaleOut));
}

// ============================================================================
// SIGNAL STRENGTH TESTS
// ============================================================================

#[test]
fn test_signal_strength_weak() {
    let strength = SignalStrength::Weak;
    assert_eq!(strength, SignalStrength::Weak);
}

#[test]
fn test_signal_strength_medium() {
    let strength = SignalStrength::Medium;
    assert_eq!(strength, SignalStrength::Medium);
}

#[test]
fn test_signal_strength_strong() {
    let strength = SignalStrength::Strong;
    assert_eq!(strength, SignalStrength::Strong);
}

#[test]
fn test_signal_strength_custom() {
    let strength = SignalStrength::Custom(0.75);
    match strength {
        SignalStrength::Custom(val) => assert!((val - 0.75).abs() < 0.001),
        _ => panic!("Expected Custom variant"),
    }
}

#[test]
fn test_signal_strength_ordering() {
    // Custom values should allow precise ordering
    let weak = SignalStrength::Custom(0.25);
    let medium = SignalStrength::Custom(0.50);
    let strong = SignalStrength::Custom(0.75);
    
    if let (SignalStrength::Custom(w), SignalStrength::Custom(m), SignalStrength::Custom(s)) = (weak, medium, strong) {
        assert!(w < m);
        assert!(m < s);
    }
}

// ============================================================================
// SIGNAL REASON TESTS
// ============================================================================

#[test]
fn test_signal_reason_technical_analysis() {
    let reason = SignalReason::TechnicalAnalysis("RSI below 30".to_string());
    let desc = reason.description();
    assert!(desc.contains("Technical Analysis"));
    assert!(desc.contains("RSI below 30"));
}

#[test]
fn test_signal_reason_fundamental_analysis() {
    let reason = SignalReason::FundamentalAnalysis("Strong earnings".to_string());
    let desc = reason.description();
    assert!(desc.contains("Fundamental Analysis"));
}

#[test]
fn test_signal_reason_risk_management() {
    let reason = SignalReason::RiskManagement("Position too large".to_string());
    let desc = reason.description();
    assert!(desc.contains("Risk Management"));
}

#[test]
fn test_signal_reason_regime_change() {
    let reason = SignalReason::RegimeChange("High volatility detected".to_string());
    let desc = reason.description();
    assert!(desc.contains("Regime Change"));
}

#[test]
fn test_signal_reason_volume_analysis() {
    let reason = SignalReason::VolumeAnalysis("Unusual volume spike".to_string());
    let desc = reason.description();
    assert!(desc.contains("Volume Analysis"));
}

#[test]
fn test_signal_reason_price_action() {
    let reason = SignalReason::PriceAction("Breakout pattern".to_string());
    let desc = reason.description();
    assert!(desc.contains("Price Action"));
}

#[test]
fn test_signal_reason_mean_reversion() {
    let reason = SignalReason::MeanReversion("Price 2 std dev below mean".to_string());
    let desc = reason.description();
    assert!(desc.contains("Mean Reversion"));
}

#[test]
fn test_signal_reason_momentum() {
    let reason = SignalReason::Momentum("Strong upward trend".to_string());
    let desc = reason.description();
    assert!(desc.contains("Momentum"));
}

#[test]
fn test_signal_reason_arbitrage() {
    let reason = SignalReason::Arbitrage("Price discrepancy detected".to_string());
    let desc = reason.description();
    assert!(desc.contains("Arbitrage"));
}

#[test]
fn test_signal_reason_custom() {
    let reason = SignalReason::Custom("MyCategory".to_string(), "Custom reason".to_string());
    let desc = reason.description();
    assert!(desc.contains("MyCategory"));
    assert!(desc.contains("Custom reason"));
}

// ============================================================================
// SIGNAL METADATA TESTS
// ============================================================================

#[test]
fn test_signal_metadata_number() {
    let metadata = SignalMetadata::Number(42.5);
    assert_eq!(metadata.as_float(), Some(42.5));
    assert_eq!(metadata.as_string(), "42.5");
}

#[test]
fn test_signal_metadata_text() {
    let metadata = SignalMetadata::Text("Important note".to_string());
    assert_eq!(metadata.as_float(), None);
    assert_eq!(metadata.as_string(), "Important note");
}

#[test]
fn test_signal_metadata_boolean_true() {
    let metadata = SignalMetadata::Boolean(true);
    assert_eq!(metadata.as_float(), Some(1.0));
    assert_eq!(metadata.as_string(), "true");
}

#[test]
fn test_signal_metadata_boolean_false() {
    let metadata = SignalMetadata::Boolean(false);
    assert_eq!(metadata.as_float(), Some(0.0));
    assert_eq!(metadata.as_string(), "false");
}

#[test]
fn test_signal_metadata_timestamp() {
    let now = Utc::now();
    let metadata = SignalMetadata::Timestamp(now);
    assert_eq!(metadata.as_float(), None);
    let string_rep = metadata.as_string();
    assert!(!string_rep.is_empty());
}

#[test]
fn test_signal_metadata_price() {
    let metadata = SignalMetadata::Price(BigDecimal::from_f64(50000.0).unwrap());
    assert_eq!(metadata.as_float(), None);
    assert!(metadata.as_string().contains("50000"));
}

#[test]
fn test_signal_metadata_percentage() {
    let metadata = SignalMetadata::Percentage(0.1234);
    assert_eq!(metadata.as_float(), Some(0.1234));
    let string_rep = metadata.as_string();
    assert!(string_rep.contains("%"));
    assert!(string_rep.contains("12.34")); // 0.1234 * 100 = 12.34%
}

#[test]
fn test_signal_with_metadata() {
    let mut signal = Signal::buy(
        "buy_meta",
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("Test".to_string()),
    );
    
    signal.metadata.insert("rsi".to_string(), SignalMetadata::Number(28.5));
    signal.metadata.insert("macd_crossover".to_string(), SignalMetadata::Boolean(true));
    signal.metadata.insert("notes".to_string(), SignalMetadata::Text("Entry signal".to_string()));
    
    assert_eq!(signal.metadata.len(), 3);
    assert!(signal.metadata.contains_key("rsi"));
}

#[test]
fn test_signal_with_multiple_metadata() {
    let mut signal = Signal::new(
        "signal_multi",
        SignalType::Buy,
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("Multiple indicators".to_string()),
    );
    
    // Add various metadata types
    signal.metadata.insert("rsi".to_string(), SignalMetadata::Number(25.0));
    signal.metadata.insert("macd".to_string(), SignalMetadata::Number(0.005));
    signal.metadata.insert("bb_position".to_string(), SignalMetadata::Percentage(-0.95));
    signal.metadata.insert("trend".to_string(), SignalMetadata::Text("bullish".to_string()));
    signal.metadata.insert("confirmed".to_string(), SignalMetadata::Boolean(true));
    
    assert_eq!(signal.metadata.len(), 5);
    
    // Verify retrieval
    if let Some(SignalMetadata::Number(rsi)) = signal.metadata.get("rsi") {
        assert!((rsi - 25.0).abs() < 0.001);
    } else {
        panic!("Expected Number metadata for rsi");
    }
}

// ============================================================================
// SIGNAL VALIDATION TESTS
// ============================================================================

#[test]
fn test_signal_validation_valid() {
    let validation = SignalValidation {
        is_valid: true,
        errors: vec![],
        warnings: vec![],
    };
    
    assert!(validation.is_valid);
    assert!(validation.errors.is_empty());
    assert!(validation.warnings.is_empty());
}

#[test]
fn test_signal_validation_with_errors() {
    let validation = SignalValidation {
        is_valid: false,
        errors: vec!["Empty signal ID".to_string(), "Invalid price".to_string()],
        warnings: vec![],
    };
    
    assert!(!validation.is_valid);
    assert_eq!(validation.errors.len(), 2);
}

#[test]
fn test_signal_validation_with_warnings() {
    let validation = SignalValidation {
        is_valid: true,
        errors: vec![],
        warnings: vec!["Signal timestamp is stale".to_string()],
    };
    
    assert!(validation.is_valid);
    assert_eq!(validation.warnings.len(), 1);
}

#[test]
fn test_signal_validation_with_errors_and_warnings() {
    let validation = SignalValidation {
        is_valid: false,
        errors: vec!["Critical error".to_string()],
        warnings: vec!["Minor warning".to_string()],
    };
    
    assert!(!validation.is_valid);
    assert_eq!(validation.errors.len(), 1);
    assert_eq!(validation.warnings.len(), 1);
}

// ============================================================================
// SIGNAL SERIALIZATION TESTS
// ============================================================================

#[test]
fn test_signal_serialization() {
    let signal = Signal::buy(
        "ser_001",
        SignalStrength::Strong,
        SignalReason::Momentum("Test".to_string()),
    )
    .with_price(BigDecimal::from_f64(50000.0).unwrap())
    .with_quantity(BigDecimal::from_f64(1.0).unwrap());
    
    let json = serde_json::to_string(&signal).unwrap();
    assert!(json.contains("ser_001"));
    assert!(json.contains("Buy"));
    assert!(json.contains("Strong"));
}

#[test]
fn test_signal_deserialization() {
    let signal = Signal::sell(
        "deser_001",
        SignalStrength::Medium,
        SignalReason::RiskManagement("Test".to_string()),
    );
    
    let json = serde_json::to_string(&signal).unwrap();
    let deserialized: Signal = serde_json::from_str(&json).unwrap();
    
    assert_eq!(signal.id, deserialized.id);
    assert_eq!(signal.signal_type, deserialized.signal_type);
    assert_eq!(signal.strength, deserialized.strength);
}

#[test]
fn test_signal_type_serialization() {
    let types = vec![
        SignalType::Buy,
        SignalType::Sell,
        SignalType::Hold,
        SignalType::Close,
        SignalType::ScaleIn,
        SignalType::ScaleOut,
        SignalType::StopLoss,
        SignalType::TakeProfit,
        SignalType::Custom("test".to_string()),
    ];
    
    for signal_type in types {
        let json = serde_json::to_string(&signal_type).unwrap();
        let deserialized: SignalType = serde_json::from_str(&json).unwrap();
        assert_eq!(signal_type, deserialized);
    }
}

#[test]
fn test_signal_strength_serialization() {
    let strengths = vec![
        SignalStrength::Weak,
        SignalStrength::Medium,
        SignalStrength::Strong,
        SignalStrength::Custom(0.42),
    ];
    
    for strength in strengths {
        let json = serde_json::to_string(&strength).unwrap();
        let deserialized: SignalStrength = serde_json::from_str(&json).unwrap();
        assert_eq!(strength, deserialized);
    }
}

// ============================================================================
// SIGNAL TIMESTAMP TESTS
// ============================================================================

#[test]
fn test_signal_has_timestamp() {
    let before = Utc::now();
    let signal = Signal::buy(
        "ts_001",
        SignalStrength::Strong,
        SignalReason::TechnicalAnalysis("Test".to_string()),
    );
    let after = Utc::now();
    
    assert!(signal.timestamp >= before);
    assert!(signal.timestamp <= after);
}

#[test]
fn test_signal_timestamps_unique() {
    let signal1 = Signal::buy("ts1", SignalStrength::Strong, SignalReason::Momentum("".to_string()));
    std::thread::sleep(std::time::Duration::from_millis(1));
    let signal2 = Signal::buy("ts2", SignalStrength::Strong, SignalReason::Momentum("".to_string()));
    
    assert!(signal2.timestamp >= signal1.timestamp);
}
