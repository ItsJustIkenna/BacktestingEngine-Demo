//! Order book level management
//!
//! A level represents all orders at a specific price point in the order book.

use crate::types::{BookSide, OrderInfo};
use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Represents a single price level in the order book
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    /// Price of this level
    pub price: BigDecimal,
    /// Side of the book (bid or ask)
    pub side: BookSide,
    /// Total quantity at this level
    pub total_quantity: BigDecimal,
    /// Number of individual orders at this level
    pub order_count: u32,
    /// Individual orders at this level (if tracking is enabled)
    pub orders: Option<IndexMap<String, OrderInfo>>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

impl Level {
    /// Create a new empty level
    pub fn new(price: BigDecimal, side: BookSide, track_orders: bool) -> Self {
        Self {
            price,
            side,
            total_quantity: BigDecimal::from(0),
            order_count: 0,
            orders: if track_orders { Some(IndexMap::new()) } else { None },
            last_updated: Utc::now(),
        }
    }

    /// Add an order to this level
    pub fn add_order(&mut self, order: OrderInfo) -> Result<()> {
        // Validate price matches level
        if order.price != self.price {
            return Err(anyhow::anyhow!(
                "Order price {} does not match level price {}",
                order.price,
                self.price
            ));
        }

        // Validate side matches level
        if order.side != self.side {
            return Err(anyhow::anyhow!(
                "Order side {:?} does not match level side {:?}",
                order.side,
                self.side
            ));
        }

        // Add to quantity and count
        self.total_quantity += &order.remaining_quantity;
        self.order_count += 1;
        self.last_updated = order.timestamp;

        // Track individual order if enabled
        if let Some(ref mut orders) = self.orders {
            orders.insert(order.order_id.clone(), order);
        }

        Ok(())
    }

    /// Remove an order from this level
    pub fn remove_order(&mut self, order_id: &str, timestamp: DateTime<Utc>) -> Result<Option<OrderInfo>> {
        let removed_order = if let Some(ref mut orders) = self.orders {
            orders.shift_remove(order_id)
        } else {
            return Err(anyhow::anyhow!(
                "Cannot remove specific order - order tracking is disabled"
            ));
        };

        if let Some(ref order) = removed_order {
            self.total_quantity -= &order.remaining_quantity;
            self.order_count -= 1;
            self.last_updated = timestamp;
        }

        Ok(removed_order)
    }

    /// Modify an order's quantity
    pub fn modify_order(&mut self, order_id: &str, new_quantity: BigDecimal, timestamp: DateTime<Utc>) -> Result<()> {
        if let Some(ref mut orders) = self.orders {
            if let Some(order) = orders.get_mut(order_id) {
                let quantity_diff = &new_quantity - &order.remaining_quantity;
                order.remaining_quantity = new_quantity;
                self.total_quantity += &quantity_diff;
                self.last_updated = timestamp;
                Ok(())
            } else {
                Err(anyhow::anyhow!("Order {} not found at this level", order_id))
            }
        } else {
            Err(anyhow::anyhow!(
                "Cannot modify specific order - order tracking is disabled"
            ))
        }
    }

    /// Execute a trade against this level (removes quantity)
    pub fn execute_trade(&mut self, quantity: &BigDecimal, timestamp: DateTime<Utc>) -> Result<Vec<OrderInfo>> {
        let mut executed_orders = Vec::new();
        let mut remaining_quantity = quantity.clone();

        if let Some(ref mut orders) = self.orders {
            // Execute against orders in FIFO order
            let mut orders_to_remove = Vec::new();
            
            for (order_id, order) in orders.iter_mut() {
                if remaining_quantity <= BigDecimal::from(0) {
                    break;
                }

                let fill_quantity = if order.remaining_quantity <= remaining_quantity {
                    // Full fill
                    let fill_qty = order.remaining_quantity.clone();
                    order.remaining_quantity = BigDecimal::from(0);
                    orders_to_remove.push(order_id.clone());
                    fill_qty
                } else {
                    // Partial fill
                    order.remaining_quantity -= &remaining_quantity;
                    remaining_quantity.clone()
                };

                self.total_quantity -= &fill_quantity;
                remaining_quantity -= &fill_quantity;

                // Create executed order info
                executed_orders.push(OrderInfo {
                    order_id: order.order_id.clone(),
                    side: order.side,
                    original_quantity: order.original_quantity.clone(),
                    remaining_quantity: fill_quantity,
                    price: order.price.clone(),
                    timestamp,
                    order_type: order.order_type,
                    liquidity_type: None, // Will be set by the caller
                });
            }

            // Remove fully filled orders
            for order_id in orders_to_remove {
                orders.shift_remove(&order_id);
                self.order_count -= 1;
            }
        } else {
            // Simple quantity subtraction without order tracking
            self.total_quantity -= quantity;
            if self.total_quantity < BigDecimal::from(0) {
                self.total_quantity = BigDecimal::from(0);
            }
        }

        self.last_updated = timestamp;
        Ok(executed_orders)
    }

    /// Check if this level is empty
    pub fn is_empty(&self) -> bool {
        self.total_quantity <= BigDecimal::from(0) || self.order_count == 0
    }

    /// Get the best order at this level (first in queue)
    pub fn get_best_order(&self) -> Option<&OrderInfo> {
        if let Some(ref orders) = self.orders {
            orders.values().next()
        } else {
            None
        }
    }

    /// Get all orders at this level
    pub fn get_orders(&self) -> Vec<&OrderInfo> {
        if let Some(ref orders) = self.orders {
            orders.values().collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BookSide, OrderType};

    fn create_test_order(id: &str, price: i32, quantity: i32, side: BookSide) -> OrderInfo {
        OrderInfo {
            order_id: id.to_string(),
            side,
            original_quantity: BigDecimal::from(quantity),
            remaining_quantity: BigDecimal::from(quantity),
            price: BigDecimal::from(price),
            timestamp: Utc::now(),
            order_type: OrderType::Limit,
        }
    }

    #[test]
    fn test_level_creation() {
        let level = Level::new(BigDecimal::from(100), BookSide::Bid, true);
        assert_eq!(level.price, BigDecimal::from(100));
        assert_eq!(level.side, BookSide::Bid);
        assert_eq!(level.total_quantity, BigDecimal::from(0));
        assert_eq!(level.order_count, 0);
        assert!(level.orders.is_some());
    }

    #[test]
    fn test_add_order() {
        let mut level = Level::new(BigDecimal::from(100), BookSide::Bid, true);
        let order = create_test_order("order1", 100, 50, BookSide::Bid);
        
        level.add_order(order).unwrap();
        
        assert_eq!(level.total_quantity, BigDecimal::from(50));
        assert_eq!(level.order_count, 1);
    }

    #[test]
    fn test_remove_order() {
        let mut level = Level::new(BigDecimal::from(100), BookSide::Bid, true);
        let order = create_test_order("order1", 100, 50, BookSide::Bid);
        
        level.add_order(order).unwrap();
        let removed = level.remove_order("order1", Utc::now()).unwrap();
        
        assert!(removed.is_some());
        assert_eq!(level.total_quantity, BigDecimal::from(0));
        assert_eq!(level.order_count, 0);
    }
}
