//! Integration tests for RiskManager
//! Tests the integration between RiskManager and other trading components

use riskmanager::{RiskManager, RiskLimits, RiskMetrics, RiskAction};

// ============================================================================
// RISK ACTION TESTS
// ============================================================================

#[test]
fn test_check_order_proceed() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        max_order_size: Some(5.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics::default();
    
    let action = rm.check_order(1.0, 0.0, &metrics);
    assert_eq!(action, RiskAction::Proceed);
}

#[test]
fn test_check_order_reduce_position() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_order_size: Some(1.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics::default();
    
    let action = rm.check_order(2.0, 0.0, &metrics);
    match action {
        RiskAction::ReducePosition(size) => {
            assert!((size - 1.0).abs() < 0.001);
        }
        _ => panic!("Expected ReducePosition action"),
    }
}

#[test]
fn test_check_order_reject_exceeds_max_position() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        max_order_size: Some(5.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics::default();
    
    // Current position is at limit, can't add more
    let action = rm.check_order(1.0, 10.0, &metrics);
    match action {
        RiskAction::RejectOrder(reason) => {
            assert!(reason.contains("exceed"));
        }
        _ => panic!("Expected RejectOrder action"),
    }
}

#[test]
fn test_check_order_close_all_on_drawdown() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_drawdown: Some(0.20),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        current_drawdown: 0.25, // 25% drawdown exceeds 20% limit
        ..Default::default()
    };
    
    let action = rm.check_order(1.0, 0.0, &metrics);
    match action {
        RiskAction::CloseAllPositions(reason) => {
            assert!(reason.contains("Drawdown"));
        }
        _ => panic!("Expected CloseAllPositions action"),
    }
}

#[test]
fn test_check_order_halt_on_daily_loss() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_daily_loss: Some(1000.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics {
        daily_pnl: -1500.0, // Lost $1500, exceeds $1000 limit
        ..Default::default()
    };
    
    let action = rm.check_order(1.0, 0.0, &metrics);
    match action {
        RiskAction::HaltTrading(reason) => {
            assert!(reason.contains("Daily loss"));
        }
        _ => panic!("Expected HaltTrading action"),
    }
}

// ============================================================================
// STOP LOSS TESTS
// ============================================================================

#[test]
fn test_stop_loss_long_position_triggered() {
    let rm = RiskManager::with_limits(RiskLimits {
        stop_loss_pct: Some(0.05), // 5% stop loss
        ..Default::default()
    });
    
    let entry_price = 100.0;
    let current_price = 94.0; // 6% loss
    
    let should_stop = rm.check_stop_loss(entry_price, current_price, true);
    assert!(should_stop, "Should trigger stop loss at 6% loss");
}

#[test]
fn test_stop_loss_long_position_not_triggered() {
    let rm = RiskManager::with_limits(RiskLimits {
        stop_loss_pct: Some(0.05), // 5% stop loss
        ..Default::default()
    });
    
    let entry_price = 100.0;
    let current_price = 97.0; // 3% loss
    
    let should_stop = rm.check_stop_loss(entry_price, current_price, true);
    assert!(!should_stop, "Should not trigger stop loss at 3% loss");
}

#[test]
fn test_stop_loss_short_position_triggered() {
    let rm = RiskManager::with_limits(RiskLimits {
        stop_loss_pct: Some(0.05), // 5% stop loss
        ..Default::default()
    });
    
    let entry_price = 100.0;
    let current_price = 106.0; // Price went up 6%, short loses
    
    let should_stop = rm.check_stop_loss(entry_price, current_price, false);
    assert!(should_stop, "Should trigger stop loss for short at 6% loss");
}

#[test]
fn test_stop_loss_short_position_not_triggered() {
    let rm = RiskManager::with_limits(RiskLimits {
        stop_loss_pct: Some(0.05), // 5% stop loss
        ..Default::default()
    });
    
    let entry_price = 100.0;
    let current_price = 103.0; // Price went up 3%
    
    let should_stop = rm.check_stop_loss(entry_price, current_price, false);
    assert!(!should_stop, "Should not trigger stop loss for short at 3% loss");
}

#[test]
fn test_stop_loss_disabled() {
    let rm = RiskManager::with_limits(RiskLimits {
        stop_loss_pct: None,
        ..Default::default()
    });
    
    let entry_price = 100.0;
    let current_price = 50.0; // 50% loss!
    
    let should_stop = rm.check_stop_loss(entry_price, current_price, true);
    assert!(!should_stop, "Stop loss disabled should never trigger");
}

// ============================================================================
// INTEGRATION WITH PORTFOLIO SIMULATION
// ============================================================================

#[test]
fn test_risk_manager_full_workflow() {
    let mut rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(10.0),
        max_order_size: Some(2.0),
        max_drawdown: Some(0.20),
        stop_loss_pct: Some(0.05),
        max_daily_loss: Some(1000.0),
        ..Default::default()
    });
    
    // Initialize with starting equity
    rm.initialize(10000.0);
    
    // Place first order - should proceed
    let metrics = RiskMetrics::default();
    let action = rm.check_order(2.0, 0.0, &metrics);
    assert_eq!(action, RiskAction::Proceed);
    
    // Update equity after profit
    rm.update_equity(10500.0);
    assert!((rm.get_current_drawdown(10500.0) - 0.0).abs() < 0.001);
    
    // Equity drops 
    rm.update_equity(9000.0);
    let dd = rm.get_current_drawdown(9000.0);
    // 10500 -> 9000 is ~14.3% drawdown
    assert!(dd > 0.14 && dd < 0.15);
    
    // Still within limit, can trade
    let metrics = RiskMetrics {
        current_drawdown: dd,
        ..Default::default()
    };
    let result = rm.check(&metrics);
    assert!(result.is_ok());
}

#[test]
fn test_risk_manager_prevents_overleveraging() {
    let rm = RiskManager::with_limits(RiskLimits {
        max_position: Some(5.0),
        max_order_size: Some(3.0),
        ..Default::default()
    });
    
    let metrics = RiskMetrics::default();
    
    // First order of 3 - allowed
    let action = rm.check_order(3.0, 0.0, &metrics);
    assert_eq!(action, RiskAction::Proceed);
    
    // Second order of 3 would exceed max position of 5
    let action = rm.check_order(3.0, 3.0, &metrics);
    match action {
        RiskAction::ReducePosition(size) => {
            assert!((size - 2.0).abs() < 0.001, "Should reduce to 2.0 to stay at limit");
        }
        _ => panic!("Expected ReducePosition action"),
    }
}
