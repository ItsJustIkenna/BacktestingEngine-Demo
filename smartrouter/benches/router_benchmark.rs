//! Benchmarks for Smart Order Router performance

use criterion::{criterion_group, criterion_main, Criterion, black_box};
use smartrouter::{
    SmartRouter, RoutingMode, ExecutionStyle,
    order::{ParentOrder, OrderSide},
    venue::{VenueSnapshot, VenueConfig, Level},
    arbitrage::ArbitrageRouter,
};

fn create_test_venues(num_levels: usize) -> Vec<(String, VenueSnapshot)> {
    let exchanges = vec!["kraken", "coinbase", "binance"];
    let mut venues = Vec::new();
    
    for (i, name) in exchanges.iter().enumerate() {
        let base_price = 42000.0 + (i as f64 * 5.0);
        
        let bids: Vec<Level> = (0..num_levels)
            .map(|j| Level {
                price: base_price - (j as f64 * 1.0),
                quantity: 1.0 + (j as f64 * 0.5),
            })
            .collect();
        
        let asks: Vec<Level> = (0..num_levels)
            .map(|j| Level {
                price: base_price + 10.0 + (j as f64 * 1.0),
                quantity: 1.0 + (j as f64 * 0.5),
            })
            .collect();
        
        let config = match *name {
            "kraken" => VenueConfig::kraken(),
            "coinbase" => VenueConfig::coinbase(),
            "binance" => VenueConfig::binance(),
            _ => VenueConfig::default(),
        };
        
        let snapshot = VenueSnapshot::new(config, "BTC-USD", 0, bids, asks);
        venues.push((name.to_string(), snapshot));
    }
    
    venues
}

fn bench_best_venue_routing(c: &mut Criterion) {
    let venues = create_test_venues(10);
    let mut router = SmartRouter::new(RoutingMode::BestVenue);
    
    for (name, snapshot) in &venues {
        router.update_venue(name, snapshot.clone());
    }
    
    c.bench_function("best_venue_routing", |b| {
        b.iter(|| {
            let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 1.0, 42000.0);
            black_box(router.route_order(&order, Some(ExecutionStyle::Balanced)))
        })
    });
}

fn bench_multi_venue_routing(c: &mut Criterion) {
    let venues = create_test_venues(10);
    let mut router = SmartRouter::new(RoutingMode::MultiVenue {
        max_venues: 3,
        min_venue_allocation: 0.1,
    });
    
    for (name, snapshot) in &venues {
        router.update_venue(name, snapshot.clone());
    }
    
    c.bench_function("multi_venue_routing", |b| {
        b.iter(|| {
            let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 5.0, 42000.0);
            black_box(router.route_order(&order, Some(ExecutionStyle::Balanced)))
        })
    });
}

fn bench_arbitrage_scan(c: &mut Criterion) {
    // Create venues with arbitrage opportunity
    let mut router = ArbitrageRouter::new(1.0, 5000);
    
    let kraken = VenueSnapshot::new(
        VenueConfig::kraken(),
        "BTC-USD",
        0,
        vec![Level { price: 42000.0, quantity: 2.0 }],
        vec![Level { price: 42010.0, quantity: 2.0 }],
    );
    
    let coinbase = VenueSnapshot::new(
        VenueConfig::coinbase(),
        "BTC-USD",
        0,
        vec![Level { price: 42050.0, quantity: 1.5 }],
        vec![Level { price: 42060.0, quantity: 1.5 }],
    );
    
    let binance = VenueSnapshot::new(
        VenueConfig::binance(),
        "BTC-USD",
        0,
        vec![Level { price: 42005.0, quantity: 3.0 }],
        vec![Level { price: 42015.0, quantity: 3.0 }],
    );
    
    router.update_venue("kraken", kraken);
    router.update_venue("coinbase", coinbase);
    router.update_venue("binance", binance);
    
    c.bench_function("arbitrage_scan", |b| {
        b.iter(|| {
            black_box(router.scan_opportunities("BTC-USD"))
        })
    });
}

fn bench_venue_scoring(c: &mut Criterion) {
    let venues = create_test_venues(20);
    
    c.bench_function("venue_scoring", |b| {
        b.iter(|| {
            for (_, snapshot) in &venues {
                black_box(snapshot.score_for_order(OrderSide::Buy, 2.0));
            }
        })
    });
}

criterion_group!(
    benches,
    bench_best_venue_routing,
    bench_multi_venue_routing,
    bench_arbitrage_scan,
    bench_venue_scoring,
);
criterion_main!(benches);
