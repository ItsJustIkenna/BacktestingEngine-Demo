//! Queue Position Modeling for realistic order fill simulation
//!
//! Models the FIFO queue position at each price level to simulate realistic
//! order fills based on time priority. This is critical for limit order strategies
//! where queue position significantly impacts fill probability.

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::BookSide;

/// Queue position for an order at a specific price level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePosition {
    /// Order ID
    pub order_id: String,
    /// Price level
    pub price: BigDecimal,
    /// Side of book
    pub side: BookSide,
    /// Order quantity
    pub quantity: BigDecimal,
    /// Quantity ahead of this order in queue
    pub quantity_ahead: BigDecimal,
    /// Total quantity at this level when order was placed
    pub total_level_quantity: BigDecimal,
    /// Queue position (0 = front of queue)
    pub position: usize,
    /// Total orders at this level when order was placed
    pub total_orders_at_level: usize,
    /// Timestamp when order entered queue
    pub entry_time: DateTime<Utc>,
    /// Estimated fill probability based on queue position
    pub estimated_fill_probability: f64,
}

impl QueuePosition {
    /// Calculate fill probability based on queue position and market activity
    pub fn calculate_fill_probability(&self, traded_volume: &BigDecimal) -> f64 {
        if self.quantity_ahead <= BigDecimal::from(0) {
            // Front of queue - very high probability if any volume trades
            return if traded_volume > &BigDecimal::from(0) { 0.95 } else { 0.1 };
        }
        
        // Probability based on how much of the queue ahead has been filled
        let quantity_ahead_f64: f64 = self.quantity_ahead.to_string().parse().unwrap_or(f64::MAX);
        let traded_f64: f64 = traded_volume.to_string().parse().unwrap_or(0.0);
        
        if quantity_ahead_f64 <= 0.0 {
            return 0.95;
        }
        
        // Probability that we get filled = P(traded_volume > quantity_ahead)
        // Simple model: linear interpolation up to 2x queue depth
        let fill_ratio = traded_f64 / quantity_ahead_f64;
        
        if fill_ratio >= 1.0 {
            // Enough volume to reach our order
            (0.5 + 0.5 * (fill_ratio - 1.0).min(1.0)).min(0.99)
        } else {
            // Not enough volume yet
            fill_ratio * 0.5
        }
    }
}

/// Tracks queue positions for all orders across price levels
#[derive(Debug, Clone, Default)]
pub struct QueuePositionTracker {
    /// Order ID -> Queue Position mapping
    positions: HashMap<String, QueuePosition>,
    /// Price level -> list of order IDs in queue order (FIFO)
    level_queues: HashMap<(BookSide, String), Vec<String>>, // Key: (side, price_string)
    /// Cumulative traded volume at each level since last update
    traded_volume: HashMap<(BookSide, String), BigDecimal>,
}

impl QueuePositionTracker {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add an order to the queue at a price level
    pub fn add_order(
        &mut self,
        order_id: String,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        existing_level_quantity: BigDecimal,
        existing_order_count: usize,
        timestamp: DateTime<Utc>,
    ) -> QueuePosition {
        let price_key = price.to_string();
        let level_key = (side, price_key.clone());
        
        // Get current queue at this level
        let queue = self.level_queues.entry(level_key.clone()).or_insert_with(Vec::new);
        let position = queue.len();
        
        // Calculate quantity ahead (existing orders at this price level)
        let quantity_ahead = existing_level_quantity.clone();
        
        // Estimate fill probability based on queue position
        let total_qty_f64: f64 = existing_level_quantity.to_string().parse().unwrap_or(1.0);
        let estimated_fill_probability = if total_qty_f64 <= 0.0 {
            0.95 // Front of queue
        } else {
            // Rough estimate: probability decreases with queue depth
            0.8 / (1.0 + (position as f64 * 0.1))
        };
        
        let queue_position = QueuePosition {
            order_id: order_id.clone(),
            price: price.clone(),
            side,
            quantity,
            quantity_ahead,
            total_level_quantity: existing_level_quantity,
            position,
            total_orders_at_level: existing_order_count + 1,
            entry_time: timestamp,
            estimated_fill_probability,
        };
        
        // Add to tracking structures
        queue.push(order_id.clone());
        self.positions.insert(order_id, queue_position.clone());
        
        queue_position
    }
    
    /// Remove an order from the queue
    pub fn remove_order(&mut self, order_id: &str) -> Option<QueuePosition> {
        if let Some(position) = self.positions.remove(order_id) {
            let price_key = position.price.to_string();
            let level_key = (position.side, price_key);
            
            if let Some(queue) = self.level_queues.get_mut(&level_key) {
                queue.retain(|id| id != order_id);
                
                // Update positions of orders behind the removed order
                for (idx, remaining_id) in queue.iter().enumerate() {
                    if let Some(remaining_pos) = self.positions.get_mut(remaining_id) {
                        remaining_pos.position = idx;
                        // Recalculate quantity ahead (simplified)
                        if remaining_pos.quantity_ahead > position.quantity {
                            remaining_pos.quantity_ahead -= &position.quantity;
                        }
                    }
                }
            }
            
            Some(position)
        } else {
            None
        }
    }
    
    /// Record traded volume at a price level (for fill probability calculation)
    pub fn record_trade(
        &mut self,
        side: BookSide,
        price: &BigDecimal,
        quantity: &BigDecimal,
    ) {
        let price_key = price.to_string();
        let level_key = (side, price_key);
        
        let current = self.traded_volume.entry(level_key).or_insert_with(|| BigDecimal::from(0));
        *current += quantity;
    }
    
    /// Get updated fill probability for an order based on traded volume
    pub fn get_fill_probability(&self, order_id: &str) -> Option<f64> {
        let position = self.positions.get(order_id)?;
        let price_key = position.price.to_string();
        let level_key = (position.side, price_key);
        
        let traded = self.traded_volume
            .get(&level_key)
            .cloned()
            .unwrap_or_else(|| BigDecimal::from(0));
        
        Some(position.calculate_fill_probability(&traded))
    }
    
    /// Get queue position for an order
    pub fn get_position(&self, order_id: &str) -> Option<&QueuePosition> {
        self.positions.get(order_id)
    }
    
    /// Check if an order should be filled based on queue position and traded volume
    pub fn should_fill(&self, order_id: &str, rng_value: f64) -> bool {
        if let Some(prob) = self.get_fill_probability(order_id) {
            rng_value <= prob
        } else {
            false
        }
    }
    
    /// Reset traded volume tracking (call at start of each time step)
    pub fn reset_traded_volume(&mut self) {
        self.traded_volume.clear();
    }
    
    /// Get all orders at a price level in queue order
    pub fn get_queue_at_level(&self, side: BookSide, price: &BigDecimal) -> Vec<&QueuePosition> {
        let price_key = price.to_string();
        let level_key = (side, price_key);
        
        if let Some(queue) = self.level_queues.get(&level_key) {
            queue.iter()
                .filter_map(|id| self.positions.get(id))
                .collect()
        } else {
            Vec::new()
        }
    }
    
    /// Simulate fill based on FIFO queue and traded volume
    /// Returns the quantity that would actually be filled
    pub fn simulate_fifo_fill(
        &self,
        order_id: &str,
        total_traded_at_level: &BigDecimal,
    ) -> BigDecimal {
        if let Some(position) = self.positions.get(order_id) {
            // Check if enough volume traded to reach our queue position
            if total_traded_at_level > &position.quantity_ahead {
                // Volume exceeds orders ahead - we get a fill
                let available = total_traded_at_level - &position.quantity_ahead;
                
                // Fill up to our order quantity
                if available >= position.quantity {
                    position.quantity.clone()
                } else {
                    available
                }
            } else {
                // Not enough volume to reach us
                BigDecimal::from(0)
            }
        } else {
            BigDecimal::from(0)
        }
    }
}

/// Configuration for queue position modeling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePositionConfig {
    /// Enable queue position tracking
    pub enabled: bool,
    /// Assume position in queue as fraction of total (0.0 = front, 1.0 = back)
    /// Used when joining existing levels without order-by-order data
    pub assumed_queue_position_fraction: f64,
    /// Minimum fill probability even when deep in queue
    pub minimum_fill_probability: f64,
    /// Whether to use stochastic fill based on probability
    pub use_stochastic_fills: bool,
}

impl Default for QueuePositionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            assumed_queue_position_fraction: 0.5, // Assume middle of queue
            minimum_fill_probability: 0.01,       // 1% minimum
            use_stochastic_fills: true,
        }
    }
}

/// Estimates queue position when joining an existing level
/// (when we don't have order-by-order data)
pub fn estimate_queue_position(
    level_quantity: &BigDecimal,
    order_count: usize,
    config: &QueuePositionConfig,
) -> (BigDecimal, usize) {
    let level_qty_f64: f64 = level_quantity.to_string().parse().unwrap_or(0.0);
    let quantity_ahead = level_qty_f64 * config.assumed_queue_position_fraction;
    let position = (order_count as f64 * config.assumed_queue_position_fraction).ceil() as usize;
    
    (
        BigDecimal::try_from(quantity_ahead).unwrap_or(BigDecimal::from(0)),
        position,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_queue_position_tracking() {
        let mut tracker = QueuePositionTracker::new();
        
        // Add orders at same price level
        let pos1 = tracker.add_order(
            "order1".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            BigDecimal::from(0),  // First order, no quantity ahead
            0,
            Utc::now(),
        );
        
        assert_eq!(pos1.position, 0);
        assert_eq!(pos1.quantity_ahead, BigDecimal::from(0));
        
        // Add second order
        let pos2 = tracker.add_order(
            "order2".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(5),
            BigDecimal::from(10),  // order1's quantity
            1,
            Utc::now(),
        );
        
        assert_eq!(pos2.position, 1);
        assert_eq!(pos2.quantity_ahead, BigDecimal::from(10));
    }
    
    #[test]
    fn test_fifo_fill_simulation() {
        let mut tracker = QueuePositionTracker::new();
        
        // Add two orders
        tracker.add_order(
            "order1".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            BigDecimal::from(50),  // 50 units ahead
            5,
            Utc::now(),
        );
        
        // Simulate trading 40 units - not enough to fill order1
        let fill = tracker.simulate_fifo_fill("order1", &BigDecimal::from(40));
        assert_eq!(fill, BigDecimal::from(0));
        
        // Simulate trading 55 units - 5 units reach order1
        let fill = tracker.simulate_fifo_fill("order1", &BigDecimal::from(55));
        assert_eq!(fill, BigDecimal::from(5));
        
        // Simulate trading 70 units - full fill
        let fill = tracker.simulate_fifo_fill("order1", &BigDecimal::from(70));
        assert_eq!(fill, BigDecimal::from(10));
    }
    
    #[test]
    fn test_fill_probability() {
        let mut tracker = QueuePositionTracker::new();
        
        // Front of queue - high probability
        let pos1 = tracker.add_order(
            "front".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            BigDecimal::from(0),
            0,
            Utc::now(),
        );
        
        assert!(pos1.estimated_fill_probability > 0.8);
        
        // Deep in queue - lower probability
        let pos2 = tracker.add_order(
            "back".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            BigDecimal::from(1000),
            50,
            Utc::now(),
        );
        
        assert!(pos2.estimated_fill_probability < pos1.estimated_fill_probability);
    }
}
