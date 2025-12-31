//! Order types for smart routing
//!
//! Defines parent orders (what the strategy wants) and child orders (what gets sent to venues)

use serde::{Serialize, Deserialize};
use uuid::Uuid;

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "BUY"),
            OrderSide::Sell => write!(f, "SELL"),
        }
    }
}

/// Order type
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum OrderType {
    /// Execute at market price immediately
    Market,
    /// Execute at specified price or better
    Limit,
    /// Execute as maker order (post-only)
    PostOnly,
    /// Iceberg order - hide full size
    Iceberg { display_size: f64 },
}

/// Parent order - the original order from the strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentOrder {
    /// Unique identifier
    pub id: String,
    /// Trading symbol (e.g., "BTC-USD")
    pub symbol: String,
    /// Buy or Sell
    pub side: OrderSide,
    /// Total quantity to execute
    pub quantity: f64,
    /// Order type
    pub order_type: OrderType,
    /// Limit price (for limit orders)
    pub limit_price: Option<f64>,
    /// Benchmark price for slippage calculation
    pub benchmark_price: f64,
    /// Creation timestamp
    pub created_at_ms: u64,
    /// Strategy-defined metadata
    pub metadata: Option<String>,
}

impl ParentOrder {
    /// Create a new market order
    pub fn market(symbol: &str, side: OrderSide, quantity: f64, benchmark_price: f64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            side,
            quantity,
            order_type: OrderType::Market,
            limit_price: None,
            benchmark_price,
            created_at_ms: 0,
            metadata: None,
        }
    }
    
    /// Create a new limit order
    pub fn limit(symbol: &str, side: OrderSide, quantity: f64, limit_price: f64, benchmark_price: f64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            symbol: symbol.to_string(),
            side,
            quantity,
            order_type: OrderType::Limit,
            limit_price: Some(limit_price),
            benchmark_price,
            created_at_ms: 0,
            metadata: None,
        }
    }
    
    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp_ms: u64) -> Self {
        self.created_at_ms = timestamp_ms;
        self
    }
    
    /// Set metadata
    pub fn with_metadata(mut self, metadata: &str) -> Self {
        self.metadata = Some(metadata.to_string());
        self
    }
}

/// Child order - a portion of the parent order sent to a specific venue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildOrder {
    /// Unique identifier
    pub id: String,
    /// Parent order ID
    pub parent_id: String,
    /// Target venue/exchange
    pub venue: String,
    /// Trading symbol
    pub symbol: String,
    /// Buy or Sell
    pub side: OrderSide,
    /// Quantity for this venue
    pub quantity: f64,
    /// Order type
    pub order_type: OrderType,
    /// Limit price (adjusted for venue)
    pub limit_price: Option<f64>,
    /// Expected fill price based on orderbook analysis
    pub expected_price: f64,
    /// Expected fees
    pub expected_fees: f64,
    /// Creation timestamp
    pub created_at_ms: u64,
    /// Order status
    pub status: ChildOrderStatus,
}

/// Status of a child order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChildOrderStatus {
    /// Order created, not yet sent
    Pending,
    /// Order sent to venue
    Submitted,
    /// Partially filled
    PartiallyFilled,
    /// Completely filled
    Filled,
    /// Order cancelled
    Cancelled,
    /// Order rejected by venue
    Rejected,
}

impl ChildOrder {
    /// Create a new child order
    pub fn new(
        parent_id: &str,
        venue: &str,
        symbol: &str,
        side: OrderSide,
        quantity: f64,
        order_type: OrderType,
        limit_price: Option<f64>,
        expected_price: f64,
        expected_fees: f64,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id: parent_id.to_string(),
            venue: venue.to_string(),
            symbol: symbol.to_string(),
            side,
            quantity,
            order_type,
            limit_price,
            expected_price,
            expected_fees,
            created_at_ms: 0,
            status: ChildOrderStatus::Pending,
        }
    }
}

/// A fill (execution) of an order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    /// Order ID that was filled
    pub order_id: String,
    /// Venue where fill occurred
    pub venue: String,
    /// Side of the fill
    pub side: OrderSide,
    /// Quantity filled
    pub quantity: f64,
    /// Fill price
    pub price: f64,
    /// Fee charged
    pub fee: f64,
    /// Timestamp of fill
    pub timestamp_ms: u64,
}

impl Fill {
    /// Calculate the total cost of this fill (including fees)
    pub fn total_cost(&self) -> f64 {
        let notional = self.quantity * self.price;
        match self.side {
            OrderSide::Buy => notional + self.fee,
            OrderSide::Sell => notional - self.fee,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_market_order_creation() {
        let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 1.5, 42000.0);
        assert_eq!(order.symbol, "BTC-USD");
        assert_eq!(order.quantity, 1.5);
        assert!(matches!(order.order_type, OrderType::Market));
    }
    
    #[test]
    fn test_limit_order_creation() {
        let order = ParentOrder::limit("ETH-USD", OrderSide::Sell, 10.0, 2500.0, 2510.0);
        assert_eq!(order.limit_price, Some(2500.0));
        assert_eq!(order.benchmark_price, 2510.0);
    }
    
    #[test]
    fn test_fill_cost_calculation() {
        let buy_fill = Fill {
            order_id: "test".to_string(),
            venue: "kraken".to_string(),
            side: OrderSide::Buy,
            quantity: 1.0,
            price: 100.0,
            fee: 0.1,
            timestamp_ms: 0,
        };
        assert_eq!(buy_fill.total_cost(), 100.1);
        
        let sell_fill = Fill {
            order_id: "test".to_string(),
            venue: "kraken".to_string(),
            side: OrderSide::Sell,
            quantity: 1.0,
            price: 100.0,
            fee: 0.1,
            timestamp_ms: 0,
        };
        assert_eq!(sell_fill.total_cost(), 99.9);
    }
}
