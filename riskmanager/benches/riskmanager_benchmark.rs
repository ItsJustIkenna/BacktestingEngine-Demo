//! Benchmarks for the riskmanager module

use riskmanager::{RiskManager, RiskMetrics, RiskLimits};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// Mock risk metrics for benchmarking
fn create_risk_metrics() -> RiskMetrics {
    RiskMetrics {
        current_position: 5.0,
        current_order_size: 1.0,
        current_inventory_skew: 1.5,
        current_volatility: 0.08,
        ..Default::default()
    }
}

// Benchmark risk manager creation
fn bench_risk_manager_creation(c: &mut Criterion) {
    c.bench_function("risk_manager_creation", |b| {
        b.iter(|| {
            let rm = RiskManager::new();
            black_box(rm)
        })
    });
}

// Benchmark risk manager with custom limits
fn bench_risk_manager_with_limits(c: &mut Criterion) {
    let limits = RiskLimits {
        max_position: Some(20.0),
        max_order_size: Some(5.0),
        max_inventory_skew: Some(3.0),
        max_volatility: Some(0.15),
        ..Default::default()
    };
    
    c.bench_function("risk_manager_with_limits", |b| {
        b.iter(|| {
            let rm = RiskManager::with_limits(black_box(limits.clone()));
            black_box(rm)
        })
    });
}

// Benchmark risk limit checking simulation
fn bench_risk_limit_checking(c: &mut Criterion) {
    let rm = RiskManager::new();
    let metrics = create_risk_metrics();
    
    c.bench_function("risk_limit_checking", |b| {
        b.iter(|| {
            let limits = &black_box(&rm).limits;
            let metrics = black_box(&metrics);
            
            // Simulate risk checks
            let position_ok = if let Some(max_pos) = limits.max_position {
                metrics.current_position <= max_pos
            } else { true };
            
            let order_ok = if let Some(max_order) = limits.max_order_size {
                metrics.current_order_size <= max_order
            } else { true };
            
            let skew_ok = if let Some(max_skew) = limits.max_inventory_skew {
                metrics.current_inventory_skew <= max_skew
            } else { true };
            
            let vol_ok = if let Some(max_vol) = limits.max_volatility {
                metrics.current_volatility <= max_vol
            } else { true };
            
            let result = position_ok && order_ok && skew_ok && vol_ok;
            black_box(result)
        })
    });
}

// Benchmark serialization/deserialization
fn bench_risk_serialization(c: &mut Criterion) {
    let limits = RiskLimits::default();
    let metrics = create_risk_metrics();
    
    c.bench_function("risk_limits_serialization", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&limits)).unwrap();
            let _deserialized: RiskLimits = serde_json::from_str(&serialized).unwrap();
            black_box(serialized)
        })
    });
    
    c.bench_function("risk_metrics_serialization", |b| {
        b.iter(|| {
            let serialized = serde_json::to_string(black_box(&metrics)).unwrap();
            let _deserialized: RiskMetrics = serde_json::from_str(&serialized).unwrap();
            black_box(serialized)
        })
    });
}

criterion_group!(
    benches,
    bench_risk_manager_creation,
    bench_risk_manager_with_limits,
    bench_risk_limit_checking,
    bench_risk_serialization
);
criterion_main!(benches);
