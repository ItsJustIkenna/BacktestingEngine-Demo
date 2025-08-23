//! Utilities for order book and signal conversion

use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use signal::{Signal, SignalType};
use crate::types::{OrderBookEvent, BookSide};

/// Convert a Signal to an OrderBookEvent::NewOrder
pub fn signal_to_order_event(signal: &Signal, timestamp: DateTime<Utc>) -> OrderBookEvent {
    OrderBookEvent::NewOrder {
        order_id: signal.id.clone(),
        side: match signal.signal_type {
            SignalType::Buy => BookSide::Bid,
            SignalType::Sell => BookSide::Ask,
            _ => BookSide::Bid, // Default for other signal types
        },
        price: signal.price.clone().unwrap_or_else(|| BigDecimal::from(0)),
        quantity: signal.quantity.clone().unwrap_or_else(|| BigDecimal::from(0)),
        timestamp,
    }
}
