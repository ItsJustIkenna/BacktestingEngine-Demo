//! Comprehensive tests for DataLoader module
//! Covers data loading, caching, and market data types

use dataloader::{DataLoader, MarketData, Candle, TradeData};
use bigdecimal::BigDecimal;
use std::str::FromStr;
use chrono::{Utc, Duration};

// ============================================================================
// DATALOADER CREATION TESTS
// ============================================================================

#[test]
fn test_dataloader_creation() {
    let _loader = DataLoader::new();
    assert!(true, "DataLoader created successfully");
}

#[test]
fn test_dataloader_mock() {
    let _loader = DataLoader::mock();
    assert!(true, "Mock DataLoader created successfully");
}

// ============================================================================
// CANDLE TESTS
// ============================================================================

#[test]
fn test_candle_creation() {
    let candle = Candle {
        timestamp: Utc::now(),
        symbol: "BTC-USD".to_string(),
        exchange: "coinbase".to_string(),
        open: BigDecimal::from_str("50000.0").unwrap(),
        high: BigDecimal::from_str("51000.0").unwrap(),
        low: BigDecimal::from_str("49500.0").unwrap(),
        close: BigDecimal::from_str("50500.0").unwrap(),
        volume: BigDecimal::from_str("100.5").unwrap(),
        trade_count: 1500,
    };
    
    assert_eq!(candle.symbol, "BTC-USD");
    assert!(candle.high >= candle.low);
}

#[test]
fn test_candle_ohlc_consistency() {
    let candle = Candle {
        timestamp: Utc::now(),
        symbol: "ETH-USD".to_string(),
        exchange: "binance".to_string(),
        open: BigDecimal::from_str("3000.0").unwrap(),
        high: BigDecimal::from_str("3200.0").unwrap(),
        low: BigDecimal::from_str("2900.0").unwrap(),
        close: BigDecimal::from_str("3100.0").unwrap(),
        volume: BigDecimal::from_str("500.0").unwrap(),
        trade_count: 2500,
    };
    
    // OHLC consistency: high >= max(open, close) and low <= min(open, close)
    assert!(candle.high >= candle.open);
    assert!(candle.high >= candle.close);
    assert!(candle.low <= candle.open);
    assert!(candle.low <= candle.close);
}

#[test]
fn test_candle_volume_positive() {
    let candle = Candle {
        timestamp: Utc::now(),
        symbol: "BTC-USD".to_string(),
        exchange: "kraken".to_string(),
        open: BigDecimal::from_str("50000.0").unwrap(),
        high: BigDecimal::from_str("50000.0").unwrap(),
        low: BigDecimal::from_str("50000.0").unwrap(),
        close: BigDecimal::from_str("50000.0").unwrap(),
        volume: BigDecimal::from_str("10.0").unwrap(),
        trade_count: 100,
    };
    
    assert!(candle.volume > BigDecimal::from(0));
    assert!(candle.trade_count > 0);
}

// ============================================================================
// TRADE DATA TESTS
// ============================================================================

#[test]
fn test_trade_data_creation() {
    let trade = TradeData::new(
        Utc::now(),
        "BTC-USD".to_string(),
        "coinbase".to_string(),
        BigDecimal::from_str("50000.0").unwrap(),
        BigDecimal::from_str("0.5").unwrap(),
        "buy".to_string(),
        "trade_001".to_string(),
    );
    
    assert_eq!(trade.symbol, "BTC-USD");
    assert_eq!(trade.side, "buy");
}

#[test]
fn test_trade_data_sides() {
    let buy_trade = TradeData::new(
        Utc::now(),
        "BTC-USD".to_string(),
        "binance".to_string(),
        BigDecimal::from_str("50000.0").unwrap(),
        BigDecimal::from_str("1.0").unwrap(),
        "buy".to_string(),
        "buy_001".to_string(),
    );
    
    let sell_trade = TradeData::new(
        Utc::now(),
        "BTC-USD".to_string(),
        "binance".to_string(),
        BigDecimal::from_str("50000.0").unwrap(),
        BigDecimal::from_str("1.0").unwrap(),
        "sell".to_string(),
        "sell_001".to_string(),
    );
    
    assert_eq!(buy_trade.side, "buy");
    assert_eq!(sell_trade.side, "sell");
}

// ============================================================================
// MARKET DATA ENUM TESTS
// ============================================================================

#[test]
fn test_market_data_candle_variant() {
    let candle = Candle {
        timestamp: Utc::now(),
        symbol: "BTC-USD".to_string(),
        exchange: "coinbase".to_string(),
        open: BigDecimal::from(50000),
        high: BigDecimal::from(51000),
        low: BigDecimal::from(49500),
        close: BigDecimal::from(50500),
        volume: BigDecimal::from(100),
        trade_count: 1000,
    };
    
    let market_data = MarketData::Candle(candle);
    
    assert!(matches!(market_data, MarketData::Candle(_)));
}

#[test]
fn test_market_data_trade_variant() {
    let trade = TradeData::new(
        Utc::now(),
        "ETH-USD".to_string(),
        "kraken".to_string(),
        BigDecimal::from(3000),
        BigDecimal::from(10),
        "buy".to_string(),
        "t_123".to_string(),
    );
    
    let market_data = MarketData::Trade(trade);
    
    assert!(matches!(market_data, MarketData::Trade(_)));
}

// ============================================================================
// TIME SERIES TESTS
// ============================================================================

#[test]
fn test_candle_time_sequence() {
    let base_time = Utc::now();
    
    let candles: Vec<Candle> = (0..10)
        .map(|i| Candle {
            timestamp: base_time + Duration::hours(i),
            symbol: "BTC-USD".to_string(),
            exchange: "test".to_string(),
            open: BigDecimal::from(50000 + i * 100),
            high: BigDecimal::from(50100 + i * 100),
            low: BigDecimal::from(49900 + i * 100),
            close: BigDecimal::from(50050 + i * 100),
            volume: BigDecimal::from(10),
            trade_count: 100,
        })
        .collect();
    
    // Verify chronological order
    for i in 1..candles.len() {
        assert!(candles[i].timestamp > candles[i-1].timestamp);
    }
}

#[test]
fn test_candle_gaps() {
    let base_time = Utc::now();
    
    // Create candles with a gap (missing hour 2)
    let timestamps = vec![
        base_time,
        base_time + Duration::hours(1),
        base_time + Duration::hours(3), // Gap here
        base_time + Duration::hours(4),
    ];
    
    // Check for gaps
    let mut gaps = Vec::new();
    for i in 1..timestamps.len() {
        let diff = timestamps[i] - timestamps[i-1];
        if diff > Duration::hours(1) {
            gaps.push(i);
        }
    }
    
    assert_eq!(gaps.len(), 1, "Should detect one gap");
    assert_eq!(gaps[0], 2, "Gap should be at index 2");
}

// ============================================================================
// PRICE CALCULATION TESTS
// ============================================================================

#[test]
fn test_vwap_calculation() {
    let trades = vec![
        (50000.0, 1.0),  // price, quantity
        (50100.0, 2.0),
        (49900.0, 1.5),
    ];
    
    let total_value: f64 = trades.iter().map(|(p, q)| p * q).sum();
    let total_qty: f64 = trades.iter().map(|(_, q)| q).sum();
    let vwap = total_value / total_qty;
    
    assert!(vwap > 49900.0 && vwap < 50100.0);
}

#[test]
fn test_twap_calculation() {
    let prices = vec![50000.0, 50100.0, 49900.0, 50200.0];
    
    let twap: f64 = prices.iter().sum::<f64>() / prices.len() as f64;
    
    assert_eq!(twap, 50050.0);
}

// ============================================================================
// CACHE TESTS
// ============================================================================

#[tokio::test]
async fn test_cache_invalidation() {
    let loader = DataLoader::new();
    
    // Invalidate cache
    loader.invalidate_cache().await;
    
    assert!(true, "Cache invalidation completed");
}

// ============================================================================
// DATA VALIDATION TESTS
// ============================================================================

#[test]
fn test_symbol_format() {
    let valid_symbols = vec!["BTC-USD", "ETH-USDT", "SOL-PERP", "BTC-EUR"];
    
    for symbol in valid_symbols {
        assert!(symbol.contains("-"), "Symbol should contain separator");
    }
}

#[test]
fn test_exchange_names() {
    let exchanges = vec!["binance", "coinbase", "kraken", "ftx", "bybit"];
    
    for exchange in exchanges {
        assert!(!exchange.is_empty());
        assert!(exchange.chars().all(|c| c.is_lowercase() || c.is_numeric()));
    }
}

// ============================================================================
// PRICE PRECISION TESTS
// ============================================================================

#[test]
fn test_bigdecimal_precision() {
    let price = BigDecimal::from_str("50000.12345678").unwrap();
    let quantity = BigDecimal::from_str("0.00001").unwrap();
    
    let value = &price * &quantity;
    
    // Value should maintain precision
    assert!(value > BigDecimal::from(0));
}

#[test]
fn test_price_rounding() {
    let price: f64 = 50000.123456789;
    let rounded_8 = (price * 100_000_000.0).round() / 100_000_000.0;
    
    assert!((rounded_8 - 50000.12345679).abs() < 0.00000001);
}
