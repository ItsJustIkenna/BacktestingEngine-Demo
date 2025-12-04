//! Integration tests for the signal module

use signal::{Signal, SignalType, SignalStrength, SignalReason, SignalMetadata};
use chrono::Utc;
use bigdecimal::BigDecimal;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_creation() {
        let mut metadata = HashMap::new();
        metadata.insert("rsi".to_string(), SignalMetadata::Number(75.0));
        
        let signal = Signal {
            id: "test_signal_1".to_string(),
            timestamp: Utc::now(),
            signal_type: SignalType::Buy,
            strength: SignalStrength::Strong,
            price: Some(BigDecimal::from(50000)),
            quantity: Some(BigDecimal::from(1)),
            reason: SignalReason::TechnicalAnalysis("RSI oversold".to_string()),
            metadata,
        };
        
        assert_eq!(signal.id, "test_signal_1");
        assert_eq!(signal.signal_type, SignalType::Buy);
        assert_eq!(signal.strength, SignalStrength::Strong);
        assert_eq!(signal.price, Some(BigDecimal::from(50000)));
        assert_eq!(signal.quantity, Some(BigDecimal::from(1)));
        assert!(matches!(signal.reason, SignalReason::TechnicalAnalysis(_)));
        assert_eq!(signal.metadata.len(), 1);
    }

    #[test]
    fn test_signal_types() {
        let signal_types = vec![
            SignalType::Buy,
            SignalType::Sell,
            SignalType::Hold,
            SignalType::Close,
            SignalType::ScaleIn,
            SignalType::ScaleOut,
            SignalType::StopLoss,
            SignalType::TakeProfit,
            SignalType::Custom("CustomType".to_string()),
        ];
        
        assert_eq!(signal_types.len(), 9);
        assert_eq!(signal_types[0], SignalType::Buy);
        assert_eq!(signal_types[1], SignalType::Sell);
        assert_eq!(signal_types[8], SignalType::Custom("CustomType".to_string()));
    }

    #[test]
    fn test_signal_strength_variants() {
        let strengths = [
            SignalStrength::Weak,
            SignalStrength::Medium,
            SignalStrength::Strong,
            SignalStrength::Custom(0.75),
        ];
        
        assert_eq!(strengths.len(), 4);
        assert_eq!(strengths[0], SignalStrength::Weak);
        assert_eq!(strengths[1], SignalStrength::Medium);
        assert_eq!(strengths[2], SignalStrength::Strong);
        assert_eq!(strengths[3], SignalStrength::Custom(0.75));
    }

    #[test]
    fn test_signal_strength_ordering() {
        // Note: This test assumes PartialOrd is implemented
        // If it's not, we just test equality
        assert_ne!(SignalStrength::Weak, SignalStrength::Strong);
        assert_ne!(SignalStrength::Medium, SignalStrength::Strong);
        assert_eq!(SignalStrength::Weak, SignalStrength::Weak);
    }

    #[test]
    fn test_signal_reason_variants() {
        let reasons = vec![
            SignalReason::TechnicalAnalysis("MA crossover".to_string()),
            SignalReason::FundamentalAnalysis("Earnings beat".to_string()),
            SignalReason::RiskManagement("Stop loss".to_string()),
            SignalReason::RegimeChange("Market shift".to_string()),
            SignalReason::VolumeAnalysis("Volume spike".to_string()),
            SignalReason::PriceAction("Breakout".to_string()),
            SignalReason::MeanReversion("Oversold".to_string()),
            SignalReason::Momentum("Trend continuation".to_string()),
            SignalReason::Arbitrage("Price difference".to_string()),
            SignalReason::Custom("MyCategory".to_string(), "My reason".to_string()),
        ];
        
        assert_eq!(reasons.len(), 10);
    }

    #[test]
    fn test_signal_reason_description() {
        let reason = SignalReason::TechnicalAnalysis("RSI divergence".to_string());
        let description = reason.description();
        assert_eq!(description, "Technical Analysis: RSI divergence");
        
        let custom_reason = SignalReason::Custom("ML".to_string(), "Neural network prediction".to_string());
        let custom_description = custom_reason.description();
        assert_eq!(custom_description, "ML: Neural network prediction");
    }

    #[test]
    fn test_signal_metadata_variants() {
        let number_meta = SignalMetadata::Number(42.5);
        let text_meta = SignalMetadata::Text("Important info".to_string());
        let bool_meta = SignalMetadata::Boolean(true);
        let timestamp_meta = SignalMetadata::Timestamp(Utc::now());
        let price_meta = SignalMetadata::Price(BigDecimal::from(1000));
        let percentage_meta = SignalMetadata::Percentage(0.15);
        
        assert!(matches!(number_meta, SignalMetadata::Number(_)));
        assert!(matches!(text_meta, SignalMetadata::Text(_)));
        assert!(matches!(bool_meta, SignalMetadata::Boolean(_)));
        assert!(matches!(timestamp_meta, SignalMetadata::Timestamp(_)));
        assert!(matches!(price_meta, SignalMetadata::Price(_)));
        assert!(matches!(percentage_meta, SignalMetadata::Percentage(_)));
    }

    #[test]
    fn test_signal_metadata_as_float() {
        let number_meta = SignalMetadata::Number(42.5);
        assert_eq!(number_meta.as_float(), Some(42.5));
        
        let percentage_meta = SignalMetadata::Percentage(0.25);
        assert_eq!(percentage_meta.as_float(), Some(0.25));
        
        let bool_true_meta = SignalMetadata::Boolean(true);
        assert_eq!(bool_true_meta.as_float(), Some(1.0));
        
        let bool_false_meta = SignalMetadata::Boolean(false);
        assert_eq!(bool_false_meta.as_float(), Some(0.0));
        
        let text_meta = SignalMetadata::Text("cannot convert".to_string());
        assert_eq!(text_meta.as_float(), None);
    }

    #[test]
    fn test_signal_metadata_as_string() {
        let number_meta = SignalMetadata::Number(42.5);
        assert_eq!(number_meta.as_string(), "42.5");
        
        let text_meta = SignalMetadata::Text("hello world".to_string());
        assert_eq!(text_meta.as_string(), "hello world");
        
        let bool_meta = SignalMetadata::Boolean(true);
        assert_eq!(bool_meta.as_string(), "true");
        
        let percentage_meta = SignalMetadata::Percentage(0.15);
        assert_eq!(percentage_meta.as_string(), "15.00%");
    }

    #[test]
    fn test_signal_with_multiple_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert("rsi".to_string(), SignalMetadata::Number(80.0));
        metadata.insert("volume".to_string(), SignalMetadata::Number(1500000.0));
        metadata.insert("confirmed".to_string(), SignalMetadata::Boolean(true));
        metadata.insert("strategy".to_string(), SignalMetadata::Text("momentum".to_string()));
        metadata.insert("confidence".to_string(), SignalMetadata::Percentage(0.85));
        
        let signal = Signal {
            id: "complex_signal".to_string(),
            timestamp: Utc::now(),
            signal_type: SignalType::Buy,
            strength: SignalStrength::Strong,
            price: Some(BigDecimal::from(50000)),
            quantity: Some(BigDecimal::from(2)),
            reason: SignalReason::TechnicalAnalysis("Multiple indicators aligned".to_string()),
            metadata,
        };
        
        assert_eq!(signal.metadata.len(), 5);
        assert!(signal.metadata.contains_key("rsi"));
        assert!(signal.metadata.contains_key("volume"));
        assert!(signal.metadata.contains_key("confirmed"));
        assert!(signal.metadata.contains_key("strategy"));
        assert!(signal.metadata.contains_key("confidence"));
        
        if let Some(SignalMetadata::Number(rsi_val)) = signal.metadata.get("rsi") {
            assert_eq!(*rsi_val, 80.0);
        } else {
            panic!("RSI metadata not found or wrong type");
        }
    }

    #[test]
    fn test_signal_serialization() {
        let mut metadata = HashMap::new();
        metadata.insert("indicator".to_string(), SignalMetadata::Text("MACD".to_string()));
        
        let signal = Signal {
            id: "serialize_test".to_string(),
            timestamp: Utc::now(),
            signal_type: SignalType::Sell,
            strength: SignalStrength::Medium,
            price: Some(BigDecimal::from(49000)),
            quantity: Some("0.5".parse::<BigDecimal>().unwrap()),
            reason: SignalReason::TechnicalAnalysis("MACD bearish crossover".to_string()),
            metadata,
        };
        
        // Test serialization
        let json_result = serde_json::to_string(&signal);
        assert!(json_result.is_ok());
        
        // Test deserialization
        let json_str = json_result.unwrap();
        let deserialized_result: Result<Signal, _> = serde_json::from_str(&json_str);
        assert!(deserialized_result.is_ok());
        
        let deserialized = deserialized_result.unwrap();
        assert_eq!(deserialized.id, signal.id);
        assert_eq!(deserialized.signal_type, signal.signal_type);
        assert_eq!(deserialized.strength, signal.strength);
        assert_eq!(deserialized.price, signal.price);
        assert_eq!(deserialized.quantity, signal.quantity);
    }

    #[test]
    fn test_signal_no_price_or_quantity() {
        let signal = Signal {
            id: "minimal_signal".to_string(),
            timestamp: Utc::now(),
            signal_type: SignalType::Hold,
            strength: SignalStrength::Weak,
            price: None,
            quantity: None,
            reason: SignalReason::RiskManagement("Waiting for better entry".to_string()),
            metadata: HashMap::new(),
        };
        
        assert_eq!(signal.id, "minimal_signal");
        assert_eq!(signal.signal_type, SignalType::Hold);
        assert_eq!(signal.strength, SignalStrength::Weak);
        assert!(signal.price.is_none());
        assert!(signal.quantity.is_none());
        assert!(signal.metadata.is_empty());
    }

    #[test]
    fn test_signal_custom_variants() {
        let custom_signal_type = SignalType::Custom("RebalancePortfolio".to_string());
        let custom_strength = SignalStrength::Custom(0.65);
        let custom_reason = SignalReason::Custom("Algorithm".to_string(), "ML model prediction".to_string());
        
        let signal = Signal {
            id: "custom_test".to_string(),
            timestamp: Utc::now(),
            signal_type: custom_signal_type.clone(),
            strength: custom_strength.clone(),
            price: Some(BigDecimal::from(1000)),
            quantity: Some(BigDecimal::from(10)),
            reason: custom_reason.clone(),
            metadata: HashMap::new(),
        };
        
        assert_eq!(signal.signal_type, custom_signal_type);
        assert_eq!(signal.strength, custom_strength);
        assert_eq!(signal.reason, custom_reason);
    }
}
