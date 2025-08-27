use criterion::{criterion_group, criterion_main, Criterion};
use portfoliomanager::{PortfolioState, Position, PositionSide};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};
use orderbook::{Fill, BookSide};

fn create_test_portfolio() -> PortfolioState {
    let mut portfolio = PortfolioState::default();
    portfolio.balance = BigDecimal::from_f64(10000.0).unwrap_or_default();
    portfolio.positions.push(Position {
        symbol: "BTC/USDT".to_string(),
        side: PositionSide::Long,
        quantity: 0.5,
        entry_price: BigDecimal::from_f64(50000.0).unwrap_or_default(),
        mark_price: Some(BigDecimal::from_f64(52000.0).unwrap_or_default()),
        realized_pnl: BigDecimal::from(0),
        unrealized_pnl: BigDecimal::from_f64(1000.0).unwrap_or_default(),
        open_time: Utc::now(),
        close_time: None,
    });
    portfolio
}

fn create_test_fill() -> Fill {
    Fill {
        order_id: "test_order_123".to_string(),
        timestamp: Utc::now(),
        side: BookSide::Bid,
        price: BigDecimal::from_f64(51000.0).unwrap_or_default(),
        base_price: Some(BigDecimal::from_f64(50900.0).unwrap_or_default()),
        quantity: BigDecimal::from_f64(0.1).unwrap_or_default(),
        liquidity_type: orderbook::LiquidityType::Taker,
        fee_rate: 0.001,
        fee_amount: BigDecimal::from_f64(5.1).unwrap_or_default(),
        slippage_bps: Some(20.0),
    }
}

fn bench_portfolio_creation(c: &mut Criterion) {
    c.bench_function("create_test_portfolio", |b| {
        b.iter(|| {
            create_test_portfolio()
        })
    });
}

fn bench_portfolio_fill_application(c: &mut Criterion) {
    c.bench_function("apply_portfolio_fill", |b| {
        b.iter(|| {
            let mut portfolio = create_test_portfolio();
            let fill = create_test_fill();
            let _ = portfolio.apply_fill(&fill);
            portfolio
        })
    });
}

fn bench_position_creation(c: &mut Criterion) {
    c.bench_function("create_position", |b| {
        b.iter(|| {
            Position {
                symbol: "ETH/USDT".to_string(),
                side: PositionSide::Short,
                quantity: 2.0,
                entry_price: BigDecimal::from_f64(3000.0).unwrap_or_default(),
                mark_price: Some(BigDecimal::from_f64(2950.0).unwrap_or_default()),
                realized_pnl: BigDecimal::from(0),
                unrealized_pnl: BigDecimal::from_f64(100.0).unwrap_or_default(),
                open_time: Utc::now(),
                close_time: None,
            }
        })
    });
}

criterion_group!(benches, bench_portfolio_creation, bench_portfolio_fill_application, bench_position_creation);
criterion_main!(benches);
