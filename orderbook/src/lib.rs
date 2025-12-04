pub mod order_utils;
/// Order Book Reconstruction Module
/// 
/// This module provides functionality to reconstruct and maintain order books
/// from historical market data, enabling realistic backtesting and market analysis.

pub mod order_book;
pub mod level;
pub mod reconstructor;
pub mod types;
pub mod logging_facade;

// Re-export main types for convenience
pub use order_book::OrderBook;
pub use level::Level;
pub use reconstructor::OrderBookReconstructor;
pub use types::{
    BookSide, Fill, LiquidityType, OrderBookEvent, OrderInfo,
    SnapshotLevel, VolumeEntry, SlippageConfig, OrderBookSnapshot,
    ExecutionLevel, MarketOrderExecution,
};
pub use config::OrderBookConfig;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Market depth information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDepth {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
    pub best_bid: Option<DepthLevel>,
    pub best_ask: Option<DepthLevel>,
    pub spread: Option<f64>,
}

/// Individual depth level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthLevel {
    pub price: f64,
    pub quantity: f64,
    pub order_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = OrderBookConfig::default();
        assert_eq!(config.max_levels, Some(100));
        assert!(config.track_individual_orders);
        assert!(config.strict_validation);
    }
}
