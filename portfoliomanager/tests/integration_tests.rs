//! Integration tests for the portfoliomanager module

use portfoliomanager::{PortfolioState, Position, PositionSide};
use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;

#[test]
fn test_portfolio_state_default() {
    let state = PortfolioState::default();
    
    assert_eq!(state.balance, BigDecimal::from(0));
    assert_eq!(state.net_position, 0.0);
    assert!(state.positions.is_empty());
    assert_eq!(state.realized_pnl, BigDecimal::from(0));
    assert_eq!(state.unrealized_pnl, BigDecimal::from(0));
    assert_eq!(state.fees_paid, BigDecimal::from(0));
}

#[test]
fn test_portfolio_state_creation() {
    let timestamp = Utc::now();
    let state = PortfolioState {
        timestamp,
        balance: BigDecimal::from(10000),
        net_position: 1.0,
        positions: Vec::new(),
        realized_pnl: BigDecimal::from(100),
        unrealized_pnl: BigDecimal::from(50),
        fees_paid: BigDecimal::from(10),
    };
    
    assert_eq!(state.balance, BigDecimal::from(10000));
    assert_eq!(state.net_position, 1.0);
    assert_eq!(state.realized_pnl, BigDecimal::from(100));
    assert_eq!(state.unrealized_pnl, BigDecimal::from(50));
    assert_eq!(state.fees_paid, BigDecimal::from(10));
    assert_eq!(state.timestamp, timestamp);
}

#[test]
fn test_position_creation() {
    let open_time = Utc::now();
    let position = Position {
        symbol: "BTC/USDT".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from(50000),
        mark_price: Some(BigDecimal::from(52000)),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(2000),
        open_time,
        close_time: None,
    };
    
    assert_eq!(position.symbol, "BTC/USDT");
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, 1.0);
    assert_eq!(position.entry_price, BigDecimal::from(50000));
    assert_eq!(position.mark_price, Some(BigDecimal::from(52000)));
    assert_eq!(position.realized_pnl, BigDecimal::from(0));
    assert_eq!(position.unrealized_pnl, BigDecimal::from(2000));
    assert_eq!(position.open_time, open_time);
    assert!(position.close_time.is_none());
}

#[test]
fn test_position_side_equality() {
    assert_eq!(PositionSide::Long, PositionSide::Long);
    assert_eq!(PositionSide::Short, PositionSide::Short);
    assert_ne!(PositionSide::Long, PositionSide::Short);
}

#[test]
fn test_position_side_serialization() {
    let long_side = PositionSide::Long;
    let short_side = PositionSide::Short;
    
    let long_json = serde_json::to_string(&long_side).unwrap();
    let short_json = serde_json::to_string(&short_side).unwrap();
    
    assert!(long_json.contains("Long"));
    assert!(short_json.contains("Short"));
    
    // Test deserialization
    let deserialized_long: PositionSide = serde_json::from_str(&long_json).unwrap();
    let deserialized_short: PositionSide = serde_json::from_str(&short_json).unwrap();
    
    assert_eq!(deserialized_long, PositionSide::Long);
    assert_eq!(deserialized_short, PositionSide::Short);
}

#[test]
fn test_portfolio_state_serialization() {
    let timestamp = Utc::now();
    let state = PortfolioState {
        timestamp,
        balance: BigDecimal::from(10000),
        net_position: 1.0,
        positions: Vec::new(),
        realized_pnl: BigDecimal::from(100),
        unrealized_pnl: BigDecimal::from(50),
        fees_paid: BigDecimal::from(10),
    };
    
    let json_str = serde_json::to_string(&state).unwrap();
    assert!(json_str.contains("balance"));
    assert!(json_str.contains("net_position"));
    assert!(json_str.contains("realized_pnl"));
    
    // Test deserialization
    let deserialized: PortfolioState = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.balance, state.balance);
    assert_eq!(deserialized.net_position, state.net_position);
    assert_eq!(deserialized.realized_pnl, state.realized_pnl);
}

#[test]
fn test_position_serialization() {
    let position = Position {
        symbol: "ETH/USDT".to_string(),
        side: PositionSide::Short,
        quantity: 5.0,
        entry_price: BigDecimal::from(3000),
        mark_price: Some(BigDecimal::from(2900)),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(500),
        open_time: Utc::now(),
        close_time: None,
    };
    
    let json_str = serde_json::to_string(&position).unwrap();
    assert!(json_str.contains("ETH/USDT"));
    assert!(json_str.contains("Short"));
    assert!(json_str.contains("quantity"));
    
    // Test deserialization
    let deserialized: Position = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.symbol, position.symbol);
    assert_eq!(deserialized.side, position.side);
    assert_eq!(deserialized.quantity, position.quantity);
}

#[test]
fn test_portfolio_with_positions() {
    let position1 = Position {
        symbol: "BTC/USDT".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from(50000),
        mark_price: Some(BigDecimal::from(52000)),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(2000),
        open_time: Utc::now(),
        close_time: None,
    };
    
    let position2 = Position {
        symbol: "ETH/USDT".to_string(),
        side: PositionSide::Short,
        quantity: 10.0,
        entry_price: BigDecimal::from(3000),
        mark_price: Some(BigDecimal::from(2900)),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(1000),
        open_time: Utc::now(),
        close_time: None,
    };
    
    let state = PortfolioState {
        timestamp: Utc::now(),
        balance: BigDecimal::from(10000),
        net_position: 1.0,
        positions: vec![position1, position2],
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(3000),
        fees_paid: BigDecimal::from(20),
    };
    
    assert_eq!(state.positions.len(), 2);
    assert_eq!(state.positions[0].symbol, "BTC/USDT");
    assert_eq!(state.positions[1].symbol, "ETH/USDT");
    assert_eq!(state.positions[0].side, PositionSide::Long);
    assert_eq!(state.positions[1].side, PositionSide::Short);
}

#[test]
fn test_closed_position() {
    let open_time = Utc::now();
    let close_time = Utc::now();
    
    let position = Position {
        symbol: "BTC/USDT".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from(50000),
        mark_price: Some(BigDecimal::from(55000)),
        realized_pnl: BigDecimal::from(5000),
        unrealized_pnl: BigDecimal::from(0),
        open_time,
        close_time: Some(close_time),
    };
    
    assert_eq!(position.realized_pnl, BigDecimal::from(5000));
    assert_eq!(position.unrealized_pnl, BigDecimal::from(0));
    assert!(position.close_time.is_some());
    assert_eq!(position.close_time.unwrap(), close_time);
}

#[test]
fn test_big_decimal_operations() {
    let balance = BigDecimal::from(10000);
    let pnl = BigDecimal::from(500);
    
    // Test basic operations
    let new_balance = &balance + &pnl;
    assert_eq!(new_balance, BigDecimal::from(10500));
    
    // Test in position
    let position = Position {
        symbol: "TEST/USDT".to_string(),
        side: PositionSide::Long,
        quantity: 1.0,
        entry_price: BigDecimal::from(100),
        mark_price: Some(BigDecimal::from(110)),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from(10),
        open_time: Utc::now(),
        close_time: None,
    };
    
    assert_eq!(position.entry_price, BigDecimal::from(100));
    assert_eq!(position.mark_price, Some(BigDecimal::from(110)));
}
