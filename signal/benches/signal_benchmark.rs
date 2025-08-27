//! Benchmarks for the signal module

use signal::{Signal, SignalType, SignalStrength, SignalReason};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use chrono::Utc;
use bigdecimal::BigDecimal;

// Helper function to create test signals
fn create_test_signal(id: &str, signal_type: SignalType) -> Signal {
    Signal {
        id: id.to_string(),
        timestamp: Utc::now(),
        signal_type,
        strength: SignalStrength::Medium,
        price: Some(BigDecimal::from(100)),
        quantity: Some(BigDecimal::from(10)),
        reason: SignalReason::TechnicalAnalysis("RSI".to_string()),
        metadata: HashMap::new(),
    }
}

// Benchmark signal creation
fn bench_signal_creation(c: &mut Criterion) {
    c.bench_function("signal_creation", |b| {
        b.iter(|| {
            let signal = create_test_signal(
                black_box("test_signal"),
                black_box(SignalType::Buy),
            );
            black_box(signal)
        })
    });
}

// Benchmark signal cloning
fn bench_signal_cloning(c: &mut Criterion) {
    let signal = create_test_signal("test", SignalType::Buy);
    
    c.bench_function("signal_cloning", |b| {
        b.iter(|| {
            let cloned = black_box(&signal).clone();
            black_box(cloned)
        })
    });
}

// Benchmark signal serialization
fn bench_signal_serialization(c: &mut Criterion) {
    let signal = create_test_signal("test", SignalType::Buy);
    
    c.bench_function("signal_serialization", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&signal)).unwrap();
            black_box(serialized)
        })
    });
}

// Benchmark signal processing with different types
fn bench_signal_type_matching(c: &mut Criterion) {
    let signals = vec![
        create_test_signal("buy", SignalType::Buy),
        create_test_signal("sell", SignalType::Sell),
        create_test_signal("hold", SignalType::Hold),
        create_test_signal("close", SignalType::Close),
    ];
    
    c.bench_function("signal_type_matching", |b| {
        b.iter(|| {
            for signal in black_box(&signals) {
                let result = match &signal.signal_type {
                    SignalType::Buy => 1,
                    SignalType::Sell => 2,
                    SignalType::Hold => 3,
                    SignalType::Close => 4,
                    _ => 0,
                };
                black_box(result);
            }
        })
    });
}

criterion_group!(
    benches,
    bench_signal_creation,
    bench_signal_cloning,
    bench_signal_serialization,
    bench_signal_type_matching
);
criterion_main!(benches);
