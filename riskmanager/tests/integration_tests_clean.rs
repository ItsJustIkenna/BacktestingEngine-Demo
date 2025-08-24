//! Integration tests for the riskmanager module

use riskmanager::{RiskLimits, RiskMetrics};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_limits_default() {
        let limits = RiskLimits::default();
        
        assert_eq!(limits.max_position, Some(10.0));
        assert_eq!(limits.max_order_size, Some(1.0));
        assert_eq!(limits.max_inventory_skew, Some(2.0));
        assert_eq!(limits.max_volatility, Some(0.10));
    }

    #[test]
    fn test_risk_limits_custom() {
        let limits = RiskLimits {
            max_position: Some(5.0),
            max_order_size: Some(0.5),
            max_inventory_skew: Some(1.0),
            max_volatility: Some(0.05),
        };
        
        assert_eq!(limits.max_position, Some(5.0));
        assert_eq!(limits.max_order_size, Some(0.5));
        assert_eq!(limits.max_inventory_skew, Some(1.0));
        assert_eq!(limits.max_volatility, Some(0.05));
    }

    #[test]
    fn test_risk_metrics_creation() {
        use bigdecimal::BigDecimal;
        use std::collections::HashMap;
        
        let metrics = RiskMetrics {
            max_position: Some(10.0),
            max_order_size: Some(1.0),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(0.10),
        };
        
        assert_eq!(metrics.max_position, Some(10.0));
        assert_eq!(metrics.max_order_size, Some(1.0));
        assert_eq!(metrics.max_inventory_skew, Some(2.0));
        assert_eq!(metrics.max_volatility, Some(0.10));
    }

    #[test]
    fn test_risk_metrics_validation() {
        let valid_metrics = RiskMetrics {
            max_position: Some(5.0),
            max_order_size: Some(1.0),
            max_inventory_skew: Some(1.5),
            max_volatility: Some(0.08),
        };
        
        // These should all be reasonable values
        assert!(valid_metrics.max_position.unwrap() > 0.0);
        assert!(valid_metrics.max_order_size.unwrap() > 0.0);
        assert!(valid_metrics.max_inventory_skew.unwrap() > 0.0);
        assert!(valid_metrics.max_volatility.unwrap() > 0.0);
    }

    #[test]
    fn test_risk_limits_serialization() {
        let limits = RiskLimits {
            max_position: Some(10.0),
            max_order_size: Some(2.0),
            max_inventory_skew: Some(3.0),
            max_volatility: Some(0.15),
        };
        
        // Test JSON serialization
        let json_str = serde_json::to_string(&limits).unwrap();
        assert!(json_str.contains("max_position"));
        assert!(json_str.contains("max_order_size"));
        
        // Test deserialization
        let deserialized: RiskLimits = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.max_position, limits.max_position);
        assert_eq!(deserialized.max_order_size, limits.max_order_size);
    }

    #[test]
    fn test_risk_metrics_serialization() {
        let metrics = RiskMetrics {
            max_position: Some(5.0),
            max_order_size: Some(1.0),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(0.12),
        };
        
        // Test JSON serialization
        let json_str = serde_json::to_string(&metrics).unwrap();
        assert!(json_str.contains("max_position"));
        assert!(json_str.contains("max_volatility"));
        
        // Test deserialization
        let deserialized: RiskMetrics = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.max_position, metrics.max_position);
        assert_eq!(deserialized.max_volatility, metrics.max_volatility);
    }

    #[test]
    fn test_risk_limits_edge_cases() {
        // Test with None values
        let limits = RiskLimits {
            max_position: None,
            max_order_size: None,
            max_inventory_skew: Some(1.0),
            max_volatility: Some(0.05),
        };
        
        assert!(limits.max_position.is_none());
        assert!(limits.max_order_size.is_none());
        assert!(limits.max_inventory_skew.is_some());
        assert!(limits.max_volatility.is_some());
    }

    #[test]
    fn test_risk_metrics_edge_cases() {
        // Test with None values
        let metrics = RiskMetrics {
            max_position: None,
            max_order_size: Some(0.1),
            max_inventory_skew: None,
            max_volatility: Some(0.20),
        };
        
        assert!(metrics.max_position.is_none());
        assert!(metrics.max_order_size.is_some());
        assert!(metrics.max_inventory_skew.is_none());
        assert!(metrics.max_volatility.is_some());
    }

    #[test]
    fn test_default_implementations() {
        let default_limits = RiskLimits::default();
        let default_metrics = RiskMetrics::default();
        
        // Default limits should have some reasonable values
        assert!(default_limits.max_position.is_some());
        assert!(default_limits.max_order_size.is_some());
        
        // Default metrics should match default limits
        assert_eq!(default_metrics.max_position, default_limits.max_position);
        assert_eq!(default_metrics.max_order_size, default_limits.max_order_size);
    }
}
