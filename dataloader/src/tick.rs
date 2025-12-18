//! Tick trait for unified market data abstraction
//!
//! This trait allows the simulation loop to work with different data sources:
//! - `MarketData` (in-memory from database)
//! - `SimulationTick` (memory-mapped binary files)
//!
//! This abstraction enables zero-copy backtesting on large datasets.

use chrono::{DateTime, Utc};

/// Trade side
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    #[inline]
    pub fn is_buy(&self) -> bool {
        matches!(self, Side::Buy)
    }
    
    #[inline]
    pub fn is_sell(&self) -> bool {
        matches!(self, Side::Sell)
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
}

impl From<&str> for Side {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sell" | "s" | "ask" | "a" => Side::Sell,
            _ => Side::Buy,
        }
    }
}

/// Core tick data trait for simulation
/// 
/// This trait extracts the essential fields needed by `SimulationLoop`:
/// - timestamp: When the tick occurred
/// - price: The price level
/// - quantity: Trade/order size
/// - side: Buy or sell
///
/// All implementations must provide fast access to these fields.
pub trait Tick: Send + Sync {
    /// Get the timestamp
    fn timestamp(&self) -> DateTime<Utc>;
    
    /// Get the timestamp as milliseconds since Unix epoch
    fn timestamp_ms(&self) -> i64 {
        self.timestamp().timestamp_millis()
    }
    
    /// Get the price as f64
    fn price(&self) -> f64;
    
    /// Get the quantity as f64
    fn quantity(&self) -> f64;
    
    /// Get the trade side
    fn side(&self) -> Side;
    
    /// Check if this is a sell
    #[inline]
    fn is_sell(&self) -> bool {
        self.side().is_sell()
    }
    
    /// Check if this is a buy
    #[inline]
    fn is_buy(&self) -> bool {
        self.side().is_buy()
    }
    
    /// Get the notional value (price * quantity)
    #[inline]
    fn value(&self) -> f64 {
        self.price() * self.quantity()
    }
}

// Implement Tick for TradeData
impl Tick for super::TradeData {
    #[inline]
    fn timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
    
    #[inline]
    fn price(&self) -> f64 {
        self.price_f64
    }
    
    #[inline]
    fn quantity(&self) -> f64 {
        self.quantity_f64
    }
    
    #[inline]
    fn side(&self) -> Side {
        Side::from(self.side.as_str())
    }
}

// Implement Tick for MarketData (delegates to inner type)
impl Tick for super::MarketData {
    fn timestamp(&self) -> DateTime<Utc> {
        match self {
            super::MarketData::Trade(t) => t.timestamp,
            super::MarketData::Candle(c) => c.timestamp,
            #[cfg(feature = "postgres")]
            super::MarketData::Snapshot(s) => s.timestamp,
            #[cfg(feature = "postgres")]
            super::MarketData::Order(o) => o.timestamp,
        }
    }
    
    fn price(&self) -> f64 {
        use bigdecimal::ToPrimitive;
        match self {
            super::MarketData::Trade(t) => t.price_f64,
            super::MarketData::Candle(c) => c.close.to_f64().unwrap_or(0.0),
            #[cfg(feature = "postgres")]
            super::MarketData::Snapshot(s) => s.price_level.to_f64().unwrap_or(0.0),
            #[cfg(feature = "postgres")]
            super::MarketData::Order(o) => o.price_level.to_f64().unwrap_or(0.0),
        }
    }
    
    fn quantity(&self) -> f64 {
        use bigdecimal::ToPrimitive;
        match self {
            super::MarketData::Trade(t) => t.quantity_f64,
            super::MarketData::Candle(c) => c.volume.to_f64().unwrap_or(0.0),
            #[cfg(feature = "postgres")]
            super::MarketData::Snapshot(s) => s.quantity.to_f64().unwrap_or(0.0),
            #[cfg(feature = "postgres")]
            super::MarketData::Order(o) => o.quantity.to_f64().unwrap_or(0.0),
        }
    }
    
    fn side(&self) -> Side {
        match self {
            super::MarketData::Trade(t) => Side::from(t.side.as_str()),
            super::MarketData::Candle(_) => Side::Buy, // Candles don't have side
            #[cfg(feature = "postgres")]
            super::MarketData::Snapshot(_) => Side::Buy, // Snapshots don't have side
            #[cfg(feature = "postgres")]
            super::MarketData::Order(o) => Side::from(o.side.as_str()),
        }
    }
}

/// A tick source that can provide ticks for simulation
/// 
/// This trait abstracts over different data sources:
/// - Memory-mapped binary files (zero-copy, constant memory)
/// - In-memory vectors (legacy database loads)
pub trait TickSource: Send + Sync {
    /// The tick type this source provides
    type TickType: Tick;
    
    /// Get the number of ticks
    fn len(&self) -> usize;
    
    /// Check if empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Get a tick by index
    fn get(&self, index: usize) -> Option<&Self::TickType>;
    
    /// Iterate over all ticks
    fn iter(&self) -> impl Iterator<Item = &Self::TickType>;
}

/// Wrapper for a slice of MarketData
pub struct MarketDataSlice<'a> {
    data: &'a [super::MarketData],
}

impl<'a> MarketDataSlice<'a> {
    pub fn new(data: &'a [super::MarketData]) -> Self {
        Self { data }
    }
}

impl<'a> TickSource for MarketDataSlice<'a> {
    type TickType = super::MarketData;
    
    fn len(&self) -> usize {
        self.data.len()
    }
    
    fn get(&self, index: usize) -> Option<&Self::TickType> {
        self.data.get(index)
    }
    
    fn iter(&self) -> impl Iterator<Item = &Self::TickType> {
        self.data.iter()
    }
}
