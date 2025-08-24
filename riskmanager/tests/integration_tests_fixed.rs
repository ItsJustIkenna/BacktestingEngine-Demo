//! Integration tests for the riskmanager module

use riskmanager::{RiskLimits, RiskMetrics, RiskManager};

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
    let metrics = RiskMetrics {
        current_position: 5.0,
        current_order_size: 1.0,
        current_inventory_skew: 1.5,
        current_volatility: 0.08,
    };
    
    assert_eq!(metrics.current_position, 5.0);
    assert_eq!(metrics.current_order_size, 1.0);
    assert_eq!(metrics.current_inventory_skew, 1.5);
    assert_eq!(metrics.current_volatility, 0.08);
}

#[test]
fn test_risk_metrics_validation() {
    let valid_metrics = RiskMetrics {
        current_position: 5.0,
        current_order_size: 1.0,
        current_inventory_skew: 1.5,
        current_volatility: 0.08,
    };
    
    // These should all be reasonable values
    assert!(valid_metrics.current_position > 0.0);
    assert!(valid_metrics.current_order_size > 0.0);
    assert!(valid_metrics.current_inventory_skew > 0.0);
    assert!(valid_metrics.current_volatility > 0.0);
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
        current_position: 5.0,
        current_order_size: 1.0,
        current_inventory_skew: 2.0,
        current_volatility: 0.12,
    };
    
    // Test JSON serialization
    let json_str = serde_json::to_string(&metrics).unwrap();
    assert!(json_str.contains("current_position"));
    assert!(json_str.contains("current_volatility"));
    
    // Test deserialization
    let deserialized: RiskMetrics = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.current_position, metrics.current_position);
    assert_eq!(deserialized.current_volatility, metrics.current_volatility);
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
    // Test with different values
    let metrics = RiskMetrics {
        current_position: 0.0,
        current_order_size: 0.1,
        current_inventory_skew: -1.0,
        current_volatility: 0.20,
    };
    
    assert_eq!(metrics.current_position, 0.0);
    assert_eq!(metrics.current_order_size, 0.1);
    assert_eq!(metrics.current_inventory_skew, -1.0);
    assert_eq!(metrics.current_volatility, 0.20);
}

#[test]
fn test_risk_manager_creation() {
    let risk_manager = RiskManager::new();
    
    // Should have default limits
    assert_eq!(risk_manager.limits.max_position, Some(10.0));
    assert_eq!(risk_manager.limits.max_order_size, Some(1.0));
    assert_eq!(risk_manager.limits.max_inventory_skew, Some(2.0));
    assert_eq!(risk_manager.limits.max_volatility, Some(0.10));
}

#[test]
fn test_risk_manager_with_custom_limits() {
    let custom_limits = RiskLimits {
        max_position: Some(5.0),
        max_order_size: Some(0.5),
        max_inventory_skew: Some(1.0),
        max_volatility: Some(0.05),
    };
    
    let risk_manager = RiskManager::with_limits(custom_limits);
    
    assert_eq!(risk_manager.limits.max_position, Some(5.0));
    assert_eq!(risk_manager.limits.max_order_size, Some(0.5));
    assert_eq!(risk_manager.limits.max_inventory_skew, Some(1.0));
    assert_eq!(risk_manager.limits.max_volatility, Some(0.05));
}

#[test]
fn test_default_implementations() {
    let default_limits = RiskLimits::default();
    let risk_manager = RiskManager::new();
    
    // Default limits should have some reasonable values
    assert!(default_limits.max_position.is_some());
    assert!(default_limits.max_order_size.is_some());
    
    // Risk manager should use default limits
    assert_eq!(risk_manager.limits.max_position, default_limits.max_position);
    assert_eq!(risk_manager.limits.max_order_size, default_limits.max_order_size);
}
