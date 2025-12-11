//! Comprehensive tests for RiskManager
//! Covers risk limits checking, metrics validation, and edge cases

use riskmanager::{RiskManager, RiskLimits, RiskMetrics};

// ============================================================================
// RISK LIMITS TESTS
// ============================================================================

#[test]
fn test_risk_limits_default() {
    let limits = RiskLimits::default();
    
    assert!(limits.max_position.is_some());
    assert!(limits.max_order_size.is_some());
    assert!(limits.max_inventory_skew.is_some());
    assert!(limits.max_volatility.is_some());
    assert!(limits.max_drawdown.is_some());
    assert!(limits.stop_loss_pct.is_some());
    assert!(limits.max_daily_loss.is_some());
}

#[test]
fn test_risk_limits_custom() {
    let limits = RiskLimits {
        max_position: Some(100.0),
        max_order_size: Some(10.0),
        max_inventory_skew: Some(5.0),
        max_volatility: Some(0.25),
        ..Default::default()
    };
    
    assert_eq!(limits.max_position, Some(100.0));
    assert_eq!(limits.max_order_size, Some(10.0));
    assert_eq!(limits.max_inventory_skew, Some(5.0));
    assert_eq!(limits.max_volatility, Some(0.25));
}

#[test]
fn test_risk_limits_partial() {
    let limits = RiskLimits {
        max_position: Some(50.0),
        max_order_size: None, // No order size limit
        max_inventory_skew: Some(3.0),
        max_volatility: None, // No volatility limit
        ..Default::default()
    };
    
    assert!(limits.max_position.is_some());
    assert!(limits.max_order_size.is_none());
    assert!(limits.max_inventory_skew.is_some());
    assert!(limits.max_volatility.is_none());
}

#[test]
fn test_risk_limits_all_none() {
    let limits = RiskLimits {
        max_position: None,
        max_order_size: None,
        max_inventory_skew: None,
        max_volatility: None,
        max_drawdown: None,
        stop_loss_pct: None,
        max_daily_loss: None,
        ..Default::default()
    };
    
    // All limits disabled
    assert!(limits.max_position.is_none());
}

// ============================================================================
// RISK METRICS TESTS
// ============================================================================

#[test]
fn test_risk_metrics_creation() {
    let metrics = RiskMetrics {
        current_position: 5.0,
        current_order_size: 0.5,
        current_inventory_skew: 1.2,
        current_volatility: 0.05,
        ..Default::default()
    };
    
    assert!((metrics.current_position - 5.0).abs() < 0.001);
    assert!((metrics.current_order_size - 0.5).abs() < 0.001);
    assert!((metrics.current_inventory_skew - 1.2).abs() < 0.001);
    assert!((metrics.current_volatility - 0.05).abs() < 0.001);
}

#[test]
fn test_risk_metrics_zero_values() {
    let metrics = RiskMetrics::default();
    
    assert!((metrics.current_position - 0.0).abs() < 0.001);
}

#[test]
fn test_risk_metrics_negative_position() {
    // Short position
    let metrics = RiskMetrics {
        current_position: -5.0,
        current_order_size: 0.5,
        current_inventory_skew: -1.2,
        current_volatility: 0.05,
        ..Default::default()
    };
    
    assert!(metrics.current_position < 0.0);
    assert!(metrics.current_inventory_skew < 0.0);
}

// ============================================================================
// RISK MANAGER CREATION TESTS
// ============================================================================

#[test]
fn test_risk_manager_new() {
    let rm = RiskManager::new();
    
    // Should have default limits
    assert!(rm.limits.max_position.is_some());
}

#[test]
fn test_risk_manager_default() {
    let rm = RiskManager::default();
    
    // Default should work same as new()
    assert!(rm.limits.max_position.is_some());
}

#[test]
fn test_risk_manager_with_limits() {
    let limits = RiskLimits {
        max_position: Some(50.0),
        max_order_size: Some(5.0),
        max_inventory_skew: Some(2.0),
        max_volatility: Some(0.15),
        ..Default::default()
    };
    
    let rm = RiskManager::with_limits(limits);
    
    assert_eq!(rm.limits.max_position, Some(50.0));
    assert_eq!(rm.limits.max_order_size, Some(5.0));
}

// ============================================================================
// RISK CHECK - POSITION LIMIT TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_position_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 5.0,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Position within limit should pass");
}

#[test]
fn test_risk_manager_check_position_at_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 10.0,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Position at exact limit should pass");
}

#[test]
fn test_risk_manager_check_position_exceeds_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 15.0,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err(), "Position exceeding limit should fail");
    
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("Position"), "Error should mention position");
}

#[test]
fn test_risk_manager_check_negative_position_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: -8.0, // Short position
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Negative position within abs limit should pass");
}

// ============================================================================
// RISK CHECK - ORDER SIZE LIMIT TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_order_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_order_size: Some(1.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_order_size: 0.5,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Order within limit should pass");
}

#[test]
fn test_risk_manager_check_order_exceeds_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_order_size: Some(1.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_order_size: 1.5,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err(), "Order exceeding limit should fail");
}

// ============================================================================
// RISK CHECK - INVENTORY SKEW LIMIT TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_skew_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_inventory_skew: Some(2.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_inventory_skew: 1.5,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Skew within limit should pass");
}

#[test]
fn test_risk_manager_check_skew_exceeds_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_inventory_skew: Some(2.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_inventory_skew: 2.5,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err(), "Skew exceeding limit should fail");
}

// ============================================================================
// RISK CHECK - VOLATILITY LIMIT TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_volatility_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_volatility: Some(0.10),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_volatility: 0.05,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Volatility within limit should pass");
}

#[test]
fn test_risk_manager_check_volatility_exceeds_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_volatility: Some(0.10),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_volatility: 0.15,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err(), "Volatility exceeding limit should fail");
}

// ============================================================================
// RISK CHECK - DRAWDOWN LIMIT TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_drawdown_within_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_drawdown: Some(0.20),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_drawdown: 0.10, // 10% drawdown
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Drawdown within limit should pass");
}

#[test]
fn test_risk_manager_check_drawdown_exceeds_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_drawdown: Some(0.20),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_drawdown: 0.25, // 25% drawdown
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err(), "Drawdown exceeding limit should fail");
}

// ============================================================================
// RISK CHECK - COMBINED LIMITS TESTS
// ============================================================================

#[test]
fn test_risk_manager_check_all_limits_pass() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        max_order_size: Some(1.0),
        max_inventory_skew: Some(2.0),
        max_volatility: Some(0.10),
        max_drawdown: Some(0.20),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 5.0,
        current_order_size: 0.5,
        current_inventory_skew: 1.0,
        current_volatility: 0.05,
        current_drawdown: 0.10,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "All metrics within limits should pass");
}

#[test]
fn test_risk_manager_check_first_violation_returned() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        max_order_size: Some(1.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 15.0, // Exceeds
        current_order_size: 2.0, // Also exceeds
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_err());
    // Error should be about position (first check)
    let error_msg = result.unwrap_err();
    assert!(error_msg.contains("Position"));
}

#[test]
fn test_risk_manager_check_no_limits() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: None,
        max_order_size: None,
        max_inventory_skew: None,
        max_volatility: None,
        max_drawdown: None,
        stop_loss_pct: None,
        max_daily_loss: None,
        ..Default::default()
    });
    
    // Even extreme values should pass when limits are disabled
    let metrics = RiskMetrics {
        current_position: 1000.0,
        current_order_size: 500.0,
        current_inventory_skew: 100.0,
        current_volatility: 1.0,
        current_drawdown: 0.90,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "No limits should allow any values");
}

// ============================================================================
// EDGE CASES
// ============================================================================

#[test]
fn test_risk_manager_check_zero_limit() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(0.0), // Zero limit
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 0.0,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Zero position with zero limit should pass");
}

#[test]
fn test_risk_manager_check_very_small_values() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(0.001),
        max_order_size: Some(0.0001),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 0.0005,
        current_order_size: 0.00005,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Very small values should work correctly");
}

#[test]
fn test_risk_manager_check_very_large_values() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(1_000_000.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_position: 500_000.0,
        ..Default::default()
    };
    
    let result = rm.check(&metrics);
    assert!(result.is_ok(), "Large values should work correctly");
}

// ============================================================================
// SERIALIZATION TESTS
// ============================================================================

#[test]
fn test_risk_limits_serialization() {
    let limits = RiskLimits::default();
    
    // Should be serializable
    let json = serde_json::to_string(&limits).expect("Should serialize");
    assert!(json.contains("max_position"));
    
    // Should be deserializable
    let parsed: RiskLimits = serde_json::from_str(&json).expect("Should deserialize");
    assert_eq!(parsed.max_position, limits.max_position);
}

#[test]
fn test_risk_metrics_serialization() {
    let metrics = RiskMetrics {
        current_position: 5.0,
        current_order_size: 0.5,
        current_inventory_skew: 1.2,
        current_volatility: 0.05,
        current_drawdown: 0.10,
        unrealized_pnl: 100.0,
        daily_pnl: 50.0,
        ..Default::default()
    };
    
    // Should be serializable
    let json = serde_json::to_string(&metrics).expect("Should serialize");
    assert!(json.contains("current_position"));
    
    // Should be deserializable
    let parsed: RiskMetrics = serde_json::from_str(&json).expect("Should deserialize");
    assert!((parsed.current_position - 5.0).abs() < 0.001);
}

// ============================================================================
// EQUITY AND DRAWDOWN TRACKING TESTS
// ============================================================================

#[test]
fn test_risk_manager_initialize() {
    let mut rm = RiskManager::new();
    rm.initialize(10000.0);
    
    // At initialization, drawdown should be 0
    assert!((rm.get_current_drawdown(10000.0) - 0.0).abs() < 0.001);
}

#[test]
fn test_risk_manager_update_equity_increases_peak() {
    let mut rm = RiskManager::new();
    rm.initialize(10000.0);
    
    rm.update_equity(11000.0);
    
    // Drawdown should still be 0 after equity increase
    assert!((rm.get_current_drawdown(11000.0) - 0.0).abs() < 0.001);
}

#[test]
fn test_risk_manager_drawdown_calculation() {
    let mut rm = RiskManager::new();
    rm.initialize(10000.0);
    
    // Simulate equity drop
    let current = 8000.0;
    let drawdown = rm.get_current_drawdown(current);
    
    // 10000 -> 8000 is 20% drawdown
    assert!((drawdown - 0.20).abs() < 0.001);
}

#[test]
fn test_risk_manager_drawdown_after_recovery() {
    let mut rm = RiskManager::new();
    rm.initialize(10000.0);
    
    // Drop to 8000
    rm.update_equity(8000.0);
    assert!((rm.get_current_drawdown(8000.0) - 0.20).abs() < 0.001);
    
    // Recover to 9000
    rm.update_equity(9000.0);
    assert!((rm.get_current_drawdown(9000.0) - 0.10).abs() < 0.001);
    
    // Recover to new high 11000
    rm.update_equity(11000.0);
    assert!((rm.get_current_drawdown(11000.0) - 0.0).abs() < 0.001);
}
