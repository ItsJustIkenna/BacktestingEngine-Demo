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

    /// Match all possible orders (comprehensive matching engine)
    pub fn match_all_orders(&mut self, timestamp: DateTime<Utc>) -> Vec<crate::types::OrderInfo> {
        let mut fills = Vec::new();
        
        while self.is_crossed() {
            // Get the best bid and ask price keys
            let best_bid_key = self.bids.keys().next_back().copied();
            let best_ask_key = self.asks.keys().next().copied();
            
            if let (Some(bid_key), Some(ask_key)) = (best_bid_key, best_ask_key) {
                // Get the matching price (use the ask price as the execution price)
                let execution_price = self.asks.get(&ask_key).unwrap().price.clone();
                
                // Calculate the maximum quantity that can be matched
                let bid_quantity = self.bids.get(&bid_key).unwrap().total_quantity.clone();
                let ask_quantity = self.asks.get(&ask_key).unwrap().total_quantity.clone();
                let match_quantity = bid_quantity.min(ask_quantity);
                
                if match_quantity <= BigDecimal::from(0) {
                    break;
                }
                
                // Execute trade on ask side (taking liquidity)
                let ask_executed_orders = {
                    let ask_level = self.asks.get_mut(&ask_key).unwrap();
                    ask_level.execute_trade(&match_quantity, timestamp).unwrap_or_default()
                };
                
                // Execute trade on bid side (taking liquidity)  
                let bid_executed_orders = {
                    let bid_level = self.bids.get_mut(&bid_key).unwrap();
                    bid_level.execute_trade(&match_quantity, timestamp).unwrap_or_default()
                };
                
                // Create fill records for bid orders (they bought at execution_price)
                for bid_order in bid_executed_orders {
                    fills.push(crate::types::OrderInfo {
                        order_id: bid_order.order_id,
                        side: crate::types::BookSide::Bid,
                        original_quantity: bid_order.original_quantity,
                        remaining_quantity: bid_order.remaining_quantity, // This is the filled quantity
                        price: execution_price.clone(),
                        timestamp,
                        order_type: bid_order.order_type,
                    });
                }
                
                // Create fill records for ask orders (they sold at execution_price)
                for ask_order in ask_executed_orders {
                    fills.push(crate::types::OrderInfo {
                        order_id: ask_order.order_id,
                        side: crate::types::BookSide::Ask,
                        original_quantity: ask_order.original_quantity,
                        remaining_quantity: ask_order.remaining_quantity, // This is the filled quantity
                        price: execution_price.clone(),
                        timestamp,
                        order_type: ask_order.order_type,
                    });
                }
                
                // Remove empty levels
                if self.bids.get(&bid_key).map_or(true, |level| level.is_empty()) {
                    self.bids.remove(&bid_key);
                }
                if self.asks.get(&ask_key).map_or(true, |level| level.is_empty()) {
                    self.asks.remove(&ask_key);
                }
                
                self.last_updated = timestamp;
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

    /// Execute a market order against the book
    pub fn execute_market_order(
        &mut self,
        order_id: String,
        side: BookSide,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<Vec<crate::types::OrderInfo>> {
        let mut fills = Vec::new();
        let mut remaining_quantity = quantity.clone();
        
        // Get the appropriate side of the book to execute against
        let levels_to_execute = match side {
            BookSide::Bid => {
                // Market buy order (bid side) executes against asks (starting from lowest price)
                self.asks.clone()
            }
            BookSide::Ask => {
                // Market sell order (ask side) executes against bids (starting from highest price)  
                self.bids.clone()
            }
        };
        
        // Execute against levels in price-time priority
        let price_keys: Vec<_> = match side {
            BookSide::Bid => levels_to_execute.keys().cloned().collect(), // Ascending for asks
            BookSide::Ask => levels_to_execute.keys().rev().cloned().collect(), // Descending for bids
        };
        
        for price_key in price_keys {
            if remaining_quantity <= BigDecimal::from(0) {
                break;
            }
            
            let execution_quantity = {
                let levels = match side {
                    BookSide::Bid => &self.asks,
                    BookSide::Ask => &self.bids,
                };
                
                if let Some(level) = levels.get(&price_key) {
                    remaining_quantity.clone().min(level.total_quantity.clone())
                } else {
                    continue;
                }
            };
            
            if execution_quantity > BigDecimal::from(0) {
                // Execute the trade
                let _executed_orders = {
                    let levels = match side {
                        BookSide::Bid => &mut self.asks,
                        BookSide::Ask => &mut self.bids,
                    };
                    
                    if let Some(level) = levels.get_mut(&price_key) {
                        level.execute_trade(&execution_quantity, timestamp)?
                    } else {
                        Vec::new()
                    }
                };
                
                // Create fill record for the market order
                let execution_price = match side {
                    BookSide::Bid => self.asks.get(&price_key).unwrap().price.clone(),
                    BookSide::Ask => self.bids.get(&price_key).unwrap().price.clone(),
                };
                
                fills.push(crate::types::OrderInfo {
                    order_id: order_id.clone(),
                    side,
                    original_quantity: quantity.clone(),
                    remaining_quantity: execution_quantity.clone(),
                    price: execution_price,
                    timestamp,
                    order_type: crate::types::OrderType::Market,
                });
                
                remaining_quantity -= &execution_quantity;
            }
        }
        
        // Remove empty levels after execution
        self.cleanup_empty_levels();
        self.last_updated = timestamp;
        
        Ok(fills)
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
