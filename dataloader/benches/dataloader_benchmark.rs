use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dataloader::{DataLoader, MarketData, Candle, TradeData};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};

// Helper function to create test candles
fn create_test_candle(symbol: &str, price: f64) -> MarketData {
    MarketData::Candle(Candle {
        symbol: symbol.to_string(),
        exchange: "TEST".to_string(),
        timestamp: Utc::now(),
        open: BigDecimal::from_f64(price).unwrap_or_default(),
        high: BigDecimal::from_f64(price * 1.05).unwrap_or_default(),
        low: BigDecimal::from_f64(price * 0.95).unwrap_or_default(),
        close: BigDecimal::from_f64(price * 1.02).unwrap_or_default(),
        volume: BigDecimal::from_f64(1000.0).unwrap_or_default(),
        trade_count: 50,
    })
}

// Helper function to create test trades
fn create_test_trade(symbol: &str, price: f64) -> MarketData {
    MarketData::Trade(TradeData {
        symbol: symbol.to_string(),
        exchange: "TEST".to_string(),
        timestamp: Utc::now(),
        trade_id: "test_trade_123".to_string(),
        price: BigDecimal::from_f64(price).unwrap_or_default(),
        quantity: BigDecimal::from_f64(100.0).unwrap_or_default(),
        side: "buy".to_string(),
    })
}

// Benchmark DataLoader creation
fn bench_dataloader_creation(c: &mut Criterion) {
    c.bench_function("dataloader_creation", |b| {
        b.iter(|| {
            let loader = DataLoader::new();
            black_box(loader)
        })
    });
}

// Benchmark market data creation
fn bench_market_data_creation(c: &mut Criterion) {
    c.bench_function("candle_creation", |b| {
        b.iter(|| {
            let candle = create_test_candle("BTC/USDT", 50000.0);
            black_box(candle)
        })
    });
    
    c.bench_function("trade_creation", |b| {
        b.iter(|| {
            let trade = create_test_trade("BTC/USDT", 50000.0);
            black_box(trade)
        })
    });
}

// Benchmark market data processing
fn bench_market_data_processing(c: &mut Criterion) {
    let candles: Vec<MarketData> = (0..1000)
        .map(|i| create_test_candle("BTC/USDT", 50000.0 + i as f64))
        .collect();
    
    c.bench_function("process_candles", |b| {
        b.iter(|| {
            let mut sum = BigDecimal::from(0);
            for candle in &candles {
                match candle {
                    MarketData::Candle(candle) => {
                        sum += &candle.close;
                    }
                    MarketData::Trade(trade) => {
                        sum += &trade.price;
                    }
                }
            }
            black_box(sum)
        })
    });
}

criterion_group!(benches, bench_dataloader_creation, bench_market_data_creation, bench_market_data_processing);
criterion_main!(benches);
