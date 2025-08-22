//! Utilities for order book and signal conversion

use chrono::{DateTime, Utc};
use signal::{Signal, SignalType};
use crate::types::{OrderBookEvent, BookSide};

/// Convert a Signal to an OrderBookEvent::NewOrder
pub fn signal_to_order_event(signal: &Signal, timestamp: DateTime<Utc>) -> OrderBookEvent {
    OrderBookEvent::NewOrder {
        order_id: signal.id().to_string(),
        side: match signal.signal_type() {
            SignalType::Buy => BookSide::Bid,
            SignalType::Sell => BookSide::Ask,
        },
        price: signal.price().clone(),
        quantity: signal.quantity().clone(),
        timestamp,
    }
}
