//! Venue representation for smart routing
//!
//! A venue is an exchange or trading venue with its own orderbook, fees, and latency characteristics

use serde::{Serialize, Deserialize};
use crate::order::OrderSide;

/// Configuration for a trading venue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueConfig {
    /// Venue identifier (e.g., "kraken", "coinbase")
    pub name: String,
    /// Taker fee rate (e.g., 0.001 for 0.1%)
    pub taker_fee_rate: f64,
    /// Maker fee rate
    pub maker_fee_rate: f64,
    /// Simulated latency in milliseconds
    pub latency_ms: f64,
    /// Reliability score (0.0 to 1.0)
    pub reliability: f64,
    /// Minimum order size
    pub min_order_size: f64,
    /// Maximum order size
    pub max_order_size: f64,
    /// Whether venue is enabled for routing
    pub enabled: bool,
}

impl Default for VenueConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            taker_fee_rate: 0.001,
            maker_fee_rate: 0.0005,
            latency_ms: 50.0,
            reliability: 0.99,
            min_order_size: 0.0001,
            max_order_size: 1000.0,
            enabled: true,
        }
    }
}

impl VenueConfig {
    /// Create config for Kraken
    pub fn kraken() -> Self {
        Self {
            name: "kraken".to_string(),
            taker_fee_rate: 0.0026,  // 0.26% for low volume
            maker_fee_rate: 0.0016,  // 0.16% for low volume
            latency_ms: 30.0,
            reliability: 0.995,
            min_order_size: 0.0001,
            max_order_size: 500.0,
            enabled: true,
        }
    }
    
    /// Create config for Coinbase
    pub fn coinbase() -> Self {
        Self {
            name: "coinbase".to_string(),
            taker_fee_rate: 0.006,   // 0.6% for low volume
            maker_fee_rate: 0.004,   // 0.4% for low volume
            latency_ms: 25.0,
            reliability: 0.998,
            min_order_size: 0.0001,
            max_order_size: 1000.0,
            enabled: true,
        }
    }
    
    /// Create config for Binance
    pub fn binance() -> Self {
        Self {
            name: "binance".to_string(),
            taker_fee_rate: 0.001,   // 0.1%
            maker_fee_rate: 0.001,   // 0.1%
            latency_ms: 15.0,
            reliability: 0.99,
            min_order_size: 0.00001,
            max_order_size: 10000.0,
            enabled: true,
        }
    }
}

/// Orderbook level (price + quantity)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Level {
    pub price: f64,
    pub quantity: f64,
}

/// Snapshot of a venue's orderbook at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueSnapshot {
    /// Venue configuration
    pub config: VenueConfig,
    /// Trading symbol
    pub symbol: String,
    /// Timestamp of this snapshot
    pub timestamp_ms: u64,
    /// Best bid levels (sorted by price descending)
    pub bids: Vec<Level>,
    /// Best ask levels (sorted by price ascending)
    pub asks: Vec<Level>,
    /// Mid price
    pub mid_price: f64,
    /// Spread in basis points
    pub spread_bps: f64,
}

impl VenueSnapshot {
    /// Create a new venue snapshot from orderbook data
    pub fn new(
        config: VenueConfig,
        symbol: &str,
        timestamp_ms: u64,
        bids: Vec<Level>,
        asks: Vec<Level>,
    ) -> Self {
        let best_bid = bids.first().map(|l| l.price).unwrap_or(0.0);
        let best_ask = asks.first().map(|l| l.price).unwrap_or(0.0);
        let mid_price = if best_bid > 0.0 && best_ask > 0.0 {
            (best_bid + best_ask) / 2.0
        } else {
            0.0
        };
        let spread_bps = if mid_price > 0.0 {
            ((best_ask - best_bid) / mid_price) * 10000.0
        } else {
            0.0
        };
        
        Self {
            config,
            symbol: symbol.to_string(),
            timestamp_ms,
            bids,
            asks,
            mid_price,
            spread_bps,
        }
    }
    
    /// Get best bid price
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|l| l.price)
    }
    
    /// Get best ask price  
    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|l| l.price)
    }
    
    /// Get total bid liquidity up to a price level
    pub fn bid_liquidity(&self, up_to_price: Option<f64>) -> f64 {
        self.bids.iter()
            .filter(|l| up_to_price.map_or(true, |p| l.price >= p))
            .map(|l| l.quantity)
            .sum()
    }
    
    /// Get total ask liquidity up to a price level
    pub fn ask_liquidity(&self, up_to_price: Option<f64>) -> f64 {
        self.asks.iter()
            .filter(|l| up_to_price.map_or(true, |p| l.price <= p))
            .map(|l| l.quantity)
            .sum()
    }
    
    /// Calculate expected fill price for a market order (without fees)
    pub fn expected_fill_price(&self, side: OrderSide, quantity: f64) -> Option<f64> {
        let levels = match side {
            OrderSide::Buy => &self.asks,
            OrderSide::Sell => &self.bids,
        };
        
        let mut remaining = quantity;
        let mut total_cost = 0.0;
        
        for level in levels {
            let fill_qty = remaining.min(level.quantity);
            total_cost += fill_qty * level.price;
            remaining -= fill_qty;
            
            if remaining <= 0.0 {
                break;
            }
        }
        
        if remaining > 0.0 {
            None // Insufficient liquidity
        } else {
            Some(total_cost / quantity)
        }
    }
    
    /// Calculate expected fill price including taker fees
    pub fn expected_fill_price_with_fees(&self, side: OrderSide, quantity: f64) -> Option<f64> {
        self.expected_fill_price(side, quantity).map(|price| {
            match side {
                OrderSide::Buy => price * (1.0 + self.config.taker_fee_rate),
                OrderSide::Sell => price * (1.0 - self.config.taker_fee_rate),
            }
        })
    }
    
    /// Simulate a fill on this venue's orderbook
    /// Returns (fill_price, fill_quantity)
    pub fn simulate_fill(
        &self,
        side: OrderSide,
        quantity: f64,
        limit_price: Option<f64>,
    ) -> (f64, f64) {
        let levels = match side {
            OrderSide::Buy => &self.asks,
            OrderSide::Sell => &self.bids,
        };
        
        let mut remaining = quantity;
        let mut total_cost = 0.0;
        let mut total_filled = 0.0;
        
        for level in levels {
            // Check limit price
            let can_fill = match (side, limit_price) {
                (OrderSide::Buy, Some(limit)) => level.price <= limit,
                (OrderSide::Sell, Some(limit)) => level.price >= limit,
                (_, None) => true,
            };
            
            if !can_fill {
                break;
            }
            
            let fill_qty = remaining.min(level.quantity);
            total_cost += fill_qty * level.price;
            total_filled += fill_qty;
            remaining -= fill_qty;
            
            if remaining <= 0.0 {
                break;
            }
        }
        
        let avg_price = if total_filled > 0.0 { total_cost / total_filled } else { 0.0 };
        (avg_price, total_filled)
    }
    
    /// Calculate market impact of an order (price move caused by the order)
    pub fn market_impact_bps(&self, side: OrderSide, quantity: f64) -> f64 {
        let fill_price = self.expected_fill_price(side, quantity);
        match fill_price {
            Some(price) => ((price - self.mid_price).abs() / self.mid_price) * 10000.0,
            None => f64::MAX,
        }
    }
    
    /// Score this venue for a given order (higher is better)
    /// Considers: price, fees, liquidity, latency
    pub fn score_for_order(&self, side: OrderSide, quantity: f64) -> f64 {
        let fill_price = match self.expected_fill_price_with_fees(side, quantity) {
            Some(p) => p,
            None => return 0.0, // No liquidity = score of 0
        };
        
        // Better price = higher score (inverted for buy orders)
        let price_score = match side {
            OrderSide::Buy => 1.0 / fill_price * 10000.0,  // Lower price = higher score
            OrderSide::Sell => fill_price / 10000.0,        // Higher price = higher score
        };
        
        // Latency penalty (small factor)
        let latency_penalty = 1.0 - (self.config.latency_ms / 1000.0).min(0.1);
        
        // Reliability bonus
        let reliability_bonus = self.config.reliability;
        
        price_score * latency_penalty * reliability_bonus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_snapshot() -> VenueSnapshot {
        let config = VenueConfig::kraken();
        let bids = vec![
            Level { price: 42000.0, quantity: 1.0 },
            Level { price: 41990.0, quantity: 2.0 },
            Level { price: 41980.0, quantity: 3.0 },
        ];
        let asks = vec![
            Level { price: 42010.0, quantity: 1.0 },
            Level { price: 42020.0, quantity: 2.0 },
            Level { price: 42030.0, quantity: 3.0 },
        ];
        VenueSnapshot::new(config, "BTC-USD", 0, bids, asks)
    }
    
    #[test]
    fn test_mid_price_calculation() {
        let snapshot = create_test_snapshot();
        assert_eq!(snapshot.mid_price, 42005.0);
    }
    
    #[test]
    fn test_expected_fill_price_buy() {
        let snapshot = create_test_snapshot();
        // Buy 1 BTC - should fill at best ask 42010
        let price = snapshot.expected_fill_price(OrderSide::Buy, 1.0);
        assert_eq!(price, Some(42010.0));
        
        // Buy 2 BTC - should fill 1@42010 + 1@42020 = avg 42015
        let price = snapshot.expected_fill_price(OrderSide::Buy, 2.0);
        assert_eq!(price, Some(42015.0));
    }
    
    #[test]
    fn test_expected_fill_price_sell() {
        let snapshot = create_test_snapshot();
        // Sell 1 BTC - should fill at best bid 42000
        let price = snapshot.expected_fill_price(OrderSide::Sell, 1.0);
        assert_eq!(price, Some(42000.0));
    }
    
    #[test]
    fn test_insufficient_liquidity() {
        let snapshot = create_test_snapshot();
        // Try to buy 10 BTC - only 6 available
        let price = snapshot.expected_fill_price(OrderSide::Buy, 10.0);
        assert_eq!(price, None);
    }
    
    #[test]
    fn test_simulate_fill_with_limit() {
        let snapshot = create_test_snapshot();
        // Try to buy 3 BTC with limit 42015 - should only fill 1@42010
        let (price, qty) = snapshot.simulate_fill(OrderSide::Buy, 3.0, Some(42015.0));
        assert_eq!(qty, 1.0);
        assert_eq!(price, 42010.0);
    }
}
