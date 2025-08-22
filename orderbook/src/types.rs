//! Core types and enums for order book operations

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use bigdecimal::BigDecimal;

/// Side of the order book (bid or ask)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookSide {
    Bid,
    Ask,
}

impl BookSide {
    /// Get the opposite side
    pub fn opposite(&self) -> Self {
        match self {
            BookSide::Bid => BookSide::Ask,
            BookSide::Ask => BookSide::Bid,
        }
    }

    /// Convert from string representation
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BID" | "BUY" | "B" => Some(BookSide::Bid),
            "ASK" | "SELL" | "S" | "OFFER" => Some(BookSide::Ask),
            _ => None,
        }
    }
}

/// Type of order book event
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OrderBookEvent {
    /// New order added to book
    NewOrder {
        order_id: String,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    },
    /// Order quantity modified
    ModifyOrder {
        order_id: String,
        new_quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    },
    /// Order cancelled/removed
    CancelOrder {
        order_id: String,
        timestamp: DateTime<Utc>,
    },
    /// Trade executed (partial or full fill)
    Trade {
        aggressor_order_id: Option<String>,
        passive_order_id: Option<String>,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    },
    /// Complete order book snapshot
    Snapshot {
        bids: Vec<SnapshotLevel>,
        asks: Vec<SnapshotLevel>,
        timestamp: DateTime<Utc>,
    },
}

/// Level in an order book snapshot
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotLevel {
    pub price: BigDecimal,
    pub quantity: BigDecimal,
    pub order_count: u32,
}

/// Complete order book snapshot at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub symbol: String,
    pub exchange: String,
    pub timestamp: DateTime<Utc>,
    pub bids: Vec<SnapshotLevel>,
    pub asks: Vec<SnapshotLevel>,
    pub sequence_number: Option<u64>,
}

/// Individual order information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderInfo {
    pub order_id: String,
    pub side: BookSide,
    pub original_quantity: BigDecimal,
    pub remaining_quantity: BigDecimal,
    pub price: BigDecimal,
    pub timestamp: DateTime<Utc>,
    pub order_type: OrderType,
}

/// Type of order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Limit,
    Market,
    Stop,
    StopLimit,
}

impl Default for OrderType {
    fn default() -> Self {
        OrderType::Limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_book_side_opposite() {
        assert_eq!(BookSide::Bid.opposite(), BookSide::Ask);
        assert_eq!(BookSide::Ask.opposite(), BookSide::Bid);
    }

    #[test]
    fn test_book_side_from_str() {
        assert_eq!(BookSide::from_str("BID"), Some(BookSide::Bid));
        assert_eq!(BookSide::from_str("ask"), Some(BookSide::Ask));
        assert_eq!(BookSide::from_str("BUY"), Some(BookSide::Bid));
        assert_eq!(BookSide::from_str("SELL"), Some(BookSide::Ask));
        assert_eq!(BookSide::from_str("invalid"), None);
    }
}
