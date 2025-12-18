//! Lightweight tick data for memory-efficient backtesting simulation
//!
//! SimulationTick is a compact representation (~48 bytes) of market data
//! compared to ~400 bytes for HistoricalOrder. This 10x reduction enables:
//! - More concurrent backtests per worker
//! - Faster memory-mapped file access
//! - Better CPU cache utilization

use serde::{Deserialize, Serialize};
use bytemuck::{Pod, Zeroable};

/// Lightweight tick data for simulation
/// 
/// Layout (48 bytes, cache-line friendly):
/// - timestamp_ms: i64 (8 bytes) - Unix milliseconds
/// - price: f64 (8 bytes) - Price level
/// - quantity: f64 (8 bytes) - Trade/order quantity  
/// - side: u8 (1 byte) - 0=Buy, 1=Sell
/// - _padding: [u8; 7] - Alignment padding
/// - volatility_1m: f32 (4 bytes) - Precomputed 1-min volatility
/// - ema_5m: f32 (4 bytes) - Precomputed 5-min EMA
/// - imbalance: f32 (4 bytes) - Order flow imbalance
/// - _reserved: f32 (4 bytes) - Future use
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Pod, Zeroable)]
#[repr(C)]
pub struct SimulationTick {
    /// Timestamp as Unix milliseconds
    pub timestamp_ms: i64,
    /// Price as f64
    pub price: f64,
    /// Quantity as f64
    pub quantity: f64,
    /// Side: 0 = Buy, 1 = Sell
    pub side: u8,
    /// Padding for alignment
    pub _padding: [u8; 7],
    /// Precomputed 1-minute volatility (filled by FeatureComputer)
    pub volatility_1m: f32,
    /// Precomputed 5-minute EMA (filled by FeatureComputer)
    pub ema_5m: f32,
    /// Order flow imbalance [-1, 1] (filled by FeatureComputer)
    pub imbalance: f32,
    /// Reserved for future features
    pub _reserved: f32,
}

// Verify size at compile time
const _: () = assert!(std::mem::size_of::<SimulationTick>() == 48);

impl SimulationTick {
    /// Create a new tick with basic data (features zeroed)
    pub fn new(timestamp_ms: i64, price: f64, quantity: f64, is_sell: bool) -> Self {
        Self {
            timestamp_ms,
            price,
            quantity,
            side: if is_sell { 1 } else { 0 },
            _padding: [0; 7],
            volatility_1m: 0.0,
            ema_5m: 0.0,
            imbalance: 0.0,
            _reserved: 0.0,
        }
    }

    /// Check if this is a sell order
    #[inline]
    pub fn is_sell(&self) -> bool {
        self.side == 1
    }

    /// Check if this is a buy order
    #[inline]
    pub fn is_buy(&self) -> bool {
        self.side == 0
    }

    /// Get timestamp as DateTime<Utc>
    pub fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.timestamp_ms)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH)
    }
    
    /// Get side as string for compatibility
    pub fn side_str(&self) -> &'static str {
        if self.is_sell() { "sell" } else { "buy" }
    }
}

// Implement the Tick trait from dataloader for unified simulation interface
impl dataloader::Tick for SimulationTick {
    #[inline]
    fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.timestamp_ms)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH)
    }
    
    #[inline]
    fn timestamp_ms(&self) -> i64 {
        self.timestamp_ms
    }
    
    #[inline]
    fn price(&self) -> f64 {
        self.price
    }
    
    #[inline]
    fn quantity(&self) -> f64 {
        self.quantity
    }
    
    #[inline]
    fn side(&self) -> dataloader::Side {
        if self.is_sell() {
            dataloader::Side::Sell
        } else {
            dataloader::Side::Buy
        }
    }
}

// Conversion from HistoricalOrder
#[cfg(feature = "postgres")]
impl From<&databaseschema::models::historical_order::HistoricalOrder> for SimulationTick {
    fn from(order: &databaseschema::models::historical_order::HistoricalOrder) -> Self {
        use bigdecimal::ToPrimitive;
        Self {
            timestamp_ms: order.timestamp.timestamp_millis(),
            price: order.price_level.to_f64().unwrap_or(0.0),
            quantity: order.quantity.to_f64().unwrap_or(0.0),
            side: if order.side.eq_ignore_ascii_case("sell") 
                   || order.side.eq_ignore_ascii_case("ask") { 1 } else { 0 },
            _padding: [0; 7],
            volatility_1m: 0.0,
            ema_5m: 0.0,
            imbalance: 0.0,
            _reserved: 0.0,
        }
    }
}

// Conversion from TradeData
impl From<&dataloader::TradeData> for SimulationTick {
    fn from(trade: &dataloader::TradeData) -> Self {
        Self {
            timestamp_ms: trade.timestamp.timestamp_millis(),
            price: trade.price_f64,
            quantity: trade.quantity_f64,
            side: if trade.side.eq_ignore_ascii_case("sell") 
                   || trade.side.eq_ignore_ascii_case("ask") { 1 } else { 0 },
            _padding: [0; 7],
            volatility_1m: 0.0,
            ema_5m: 0.0,
            imbalance: 0.0,
            _reserved: 0.0,
        }
    }
}

// Conversion from Candle
impl From<&dataloader::Candle> for SimulationTick {
    fn from(candle: &dataloader::Candle) -> Self {
        use bigdecimal::ToPrimitive;
        Self {
            timestamp_ms: candle.timestamp.timestamp_millis(),
            price: candle.close.to_f64().unwrap_or(0.0),
            quantity: candle.volume.to_f64().unwrap_or(0.0),
            side: 0, // Candles don't have side
            _padding: [0; 7],
            volatility_1m: 0.0,
            ema_5m: 0.0,
            imbalance: 0.0,
            _reserved: 0.0,
        }
    }
}

// Conversion from MarketData enum
impl From<&dataloader::MarketData> for SimulationTick {
    fn from(data: &dataloader::MarketData) -> Self {
        match data {
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Order(o) => SimulationTick::from(o),
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Snapshot(s) => {
                use bigdecimal::ToPrimitive;
                SimulationTick {
                    timestamp_ms: s.timestamp.timestamp_millis(),
                    price: s.price_level.to_f64().unwrap_or(0.0),
                    quantity: s.quantity.to_f64().unwrap_or(0.0),
                    side: if s.side.eq_ignore_ascii_case("sell") 
                           || s.side.eq_ignore_ascii_case("ask") { 1 } else { 0 },
                    _padding: [0; 7],
                    volatility_1m: 0.0,
                    ema_5m: 0.0,
                    imbalance: 0.0,
                    _reserved: 0.0,
                }
            }
            dataloader::MarketData::Candle(c) => SimulationTick::from(c),
            dataloader::MarketData::Trade(t) => SimulationTick::from(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_size() {
        assert_eq!(std::mem::size_of::<SimulationTick>(), 48);
    }

    #[test]
    fn test_tick_creation() {
        let tick = SimulationTick::new(1702658722123, 50234.57, 0.1235, false);
        assert_eq!(tick.timestamp_ms, 1702658722123);
        assert!((tick.price - 50234.57).abs() < 0.001);
        assert!(tick.is_buy());
        assert!(!tick.is_sell());
    }
}
