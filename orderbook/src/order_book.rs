use crate::order_utils;
// Utility for converting Signal to OrderBookEvent moved to order_utils.rs

/// Core order book implementation
///
/// This module provides the main OrderBook structure that manages bid and ask sides
/// and handles all order book operations.

use crate::level::Level;
use crate::types::{BookSide, OrderBookEvent, OrderInfo, SnapshotLevel};
use crate::{DepthLevel, MarketDepth, OrderBookConfig};
use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, warn};

/// Main order book structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    /// Symbol being traded
    pub symbol: String,
    /// Exchange name
    pub exchange: String,
    /// Bid side (buy orders) - ordered by price descending (highest first)
    pub bids: BTreeMap<OrderedFloat<f64>, Level>,
    /// Ask side (sell orders) - ordered by price ascending (lowest first)
    pub asks: BTreeMap<OrderedFloat<f64>, Level>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
    /// Configuration
    config: OrderBookConfig,
    /// Event sequence number
    pub sequence_number: u64,
}

impl OrderBook {

    /// Submit an order from a Signal (convenience for simulation)
    pub fn submit_order_from_signal(&mut self, signal: &signal::Signal, timestamp: DateTime<Utc>) -> anyhow::Result<()> {
        // Use the utility function from order_utils
        let event = order_utils::signal_to_order_event(signal, timestamp);
        self.apply_event(event)
    }

    /// Match all possible orders (simple fill model for backtesting)
    pub fn match_all_orders(&mut self, timestamp: DateTime<Utc>) -> Vec<crate::types::OrderInfo> {
        // Simple model: match best bid and best ask if crossed
        let mut fills = Vec::new();
        while self.is_crossed() {
            let best_bid = self.bids.values().next_back().cloned();
            let best_ask = self.asks.values().next().cloned();
            if let (Some(bid_level), Some(ask_level)) = (best_bid, best_ask) {
                let fill_qty = bid_level.total_quantity.min(ask_level.total_quantity.clone());
                // Remove from both sides
                // (In a real engine, would update orders and levels, here just simulate fill)
                // For now, just record a fill event
                // TODO: Actually update order quantities and remove filled orders
                fills.push(crate::types::OrderInfo {
                    order_id: "sim_fill_bid".to_string(),
                    side: crate::types::BookSide::Bid,
                    original_quantity: fill_qty.clone(),
                    remaining_quantity: BigDecimal::from(0),
                    price: bid_level.price.clone(),
                    timestamp,
                    order_type: crate::types::OrderType::Limit,
                });
                fills.push(crate::types::OrderInfo {
                    order_id: "sim_fill_ask".to_string(),
                    side: crate::types::BookSide::Ask,
                    original_quantity: fill_qty.clone(),
                    remaining_quantity: BigDecimal::from(0),
                    price: ask_level.price.clone(),
                    timestamp,
                    order_type: crate::types::OrderType::Limit,
                });
                // Remove filled quantities (not implemented in this stub)
                break;
            } else {
                break;
            }
        }
        fills
    }
    /// Create a new empty order book
    pub fn new(symbol: String, exchange: String, config: OrderBookConfig) -> Self {
        Self {
            symbol,
            exchange,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_updated: Utc::now(),
            config,
            sequence_number: 0,
        }
    }

    /// Apply an order book event
    pub fn apply_event(&mut self, event: OrderBookEvent) -> Result<()> {
        match event {
            OrderBookEvent::NewOrder {
                order_id,
                side,
                price,
                quantity,
                timestamp,
            } => {
                self.add_order(order_id, side, price, quantity, timestamp)?;
            }
            OrderBookEvent::ModifyOrder {
                order_id,
                new_quantity,
                timestamp,
            } => {
                self.modify_order(&order_id, new_quantity, timestamp)?;
            }
            OrderBookEvent::CancelOrder { order_id, timestamp } => {
                self.cancel_order(&order_id, timestamp)?;
            }
            OrderBookEvent::Trade {
                side,
                price,
                quantity,
                timestamp,
                ..
            } => {
                self.execute_trade(side, price, quantity, timestamp)?;
            }
            OrderBookEvent::Snapshot {
                bids,
                asks,
                timestamp,
            } => {
                self.apply_snapshot(bids, asks, timestamp)?;
            }
        }
        
        self.sequence_number += 1;
        self.cleanup_empty_levels();
        Ok(())
    }

    /// Add a new order to the book
    pub fn add_order(
        &mut self,
        order_id: String,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let price_key = OrderedFloat(price.to_string().parse::<f64>().map_err(|e| {
            anyhow::anyhow!("Failed to convert price to f64: {}", e)
        })?);

        let order = OrderInfo {
            order_id: order_id.clone(),
            side,
            original_quantity: quantity.clone(),
            remaining_quantity: quantity.clone(),
            price: price.clone(),
            timestamp,
            order_type: crate::types::OrderType::Limit,
        };

        let levels = match side {
            BookSide::Bid => &mut self.bids,
            BookSide::Ask => &mut self.asks,
        };

        // Get or create level
        let level = levels
            .entry(price_key)
            .or_insert_with(|| Level::new(price.clone(), side, self.config.track_individual_orders));

        level.add_order(order)?;
        self.last_updated = timestamp;

        debug!(
            "Added order {} to {} side at price {} with quantity {}",
            order_id,
            match side {
                BookSide::Bid => "bid",
                BookSide::Ask => "ask",
            },
            price,
            quantity
        );

        Ok(())
    }

    /// Modify an existing order
    pub fn modify_order(
        &mut self,
        order_id: &str,
        new_quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Search both sides for the order
        for (_, level) in self.bids.iter_mut().chain(self.asks.iter_mut()) {
            if level.orders.as_ref().map_or(false, |orders| orders.contains_key(order_id)) {
                level.modify_order(order_id, new_quantity, timestamp)?;
                self.last_updated = timestamp;
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Order {} not found for modification", order_id))
    }

    /// Cancel an order
    pub fn cancel_order(&mut self, order_id: &str, timestamp: DateTime<Utc>) -> Result<()> {
        // Search both sides for the order
        for (_, level) in self.bids.iter_mut().chain(self.asks.iter_mut()) {
            if level.orders.as_ref().map_or(false, |orders| orders.contains_key(order_id)) {
                level.remove_order(order_id, timestamp)?;
                self.last_updated = timestamp;
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Order {} not found for cancellation", order_id))
    }

    /// Execute a trade against the book
    pub fn execute_trade(
        &mut self,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<Vec<OrderInfo>> {
        let price_key = OrderedFloat(price.to_string().parse::<f64>().map_err(|e| {
            anyhow::anyhow!("Failed to convert price to f64: {}", e)
        })?);

        let levels = match side {
            BookSide::Bid => &mut self.bids,
            BookSide::Ask => &mut self.asks,
        };

        if let Some(level) = levels.get_mut(&price_key) {
            let executed_orders = level.execute_trade(&quantity, timestamp)?;
            self.last_updated = timestamp;
            Ok(executed_orders)
        } else {
            warn!("Trade execution attempted at non-existent price level: {}", price);
            Ok(Vec::new())
        }
    }

    /// Apply a complete snapshot to rebuild the book
    pub fn apply_snapshot(
        &mut self,
        bids: Vec<SnapshotLevel>,
        asks: Vec<SnapshotLevel>,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Clear existing levels
        self.bids.clear();
        self.asks.clear();

        // Rebuild from snapshot
        for bid in bids {
            let price_key = OrderedFloat(bid.price.to_string().parse::<f64>().map_err(|e| {
                anyhow::anyhow!("Failed to convert bid price to f64: {}", e)
            })?);
            
            let mut level = Level::new(bid.price.clone(), BookSide::Bid, false);
            level.total_quantity = bid.quantity;
            level.order_count = bid.order_count;
            level.last_updated = timestamp;
            
            self.bids.insert(price_key, level);
        }

        for ask in asks {
            let price_key = OrderedFloat(ask.price.to_string().parse::<f64>().map_err(|e| {
                anyhow::anyhow!("Failed to convert ask price to f64: {}", e)
            })?);
            
            let mut level = Level::new(ask.price.clone(), BookSide::Ask, false);
            level.total_quantity = ask.quantity;
            level.order_count = ask.order_count;
            level.last_updated = timestamp;
            
            self.asks.insert(price_key, level);
        }

        self.last_updated = timestamp;
        Ok(())
    }

    /// Get market depth information
    pub fn get_market_depth(&self, max_levels: Option<usize>) -> MarketDepth {
        let limit = max_levels.unwrap_or(self.config.max_levels.unwrap_or(100));

        let bids: Vec<DepthLevel> = self
            .bids
            .iter()
            .rev() // Highest bids first
            .take(limit)
            .map(|(price, level)| DepthLevel {
                price: price.0,
                quantity: level.total_quantity.to_string().parse().unwrap_or(0.0),
                order_count: level.order_count,
            })
            .collect();

        let asks: Vec<DepthLevel> = self
            .asks
            .iter() // Lowest asks first
            .take(limit)
            .map(|(price, level)| DepthLevel {
                price: price.0,
                quantity: level.total_quantity.to_string().parse().unwrap_or(0.0),
                order_count: level.order_count,
            })
            .collect();

        let best_bid = bids.first().cloned();
        let best_ask = asks.first().cloned();
        let spread = match (&best_bid, &best_ask) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        };

        MarketDepth {
            timestamp: self.last_updated,
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
            bids,
            asks,
            best_bid,
            best_ask,
            spread,
        }
    }

    /// Get the best bid price
    pub fn best_bid(&self) -> Option<&BigDecimal> {
        self.bids.values().next_back().map(|level| &level.price)
    }

    /// Get the best ask price
    pub fn best_ask(&self) -> Option<&BigDecimal> {
        self.asks.values().next().map(|level| &level.price)
    }

    /// Get the current spread
    pub fn spread(&self) -> Option<BigDecimal> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Remove empty levels from the book
    fn cleanup_empty_levels(&mut self) {
        self.bids.retain(|_, level| !level.is_empty());
        self.asks.retain(|_, level| !level.is_empty());
    }

    /// Check if the order book is crossed (best bid >= best ask)
    pub fn is_crossed(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid >= ask,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BookSide;

    #[test]
    fn test_order_book_creation() {
        let config = OrderBookConfig::default();
        let book = OrderBook::new("BTCUSD".to_string(), "TEST".to_string(), config);
        
        assert_eq!(book.symbol, "BTCUSD");
        assert_eq!(book.exchange, "TEST");
        assert!(book.bids.is_empty());
        assert!(book.asks.is_empty());
    }

    #[test]
    fn test_add_orders() {
        let config = OrderBookConfig::default();
        let mut book = OrderBook::new("BTCUSD".to_string(), "TEST".to_string(), config);
        
        // Add bid
        book.add_order(
            "bid1".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            Utc::now(),
        ).unwrap();
        
        // Add ask
        book.add_order(
            "ask1".to_string(),
            BookSide::Ask,
            BigDecimal::from(105),
            BigDecimal::from(15),
            Utc::now(),
        ).unwrap();
        
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
        assert_eq!(book.best_bid(), Some(&BigDecimal::from(100)));
        assert_eq!(book.best_ask(), Some(&BigDecimal::from(105)));
    }
}
