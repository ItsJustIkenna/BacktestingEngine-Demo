//! Benchmarks for the config module

use config::BacktestConfig;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

fn bench_config_creation(c: &mut Criterion) {
    c.bench_function("config_default_creation", |b| {
        b.iter(|| {
            black_box(BacktestConfig::default())
        })
    });
}

fn bench_config_validation(c: &mut Criterion) {
    let config = BacktestConfig::default();
    
    c.bench_function("config_validation", |b| {
        b.iter(|| {
            black_box(config.validate()).unwrap()
        })
    });
}

fn bench_config_serialization(c: &mut Criterion) {
    let config = BacktestConfig::default();
    
    c.bench_function("config_yaml_serialization", |b| {
        b.iter(|| {
            black_box(serde_yaml::to_string(&config)).unwrap()
        })
    });
    
    let yaml_str = serde_yaml::to_string(&config).unwrap();
    c.bench_function("config_yaml_deserialization", |b| {
        b.iter(|| {
            black_box(serde_yaml::from_str::<BacktestConfig>(&yaml_str)).unwrap()
        })
    });
}

fn bench_config_file_operations(c: &mut Criterion) {
    let config = BacktestConfig::default();
    
    c.bench_function("config_file_save_load", |b| {
        b.iter(|| {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join("bench_config.yaml");
            
            // Save
            black_box(config.to_file(&config_path)).unwrap();
            
            // Load
            black_box(BacktestConfig::from_file(&config_path)).unwrap()
        })
    });
}

fn bench_config_cloning(c: &mut Criterion) {
    let config = BacktestConfig::default();
    
    c.bench_function("config_clone", |b| {
        b.iter(|| {
            black_box(config.clone())
        })
    });
}

fn bench_config_parameter_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_parameter_access");
    
    let config = BacktestConfig::default();
    
    group.bench_function("database_config_access", |b| {
        b.iter(|| {
            black_box(&config.database.host);
            black_box(config.database.port);
            black_box(&config.database.database);
        })
    });
    
    group.bench_function("trading_config_access", |b| {
        b.iter(|| {
            black_box(config.trading.initial_capital);
            black_box(config.trading.commission_rate);
        })
    });
    
    group.finish();
}

criterion_group!(
    config_benches,
    bench_config_creation,
    bench_config_validation,
    bench_config_serialization,
    bench_config_file_operations,
    bench_config_cloning,
    bench_config_parameter_access
);

criterion_main!(config_benches);
