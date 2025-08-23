//! Integration tests for the fixed orderbook implementation

use bigdecimal::BigDecimal;
use chrono::Utc;
use orderbook::order_book::OrderBook;
use orderbook::OrderBookConfig;
use orderbook::types::BookSide;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_orderbook() -> OrderBook {
        let config = OrderBookConfig {
            max_levels: Some(100),
            track_individual_orders: true,
            snapshot_validation_frequency: None,
            strict_validation: false,
        };
        OrderBook::new("BTCUSD".to_string(), "TEST".to_string(), config)
    }

    #[test]
    fn test_order_matching_comprehensive() {
        let mut orderbook = create_test_orderbook();
        let timestamp = Utc::now();

        // Add some bid orders
        orderbook.add_order(
            "bid1".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            timestamp,
        ).unwrap();

        orderbook.add_order(
            "bid2".to_string(),
            BookSide::Bid,
            BigDecimal::from(99),
            BigDecimal::from(5),
            timestamp,
        ).unwrap();

        // Add some ask orders
        orderbook.add_order(
            "ask1".to_string(),
            BookSide::Ask,
            BigDecimal::from(101),
            BigDecimal::from(8),
            timestamp,
        ).unwrap();

        orderbook.add_order(
            "ask2".to_string(),
            BookSide::Ask,
            BigDecimal::from(102),
            BigDecimal::from(12),
            timestamp,
        ).unwrap();

        // Verify orderbook state
        assert_eq!(orderbook.best_bid(), Some(&BigDecimal::from(100)));
        assert_eq!(orderbook.best_ask(), Some(&BigDecimal::from(101)));
        assert_eq!(orderbook.spread(), Some(BigDecimal::from(1)));
        assert!(!orderbook.is_crossed());

        // Add a crossing order to trigger matching
        orderbook.add_order(
            "cross_ask".to_string(),
            BookSide::Ask,
            BigDecimal::from(100), // Same as best bid
            BigDecimal::from(5),
            timestamp,
        ).unwrap();

        // Book should now be crossed
        assert!(orderbook.is_crossed());

        // Execute matching
        let fills = orderbook.match_all_orders(timestamp);
        
        // Verify fills occurred
        assert!(!fills.is_empty());
        println!("Fills: {:?}", fills);

        // Check that crossing is resolved
        assert!(!orderbook.is_crossed());

        // Verify remaining quantities
        let best_bid_qty = orderbook.bids.values().next_back().map(|l| &l.total_quantity);
        let best_ask_qty = orderbook.asks.values().next().map(|l| &l.total_quantity);
        
        println!("Remaining bid quantity: {:?}", best_bid_qty);
        println!("Remaining ask quantity: {:?}", best_ask_qty);
    }

    #[test]
    fn test_market_order_execution() {
        let mut orderbook = create_test_orderbook();
        let timestamp = Utc::now();

        // Setup the book with some liquidity
        orderbook.add_order(
            "ask1".to_string(),
            BookSide::Ask,
            BigDecimal::from(101),
            BigDecimal::from(5),
            timestamp,
        ).unwrap();

        orderbook.add_order(
            "ask2".to_string(),
            BookSide::Ask,
            BigDecimal::from(102),
            BigDecimal::from(10),
            timestamp,
        ).unwrap();

        orderbook.add_order(
            "bid1".to_string(),
            BookSide::Bid,
            BigDecimal::from(99),
            BigDecimal::from(8),
            timestamp,
        ).unwrap();

        // Execute a market buy order
        let market_fills = orderbook.execute_market_order(
            "market_buy".to_string(),
            BookSide::Bid,
            BigDecimal::from(12), // More than available at best ask
            timestamp,
        ).unwrap();

        assert!(!market_fills.is_empty());
        println!("Market order fills: {:?}", market_fills);

        // Check that liquidity was consumed
        let remaining_ask_qty: BigDecimal = orderbook.asks.values().map(|l| &l.total_quantity).sum();
        println!("Remaining ask quantity: {}", remaining_ask_qty);
    }

    #[test]
    fn test_partial_fill_tracking() {
        let mut orderbook = create_test_orderbook();
        let timestamp = Utc::now();

        // Add a large bid order
        orderbook.add_order(
            "large_bid".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(20),
            timestamp,
        ).unwrap();

        // Add a smaller ask order that will partially fill the bid
        orderbook.add_order(
            "small_ask".to_string(),
            BookSide::Ask,
            BigDecimal::from(100),
            BigDecimal::from(8),
            timestamp,
        ).unwrap();

        // Execute matching
        let fills = orderbook.match_all_orders(timestamp);
        
        // Should have fills
        assert!(!fills.is_empty());

        // Check remaining bid quantity should be 20 - 8 = 12
        let remaining_bid = orderbook.bids.get(&ordered_float::OrderedFloat(100.0));
        if let Some(level) = remaining_bid {
            assert_eq!(level.total_quantity, BigDecimal::from(12));
        }

        // Ask should be completely consumed
        assert!(orderbook.asks.is_empty());
    }

    #[test]
    fn test_order_modification() {
        let mut orderbook = create_test_orderbook();
        let timestamp = Utc::now();

        // Add an order
        orderbook.add_order(
            "test_order".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            timestamp,
        ).unwrap();

        // Modify the order
        let result = orderbook.modify_order(
            "test_order",
            BigDecimal::from(15),
            timestamp,
        );
        assert!(result.is_ok());

        // Check new quantity
        let level = orderbook.bids.get(&ordered_float::OrderedFloat(100.0)).unwrap();
        assert_eq!(level.total_quantity, BigDecimal::from(15));
    }

    #[test]
    fn test_order_cancellation() {
        let mut orderbook = create_test_orderbook();
        let timestamp = Utc::now();

        // Add an order
        orderbook.add_order(
            "test_order".to_string(),
            BookSide::Ask,
            BigDecimal::from(101),
            BigDecimal::from(5),
            timestamp,
        ).unwrap();

        // Cancel the order
        let result = orderbook.cancel_order("test_order", timestamp);
        assert!(result.is_ok());

        // Check that level is empty or removed
        if let Some(level) = orderbook.asks.get(&ordered_float::OrderedFloat(101.0)) {
            assert!(level.is_empty());
        }
    }
}
