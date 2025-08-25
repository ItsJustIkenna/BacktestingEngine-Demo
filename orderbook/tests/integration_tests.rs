//! Integration tests for the orderbook module core data structures

use orderbook::{MarketDepth, DepthLevel};
use chrono::{Utc, TimeZone};

#[test]
fn test_depth_level_creation() {
    let level = DepthLevel {
        price: 50000.0,
        quantity: 1.5,
        order_count: 3,
    };
    
    assert_eq!(level.price, 50000.0);
    assert_eq!(level.quantity, 1.5);
    assert_eq!(level.order_count, 3);
}

#[test]
fn test_market_depth_creation() {
    let timestamp = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
    let bid_level = DepthLevel {
        price: 49999.0,
        quantity: 2.0,
        order_count: 5,
    };
    let ask_level = DepthLevel {
        price: 50001.0,
        quantity: 1.8,
        order_count: 4,
    };
    
    let market_depth = MarketDepth {
        timestamp,
        symbol: "BTC/USDT".to_string(),
        exchange: "binance".to_string(),
        bids: vec![bid_level.clone()],
        asks: vec![ask_level.clone()],
        best_bid: Some(bid_level.clone()),
        best_ask: Some(ask_level.clone()),
        spread: Some(2.0), // 50001.0 - 49999.0
    };
    
    assert_eq!(market_depth.symbol, "BTC/USDT");
    assert_eq!(market_depth.exchange, "binance");
    assert_eq!(market_depth.bids.len(), 1);
    assert_eq!(market_depth.asks.len(), 1);
    assert_eq!(market_depth.spread, Some(2.0));
    assert_eq!(market_depth.timestamp, timestamp);
}

#[test]
fn test_market_depth_empty_book() {
    let market_depth = MarketDepth {
        timestamp: Utc::now(),
        symbol: "ETH/USDT".to_string(),
        exchange: "coinbase".to_string(),
        bids: Vec::new(),
        asks: Vec::new(),
        best_bid: None,
        best_ask: None,
        spread: None,
    };
    
    assert_eq!(market_depth.symbol, "ETH/USDT");
    assert_eq!(market_depth.exchange, "coinbase");
    assert!(market_depth.bids.is_empty());
    assert!(market_depth.asks.is_empty());
    assert!(market_depth.best_bid.is_none());
    assert!(market_depth.best_ask.is_none());
    assert!(market_depth.spread.is_none());
}

#[test]
fn test_depth_level_serialization() {
    let level = DepthLevel {
        price: 3000.0,
        quantity: 5.0,
        order_count: 10,
    };
    
    let json_str = serde_json::to_string(&level).unwrap();
    assert!(json_str.contains("3000"));
    assert!(json_str.contains("price"));
    assert!(json_str.contains("quantity"));
    
    // Test deserialization
    let deserialized: DepthLevel = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.price, level.price);
    assert_eq!(deserialized.quantity, level.quantity);
    assert_eq!(deserialized.order_count, level.order_count);
}

#[test]
fn test_market_depth_serialization() {
    let bid_level = DepthLevel {
        price: 49999.0,
        quantity: 2.0,
        order_count: 5,
    };
    let ask_level = DepthLevel {
        price: 50001.0,
        quantity: 1.8,
        order_count: 4,
    };
    
    let market_depth = MarketDepth {
        timestamp: Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap(),
        symbol: "BTC/USDT".to_string(),
        exchange: "binance".to_string(),
        bids: vec![bid_level],
        asks: vec![ask_level],
        best_bid: None,
        best_ask: None,
        spread: Some(2.0),
    };
    
    let json_str = serde_json::to_string(&market_depth).unwrap();
    assert!(json_str.contains("BTC/USDT"));
    assert!(json_str.contains("binance"));
    assert!(json_str.contains("bids"));
    assert!(json_str.contains("asks"));
    
    // Test deserialization
    let deserialized: MarketDepth = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.symbol, market_depth.symbol);
    assert_eq!(deserialized.exchange, market_depth.exchange);
    assert_eq!(deserialized.bids.len(), market_depth.bids.len());
    assert_eq!(deserialized.asks.len(), market_depth.asks.len());
}

#[test]
fn test_multiple_depth_levels() {
    let bid_levels = vec![
        DepthLevel { price: 50000.0, quantity: 1.0, order_count: 1 },
        DepthLevel { price: 49999.0, quantity: 2.0, order_count: 3 },
        DepthLevel { price: 49998.0, quantity: 1.5, order_count: 2 },
    ];
    
    let ask_levels = vec![
        DepthLevel { price: 50001.0, quantity: 0.8, order_count: 2 },
        DepthLevel { price: 50002.0, quantity: 1.2, order_count: 1 },
        DepthLevel { price: 50003.0, quantity: 2.5, order_count: 4 },
    ];
    
    let market_depth = MarketDepth {
        timestamp: Utc::now(),
        symbol: "ETH/USDT".to_string(),
        exchange: "binance".to_string(),
        bids: bid_levels.clone(),
        asks: ask_levels.clone(),
        best_bid: Some(bid_levels[0].clone()),
        best_ask: Some(ask_levels[0].clone()),
        spread: Some(1.0), // 50001.0 - 50000.0
    };
    
    assert_eq!(market_depth.bids.len(), 3);
    assert_eq!(market_depth.asks.len(), 3);
    
    // Verify bid ordering (should be descending)
    assert!(market_depth.bids[0].price > market_depth.bids[1].price);
    assert!(market_depth.bids[1].price > market_depth.bids[2].price);
    
    // Verify ask ordering (should be ascending)
    assert!(market_depth.asks[0].price < market_depth.asks[1].price);
    assert!(market_depth.asks[1].price < market_depth.asks[2].price);
}

#[test]
fn test_best_bid_ask_calculation() {
    let bids = vec![
        DepthLevel { price: 50000.0, quantity: 1.0, order_count: 1 },
        DepthLevel { price: 49999.0, quantity: 2.0, order_count: 3 },
    ];
    
    let asks = vec![
        DepthLevel { price: 50001.0, quantity: 0.8, order_count: 2 },
        DepthLevel { price: 50002.0, quantity: 1.2, order_count: 1 },
    ];
    
    let market_depth = MarketDepth {
        timestamp: Utc::now(),
        symbol: "BTC/USDT".to_string(),
        exchange: "binance".to_string(),
        bids: bids.clone(),
        asks: asks.clone(),
        best_bid: Some(bids[0].clone()), // Highest bid
        best_ask: Some(asks[0].clone()), // Lowest ask
        spread: Some(1.0),
    };
    
    let best_bid = market_depth.best_bid.unwrap();
    let best_ask = market_depth.best_ask.unwrap();
    
    assert_eq!(best_bid.price, 50000.0);
    assert_eq!(best_ask.price, 50001.0);
    
    // Spread should be ask - bid
    let calculated_spread = best_ask.price - best_bid.price;
    assert_eq!(calculated_spread, 1.0);
    assert_eq!(market_depth.spread.unwrap(), calculated_spread);
}

#[test]
fn test_depth_level_edge_cases() {
    // Zero quantity level
    let zero_level = DepthLevel {
        price: 50000.0,
        quantity: 0.0,
        order_count: 0,
    };
    
    assert_eq!(zero_level.quantity, 0.0);
    assert_eq!(zero_level.order_count, 0);
    
    // Very small quantity
    let small_level = DepthLevel {
        price: 50000.0,
        quantity: 0.00000001,
        order_count: 1,
    };
    
    assert_eq!(small_level.quantity, 0.00000001);
    
    // High order count
    let high_count_level = DepthLevel {
        price: 50000.0,
        quantity: 100.0,
        order_count: 1000,
    };
    
    assert_eq!(high_count_level.order_count, 1000);
}
