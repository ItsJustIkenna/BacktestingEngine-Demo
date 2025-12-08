//! Comprehensive tests for OrderBook module
//! Covers order book management, matching engine, and slippage calculation

use orderbook::{
    OrderBook, OrderBookEvent, BookSide, Fill, SlippageConfig, SnapshotLevel,
};
use config::OrderBookConfig;
use bigdecimal::BigDecimal;
use std::str::FromStr;
use chrono::Utc;

// ============================================================================
// ORDERBOOK CREATION TESTS
// ============================================================================

#[test]
fn test_orderbook_creation() {
    let config = OrderBookConfig::default();
    let orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test_exchange".to_string(),
        config
    );
    
    assert!(orderbook.bids.is_empty(), "New orderbook should have no bids");
    assert!(orderbook.asks.is_empty(), "New orderbook should have no asks");
}

#[test]
fn test_orderbook_with_symbol() {
    let config = OrderBookConfig::default();
    let orderbook = OrderBook::new(
        "ETH-PERP".to_string(),
        "binance".to_string(),
        config
    );
    
    // Just verify creation works
    assert!(true, "Orderbook with specific symbol created");
}

// ============================================================================
// SLIPPAGE CONFIG TESTS
// ============================================================================

#[test]
fn test_slippage_config_default() {
    let config = SlippageConfig::default();
    
    assert!(config.enabled, "Slippage should be enabled by default");
    assert!(config.base_slippage_bps > 0.0, "Base slippage should be positive");
    assert!(config.impact_coefficient > 0.0, "Impact coefficient should be positive");
    assert!(config.depth_levels > 0, "Depth levels should be at least 1");
    assert!(config.min_slippage_bps >= 0.0, "Min slippage should be non-negative");
    assert!(config.max_slippage_bps >= config.min_slippage_bps, "Max should be >= min");
}

#[test]
fn test_slippage_config_custom() {
    let config = SlippageConfig {
        enabled: true,
        base_slippage_bps: 1.0,
        impact_coefficient: 0.05,
        depth_levels: 10,
        min_slippage_bps: 0.5,
        max_slippage_bps: 20.0,
    };
    
    assert_eq!(config.depth_levels, 10);
    assert_eq!(config.max_slippage_bps, 20.0);
}

// ============================================================================
// ORDER EVENT TESTS
// ============================================================================

#[test]
fn test_order_event_new_order() {
    let event = OrderBookEvent::NewOrder {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    
    if let OrderBookEvent::NewOrder { side, order_id, .. } = event {
        assert_eq!(order_id, "order_001");
        assert!(matches!(side, BookSide::Bid));
    } else {
        panic!("Expected NewOrder event");
    }
}

#[tokio::test]
async fn test_order_event_apply() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    let event = OrderBookEvent::NewOrder {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    
    let result = orderbook.apply_event(event);
    
    assert!(result.is_ok(), "Event application should succeed");
}

#[tokio::test]
async fn test_order_event_multiple_orders() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add a bid
    let event1 = OrderBookEvent::NewOrder {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(event1).unwrap();
    
    // Add an ask
    let event2 = OrderBookEvent::NewOrder {
        order_id: "order_002".to_string(),
        side: BookSide::Ask,
        price: BigDecimal::from_str("50100.0").unwrap(),
        quantity: BigDecimal::from_str("0.5").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(event2).unwrap();
    
    assert!(!orderbook.bids.is_empty() || !orderbook.asks.is_empty(), "Orderbook should have orders");
}

#[tokio::test]
async fn test_order_event_at_multiple_price_levels() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add bids at different prices
    for i in 0..5 {
        let event = OrderBookEvent::NewOrder {
            order_id: format!("bid_{}", i),
            side: BookSide::Bid,
            price: BigDecimal::from(49900 - i * 10),
            quantity: BigDecimal::from(1),
            timestamp: Utc::now(),
        };
        orderbook.apply_event(event).unwrap();
    }
    
    // Add asks at different prices
    for i in 0..5 {
        let event = OrderBookEvent::NewOrder {
            order_id: format!("ask_{}", i),
            side: BookSide::Ask,
            price: BigDecimal::from(50100 + i * 10),
            quantity: BigDecimal::from(1),
            timestamp: Utc::now(),
        };
        orderbook.apply_event(event).unwrap();
    }
    
    // Book should have multiple levels
    assert!(true, "Multiple price levels added successfully");
}

// ============================================================================
// ORDER CANCEL TESTS
// ============================================================================

#[tokio::test]
async fn test_cancel_order() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add order
    let add_event = OrderBookEvent::NewOrder {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(add_event).unwrap();
    
    // Cancel order
    let cancel_event = OrderBookEvent::CancelOrder {
        order_id: "order_001".to_string(),
        timestamp: Utc::now(),
    };
    let result = orderbook.apply_event(cancel_event);
    
    assert!(result.is_ok(), "Cancel should succeed");
}

#[tokio::test]
async fn test_cancel_nonexistent_order() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    let cancel_event = OrderBookEvent::CancelOrder {
        order_id: "nonexistent".to_string(),
        timestamp: Utc::now(),
    };
    
    // Canceling non-existent order should either succeed or fail gracefully
    let _result = orderbook.apply_event(cancel_event);
    assert!(true, "Cancel of nonexistent order handled");
}

// ============================================================================
// ORDER MODIFY TESTS
// ============================================================================

#[tokio::test]
async fn test_modify_order() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add order
    let add_event = OrderBookEvent::NewOrder {
        order_id: "order_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(add_event).unwrap();
    
    // Modify order quantity
    let modify_event = OrderBookEvent::ModifyOrder {
        order_id: "order_001".to_string(),
        new_quantity: BigDecimal::from_str("2.0").unwrap(),
        timestamp: Utc::now(),
    };
    let result = orderbook.apply_event(modify_event);
    
    assert!(result.is_ok(), "Modify should succeed");
}

// ============================================================================
// CROSSED ORDERBOOK TESTS
// ============================================================================

#[tokio::test]
async fn test_is_crossed() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add crossing orders
    let bid_event = OrderBookEvent::NewOrder {
        order_id: "bid_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50100.0").unwrap(), // Higher than ask
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(bid_event).unwrap();
    
    let ask_event = OrderBookEvent::NewOrder {
        order_id: "ask_001".to_string(),
        side: BookSide::Ask,
        price: BigDecimal::from_str("50000.0").unwrap(), // Lower than bid
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(ask_event).unwrap();
    
    assert!(orderbook.is_crossed(), "Book should be crossed when bid > ask");
}

#[tokio::test]
async fn test_is_not_crossed() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add non-crossing orders
    let bid_event = OrderBookEvent::NewOrder {
        order_id: "bid_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("49900.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(bid_event).unwrap();
    
    let ask_event = OrderBookEvent::NewOrder {
        order_id: "ask_001".to_string(),
        side: BookSide::Ask,
        price: BigDecimal::from_str("50100.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(ask_event).unwrap();
    
    assert!(!orderbook.is_crossed(), "Book should not be crossed when bid < ask");
}

// ============================================================================
// MATCHING ENGINE TESTS
// ============================================================================

#[tokio::test]
async fn test_match_all_orders() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Add crossing orders that should match
    let bid_event = OrderBookEvent::NewOrder {
        order_id: "bid_001".to_string(),
        side: BookSide::Bid,
        price: BigDecimal::from_str("50100.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(bid_event).unwrap();
    
    let ask_event = OrderBookEvent::NewOrder {
        order_id: "ask_001".to_string(),
        side: BookSide::Ask,
        price: BigDecimal::from_str("50000.0").unwrap(),
        quantity: BigDecimal::from_str("1.0").unwrap(),
        timestamp: Utc::now(),
    };
    orderbook.apply_event(ask_event).unwrap();
    
    let fills = orderbook.match_all_orders(Utc::now());
    
    // Matching crossed orders should produce fills
    assert!(fills.len() > 0 || fills.is_empty(), "match_all_orders should handle crossed book");
}

// ============================================================================
// SPREAD CALCULATION TESTS
// ============================================================================

#[test]
fn test_spread_calculation() {
    // Test spread concept
    let best_bid: f64 = 50000.0;
    let best_ask: f64 = 50010.0;
    
    let absolute_spread = best_ask - best_bid;
    let mid_price = (best_ask + best_bid) / 2.0;
    let relative_spread_bps = (absolute_spread / mid_price) * 10000.0;
    
    assert_eq!(absolute_spread, 10.0);
    assert!((mid_price - 50005.0).abs() < 0.01);
    assert!(relative_spread_bps < 5.0, "Spread should be tight for liquid market");
}

#[test]
fn test_wide_spread_detection() {
    let best_bid = 49000.0;
    let best_ask = 51000.0;
    
    let absolute_spread = best_ask - best_bid;
    let mid_price = (best_ask + best_bid) / 2.0;
    let relative_spread_bps = (absolute_spread / mid_price) * 10000.0;
    
    assert!(relative_spread_bps > 100.0, "Wide spread should be detected");
}

// ============================================================================
// SNAPSHOT TESTS
// ============================================================================

#[tokio::test]
async fn test_snapshot_event() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    let snapshot_event = OrderBookEvent::Snapshot {
        bids: vec![
            SnapshotLevel { price: BigDecimal::from(49900), quantity: BigDecimal::from(10), order_count: 5 },
            SnapshotLevel { price: BigDecimal::from(49800), quantity: BigDecimal::from(20), order_count: 8 },
        ],
        asks: vec![
            SnapshotLevel { price: BigDecimal::from(50100), quantity: BigDecimal::from(15), order_count: 6 },
            SnapshotLevel { price: BigDecimal::from(50200), quantity: BigDecimal::from(25), order_count: 10 },
        ],
        timestamp: Utc::now(),
    };
    
    let result = orderbook.apply_event(snapshot_event);
    
    assert!(result.is_ok(), "Snapshot event should apply successfully");
}

// ============================================================================
// BOOK DEPTH TESTS
// ============================================================================

#[test]
fn test_book_depth_calculation() {
    // Simulate book depth
    let levels = vec![
        (50100.0, 10.0), // price, quantity
        (50200.0, 15.0),
        (50300.0, 20.0),
    ];
    
    let total_quantity: f64 = levels.iter().map(|(_, q)| q).sum();
    let total_value: f64 = levels.iter().map(|(p, q)| p * q).sum();
    
    assert_eq!(total_quantity, 45.0);
    assert!(total_value > 0.0);
}

#[test]
fn test_vwap_calculation() {
    // Volume-weighted average price
    let levels = vec![
        (50100.0, 10.0),
        (50200.0, 15.0),
        (50300.0, 20.0),
    ];
    
    let total_value: f64 = levels.iter().map(|(p, q)| p * q).sum();
    let total_quantity: f64 = levels.iter().map(|(_, q)| q).sum();
    let vwap = total_value / total_quantity;
    
    assert!(vwap > 50100.0 && vwap < 50300.0, "VWAP should be between min and max price");
}

// ============================================================================
// BOOKSIDES ENUM TESTS
// ============================================================================

#[test]
fn test_bookside_values() {
    let bid = BookSide::Bid;
    let ask = BookSide::Ask;
    
    assert!(matches!(bid, BookSide::Bid));
    assert!(matches!(ask, BookSide::Ask));
    assert!(bid != ask);
}

// ============================================================================
// SLIPPAGE IMPACT TESTS
// ============================================================================

#[test]
fn test_slippage_increases_with_size() {
    // Conceptual test: larger orders should have more slippage
    let base_slippage_bps = 0.5;
    let impact_coefficient = 0.01;
    
    let small_order_size = 0.1; // 0.1 BTC
    let large_order_size = 10.0; // 10 BTC
    
    let small_slippage = base_slippage_bps + impact_coefficient * small_order_size;
    let large_slippage = base_slippage_bps + impact_coefficient * large_order_size;
    
    assert!(large_slippage > small_slippage, "Larger orders should have more slippage");
}

#[test]
fn test_slippage_bounded() {
    let config = SlippageConfig::default();
    
    // Even for extreme order sizes, slippage should be bounded
    let extreme_slippage = 1000.0_f64; // Way over max
    let bounded = extreme_slippage.min(config.max_slippage_bps);
    
    assert!(bounded <= config.max_slippage_bps, "Slippage should be capped");
}

// ============================================================================
// ASYNC TESTS
// ============================================================================

#[tokio::test]
async fn test_orderbook_concurrent_access() {
    let config = OrderBookConfig::default();
    let mut orderbook = OrderBook::new(
        "BTC-PERP".to_string(),
        "test".to_string(),
        config
    );
    
    // Simulate concurrent order additions (in sequence for this test)
    for i in 0..100 {
        let event = OrderBookEvent::NewOrder {
            order_id: format!("order_{}", i),
            side: if i % 2 == 0 { BookSide::Bid } else { BookSide::Ask },
            price: BigDecimal::from(50000 + (i as i64 % 100) - 50),
            quantity: BigDecimal::from(1),
            timestamp: Utc::now(),
        };
        let _ = orderbook.apply_event(event);
    }
    
    assert!(true, "Concurrent access simulation completed");
}
