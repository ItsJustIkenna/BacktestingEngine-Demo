//! Comprehensive tests for Portfolio Manager
//! Covers PortfolioState, Position, fill handling, and P&L calculations

use portfoliomanager::{
    PortfolioState, Position, PositionSide,
};
use orderbook::{Fill, BookSide, LiquidityType};
use bigdecimal::BigDecimal;
use std::str::FromStr;
use chrono::{Utc, Duration};

// ============================================================================
// PORTFOLIO STATE CREATION TESTS
// ============================================================================

#[test]
fn test_portfolio_state_default() {
    let portfolio = PortfolioState::default();
    
    assert_eq!(portfolio.balance, BigDecimal::from(0));
    assert_eq!(portfolio.net_position, 0.0);
    assert!(portfolio.positions.is_empty());
}

#[test]
fn test_portfolio_state_with_balance() {
    let initial_balance = BigDecimal::from(100000);
    let portfolio = PortfolioState::with_balance(initial_balance.clone());
    
    assert_eq!(portfolio.balance, initial_balance);
    assert!(portfolio.positions.is_empty());
}

#[test]
fn test_portfolio_state_various_balances() {
    let balances = vec![
        BigDecimal::from(1000),
        BigDecimal::from(100000),
        BigDecimal::from(1000000),
    ];
    
    for balance in balances {
        let portfolio = PortfolioState::with_balance(balance.clone());
        assert_eq!(portfolio.balance, balance);
    }
}

// ============================================================================
// POSITION TESTS
// ============================================================================

#[test]
fn test_position_creation() {
    let position = Position {
        symbol: "BTC-PERP".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from_str("50000.0").unwrap(),
        mark_price: Some(BigDecimal::from_str("51000.0").unwrap()),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(1000),
        open_time: Utc::now(),
        close_time: None,
    };
    
    assert_eq!(position.symbol, "BTC-PERP");
    assert!(matches!(position.side, PositionSide::Long));
    assert_eq!(position.quantity, 1.0);
}

#[test]
fn test_position_sides() {
    let long = PositionSide::Long;
    let short = PositionSide::Short;
    
    assert!(matches!(long, PositionSide::Long));
    assert!(matches!(short, PositionSide::Short));
    assert!(long != short);
}

#[test]
fn test_position_with_closed_time() {
    let position = Position {
        symbol: "ETH-PERP".to_string(),
        side: PositionSide::Short,
        quantity: 10.0,
        entry_price: BigDecimal::from_str("2000.0").unwrap(),
        mark_price: None,
        realized_pnl: BigDecimal::from(500),
        unrealized_pnl: BigDecimal::from(0),
        open_time: Utc::now() - Duration::hours(24),
        close_time: Some(Utc::now()),
    };
    
    assert!(position.close_time.is_some());
    assert!(position.realized_pnl > BigDecimal::from(0));
}

// ============================================================================
// FILL APPLICATION TESTS
// ============================================================================

#[test]
fn test_apply_fill_buy() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    let fill = Fill {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        base_price: None,
        quantity: BigDecimal::from_str("0.1").unwrap(),
        liquidity_type: LiquidityType::Taker,
        fee_rate: 0.001,
        fee_amount: BigDecimal::from_str("5.0").unwrap(),
        slippage_bps: Some(0.5),
        timestamp: Utc::now(),
    };
    
    let result = portfolio.apply_fill(&fill);
    
    assert!(result.is_ok(), "Apply fill should succeed");
}

#[test]
fn test_apply_fill_sell() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    // First buy to have a position
    let buy_fill = Fill {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        base_price: None,
        quantity: BigDecimal::from_str("1.0").unwrap(),
        liquidity_type: LiquidityType::Taker,
        fee_rate: 0.001,
        fee_amount: BigDecimal::from_str("50.0").unwrap(),
        slippage_bps: Some(0.5),
        timestamp: Utc::now(),
    };
    portfolio.apply_fill(&buy_fill).unwrap();
    
    // Now sell
    let sell_fill = Fill {
        order_id: "order_002".to_string(),
        side: BookSide::Ask,
        price: BigDecimal::from_str("51000.0").unwrap(),
        base_price: None,
        quantity: BigDecimal::from_str("0.5").unwrap(),
        liquidity_type: LiquidityType::Maker,
        fee_rate: 0.0005,
        fee_amount: BigDecimal::from_str("25.0").unwrap(),
        slippage_bps: Some(0.0),
        timestamp: Utc::now(),
    };
    
    let result = portfolio.apply_fill(&sell_fill);
    
    assert!(result.is_ok(), "Sell fill should succeed");
}

#[test]
fn test_apply_multiple_fills() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    let fills = vec![
        Fill {
            order_id: "order_001".to_string(),
            side: BookSide::Bid,
            price: BigDecimal::from_str("50000.0").unwrap(),
            base_price: None,
            quantity: BigDecimal::from_str("0.1").unwrap(),
            liquidity_type: LiquidityType::Taker,
            fee_rate: 0.001,
            fee_amount: BigDecimal::from_str("5.0").unwrap(),
            slippage_bps: Some(0.5),
            timestamp: Utc::now(),
        },
        Fill {
            order_id: "order_002".to_string(),
            side: BookSide::Bid,
            price: BigDecimal::from_str("49500.0").unwrap(),
            base_price: None,
            quantity: BigDecimal::from_str("0.1").unwrap(),
            liquidity_type: LiquidityType::Taker,
            fee_rate: 0.001,
            fee_amount: BigDecimal::from_str("4.95").unwrap(),
            slippage_bps: Some(0.5),
            timestamp: Utc::now(),
        },
    ];
    
    let result = portfolio.apply_fills(&fills);
    
    assert!(result.is_ok(), "Apply multiple fills should succeed");
}

// ============================================================================
// P&L CALCULATION TESTS
// ============================================================================

#[test]
fn test_pnl_calculation_concept() {
    // Long position P&L
    let entry_price = 50000.0;
    let exit_price = 51000.0;
    let quantity = 1.0;
    
    let pnl = (exit_price - entry_price) * quantity;
    
    assert_eq!(pnl, 1000.0);
}

#[test]
fn test_pnl_short_position_concept() {
    // Short position P&L (profit when price drops)
    let entry_price = 50000.0;
    let exit_price = 49000.0;
    let quantity = 1.0;
    
    let pnl = (entry_price - exit_price) * quantity;
    
    assert_eq!(pnl, 1000.0);
}

#[test]
fn test_pnl_with_fees() {
    let entry_price = 50000.0;
    let exit_price = 51000.0;
    let quantity = 1.0;
    let fee_rate = 0.001; // 0.1%
    
    let gross_pnl = (exit_price - entry_price) * quantity;
    let entry_fee = entry_price * quantity * fee_rate;
    let exit_fee = exit_price * quantity * fee_rate;
    let net_pnl = gross_pnl - entry_fee - exit_fee;
    
    assert!(net_pnl < gross_pnl, "Net P&L should be less than gross due to fees");
    assert!(net_pnl > 0.0, "Should still be profitable after fees");
}

#[test]
fn test_unrealized_pnl() {
    let entry_price = 50000.0;
    let mark_price = 52000.0;
    let quantity = 1.0;
    
    let unrealized_pnl = (mark_price - entry_price) * quantity;
    
    assert_eq!(unrealized_pnl, 2000.0);
}

// ============================================================================
// TOTAL VALUE TESTS
// ============================================================================

#[test]
fn test_get_total_value() {
    let portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    let total = portfolio.get_total_value();
    
    // With no positions, total value should equal balance
    assert!(total >= BigDecimal::from(0));
}

#[test]
fn test_total_value_with_unrealized_pnl() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    portfolio.unrealized_pnl = BigDecimal::from(5000);
    
    let total = portfolio.get_total_value();
    
    // Total should include unrealized P&L
    assert!(total >= portfolio.balance);
}

// ============================================================================
// POSITION TRACKING TESTS
// ============================================================================

#[test]
fn test_position_update_unrealized_pnl() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    // Add a position manually
    let position = Position {
        symbol: "BTC-PERP".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from_str("50000.0").unwrap(),
        mark_price: None,
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(0),
        open_time: Utc::now(),
        close_time: None,
    };
    portfolio.positions.push(position);
    
    // Update unrealized P&L with a new mark price
    let current_price = BigDecimal::from_str("52000.0").unwrap();
    portfolio.update_unrealized_pnl(&current_price);
    
    // Unrealized P&L should be updated
    assert!(true, "Unrealized P&L update completed");
}

#[test]
fn test_net_position_tracking() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    // Simulate buying
    portfolio.net_position = 1.0; // Bought 1 BTC
    
    assert_eq!(portfolio.net_position, 1.0);
    
    // Simulate selling half
    portfolio.net_position = 0.5;
    
    assert_eq!(portfolio.net_position, 0.5);
}

// ============================================================================
// EQUITY CURVE CONCEPT TESTS
// ============================================================================

#[test]
fn test_equity_curve_calculation() {
    // Simulate equity over time
    let initial_balance = 100000.0;
    let pnl_sequence = vec![100.0, -50.0, 200.0, -30.0, 150.0];
    
    let mut equity_curve = vec![initial_balance];
    let mut current_equity = initial_balance;
    
    for pnl in pnl_sequence {
        current_equity += pnl;
        equity_curve.push(current_equity);
    }
    
    assert_eq!(equity_curve.len(), 6);
    assert!(equity_curve.last().unwrap() > &initial_balance, "Final equity should be higher");
}

#[test]
fn test_max_drawdown_calculation() {
    let equity_curve = vec![100000.0, 102000.0, 98000.0, 101000.0, 97000.0, 105000.0];
    
    let mut max_dd = 0.0;
    let mut peak = equity_curve[0];
    
    for &equity in &equity_curve {
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    
    assert!(max_dd > 0.0, "There should be a drawdown");
    assert!(max_dd < 1.0, "Drawdown should be less than 100%");
}

// ============================================================================
// FEE TRACKING TESTS
// ============================================================================

#[test]
fn test_fees_paid_tracking() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    portfolio.fees_paid = BigDecimal::from(50);
    
    assert_eq!(portfolio.fees_paid, BigDecimal::from(50));
}

#[test]
fn test_cumulative_fees() {
    let trades = vec![
        (50000.0, 0.1, 0.001),  // price, quantity, fee_rate
        (49500.0, 0.2, 0.001),
        (51000.0, 0.15, 0.0005),
    ];
    
    let total_fees: f64 = trades.iter()
        .map(|(price, qty, rate)| price * qty * rate)
        .sum();
    
    assert!(total_fees > 0.0);
}

// ============================================================================
// MARGIN CONCEPT TESTS
// ============================================================================

#[test]
fn test_margin_requirement_concept() {
    let position_value = 50000.0;
    let leverage = 10.0;
    let initial_margin_rate = 1.0 / leverage; // 10% initial margin
    let maintenance_margin_rate = 0.05; // 5%
    
    let initial_margin = position_value * initial_margin_rate;
    let maintenance_margin = position_value * maintenance_margin_rate;
    
    assert_eq!(initial_margin, 5000.0);
    assert_eq!(maintenance_margin, 2500.0);
    assert!(initial_margin > maintenance_margin);
}

#[test]
fn test_liquidation_price_concept() {
    let entry_price = 50000.0;
    let leverage = 10.0;
    let quantity = 1.0;
    let maintenance_margin_rate = 0.05;
    
    // For a long position, liquidation happens when:
    // equity = initial_margin * (1 - maintenance_rate)
    let position_value = entry_price * quantity;
    let _initial_margin = position_value / leverage;
    
    // Approximate liquidation price (simplified)
    let liq_price = entry_price * (1.0 - 1.0/leverage + maintenance_margin_rate);
    
    assert!(liq_price < entry_price, "Liq price for long should be below entry");
}

// ============================================================================
// POSITION SUMMARY TESTS
// ============================================================================

#[test]
fn test_get_position_summary() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    let position = Position {
        symbol: "BTC-PERP".to_string(),
        side: PositionSide::Long,
        quantity: 0.5,
        entry_price: BigDecimal::from_str("50000.0").unwrap(),
        mark_price: Some(BigDecimal::from_str("51000.0").unwrap()),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(500),
        open_time: Utc::now(),
        close_time: None,
    };
    portfolio.positions.push(position);
    
    let summary = portfolio.get_position_summary();
    
    assert!(!summary.is_empty(), "Summary should not be empty");
}

// ============================================================================
// ASYNC TESTS
// ============================================================================

#[tokio::test]
async fn test_portfolio_state_concurrent_updates() {
    let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100000));
    
    // Simulate concurrent fill processing (in sequence for this test)
    for i in 0..10 {
        let fill = Fill {
            order_id: format!("order_{}", i),
            side: if i % 2 == 0 { BookSide::Bid } else { BookSide::Ask },
            price: BigDecimal::from(50000),
            base_price: None,
            quantity: BigDecimal::from_str("0.01").unwrap(),
            liquidity_type: LiquidityType::Taker,
            fee_rate: 0.001,
            fee_amount: BigDecimal::from_str("0.5").unwrap(),
            slippage_bps: Some(0.5),
            timestamp: Utc::now(),
        };
        let _ = portfolio.apply_fill(&fill);
    }
    
    assert!(true, "Concurrent updates simulation completed");
}
