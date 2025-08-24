//! Benchmarks for the dataloader module

use dataloader::{DataLoader, MarketData, QueryBuilder};
use config::DataLoaderConfig;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use tokio::runtime::Runtime;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashMap;

// Mock database connection for benchmarking
struct MockDbConnection;

impl MockDbConnection {
    fn new() -> Self {
        Self
    }
    
    async fn query_market_data(&self, query: &str, params: &[&str]) -> Vec<MarketData> {
        // Simulate database query latency
        tokio::time::sleep(Duration::from_micros(100)).await;
        
        // Generate mock data based on query complexity
        let data_points = if query.contains("LIMIT") { 100 } else { 1000 };
        
        (0..data_points).map(|i| {
            MarketData::OHLCV {
                timestamp: UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
                open: 100.0 + (i as f64 * 0.1) % 10.0,
                high: 105.0 + (i as f64 * 0.1) % 10.0,
                low: 95.0 + (i as f64 * 0.1) % 10.0,
                close: 102.0 + (i as f64 * 0.1) % 10.0,
                volume: 1000.0 + (i as f64 * 10.0) % 5000.0,
            }
        }).collect()
    }
    
    async fn execute_query(&self, query: &str) -> Result<Vec<HashMap<String, String>>, String> {
        // Simulate different query execution times
        let sleep_time = if query.len() > 200 { 
            Duration::from_millis(10)
        } else {
            Duration::from_micros(500)
        };
        
        tokio::time::sleep(sleep_time).await;
        
        // Generate mock result set
        let row_count = if query.contains("LIMIT") { 
            query.split("LIMIT").nth(1)
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(100)
        } else { 
            1000 
        };
        
        let results: Vec<HashMap<String, String>> = (0..row_count).map(|i| {
            let mut row = HashMap::new();
            row.insert("id".to_string(), i.to_string());
            row.insert("symbol".to_string(), "AAPL".to_string());
            row.insert("price".to_string(), format!("{:.2}", 100.0 + i as f64 * 0.1));
            row.insert("timestamp".to_string(), (1640000000 + i * 60).to_string());
            row
        }).collect();
        
        Ok(results)
    }
}

fn bench_dataloader_creation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    c.bench_function("dataloader_creation", |b| {
        b.iter(|| {
            rt.block_on(async {
                let config = DataLoaderConfig {
                    database_url: "mock://localhost".to_string(),
                    cache_size: 1000,
                    query_timeout: Duration::from_secs(30),
                    batch_size: 100,
                };
                black_box(DataLoader::new(config).await)
            })
        })
    });
}

fn bench_market_data_queries(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("market_data_queries");
    
    let db = MockDbConnection::new();
    
    // Benchmark different query types
    group.bench_function("simple_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let query = "SELECT * FROM market_data WHERE symbol = ? LIMIT 100";
                let params = vec!["AAPL"];
                black_box(db.query_market_data(query, &params).await)
            })
        })
    });
    
    group.bench_function("complex_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let query = r#"
                    SELECT symbol, timestamp, close, volume 
                    FROM market_data 
                    WHERE symbol IN (?, ?, ?) 
                    AND timestamp BETWEEN ? AND ?
                    ORDER BY timestamp DESC
                    LIMIT 500
                "#;
                let params = vec!["AAPL", "GOOGL", "MSFT", "1640000000", "1640086400"];
                black_box(db.query_market_data(query, &params).await)
            })
        })
    });
    
    group.bench_function("aggregation_query", |b| {
        b.iter(|| {
            rt.block_on(async {
                let query = r#"
                    SELECT symbol, DATE(timestamp) as date,
                           AVG(close) as avg_close,
                           SUM(volume) as total_volume,
                           MIN(low) as min_low,
                           MAX(high) as max_high
                    FROM market_data 
                    WHERE symbol = ?
                    GROUP BY symbol, DATE(timestamp)
                    ORDER BY date DESC
                    LIMIT 50
                "#;
                let params = vec!["AAPL"];
                black_box(db.query_market_data(query, &params).await)
            })
        })
    });
    
    group.finish();
}

fn bench_query_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_builder");
    
    group.bench_function("simple_select", |b| {
        b.iter(|| {
            let mut builder = QueryBuilder::new();
            builder
                .select(&["symbol", "close", "volume"])
                .from("market_data")
                .where_clause("symbol = ?")
                .limit(100);
            black_box(builder.build())
        })
    });
    
    group.bench_function("complex_select", |b| {
        b.iter(|| {
            let mut builder = QueryBuilder::new();
            builder
                .select(&["symbol", "timestamp", "open", "high", "low", "close", "volume"])
                .from("market_data m")
                .join("LEFT JOIN securities s ON m.symbol = s.symbol")
                .where_clause("s.exchange = ?")
                .where_clause("m.timestamp BETWEEN ? AND ?")
                .where_clause("m.volume > ?")
                .order_by("m.timestamp DESC")
                .limit(1000);
            black_box(builder.build())
        })
    });
    
    group.bench_function("aggregation_query_build", |b| {
        b.iter(|| {
            let mut builder = QueryBuilder::new();
            builder
                .select(&["symbol", "DATE(timestamp) as date"])
                .select(&["AVG(close) as avg_close", "SUM(volume) as total_volume"])
                .select(&["MIN(low) as min_low", "MAX(high) as max_high"])
                .from("market_data")
                .where_clause("timestamp >= ?")
                .group_by("symbol, DATE(timestamp)")
                .having("SUM(volume) > ?")
                .order_by("date DESC")
                .limit(365);
            black_box(builder.build())
        })
    });
    
    group.finish();
}

fn bench_data_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_processing");
    
    // Create sample data
    let small_dataset: Vec<MarketData> = (0..100).map(|i| {
        MarketData::OHLCV {
            timestamp: UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
            open: 100.0 + (i as f64 * 0.1) % 10.0,
            high: 105.0 + (i as f64 * 0.1) % 10.0,
            low: 95.0 + (i as f64 * 0.1) % 10.0,
            close: 102.0 + (i as f64 * 0.1) % 10.0,
            volume: 1000.0 + (i as f64 * 10.0) % 5000.0,
        }
    }).collect();
    
    let large_dataset: Vec<MarketData> = (0..10000).map(|i| {
        MarketData::OHLCV {
            timestamp: UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
            open: 100.0 + (i as f64 * 0.01) % 50.0,
            high: 105.0 + (i as f64 * 0.01) % 50.0,
            low: 95.0 + (i as f64 * 0.01) % 50.0,
            close: 102.0 + (i as f64 * 0.01) % 50.0,
            volume: 1000.0 + (i as f64) % 50000.0,
        }
    }).collect();
    
    group.bench_function("small_dataset_processing", |b| {
        b.iter(|| {
            // Simulate data processing operations
            let processed: Vec<_> = small_dataset.iter().map(|data| {
                if let MarketData::OHLCV { close, volume, .. } = data {
                    close * volume
                } else {
                    0.0
                }
            }).collect();
            black_box(processed)
        })
    });
    
    group.bench_function("large_dataset_processing", |b| {
        b.iter(|| {
            let processed: Vec<_> = large_dataset.iter().map(|data| {
                if let MarketData::OHLCV { close, volume, .. } = data {
                    close * volume
                } else {
                    0.0
                }
            }).collect();
            black_box(processed)
        })
    });
    
    group.finish();
}

fn bench_caching_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("caching_operations");
    
    // Mock cache implementation
    struct MockCache {
        data: std::sync::Mutex<HashMap<String, Vec<MarketData>>>,
    }
    
    impl MockCache {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(HashMap::new()),
            }
        }
        
        fn get(&self, key: &str) -> Option<Vec<MarketData>> {
            self.data.lock().unwrap().get(key).cloned()
        }
        
        fn set(&self, key: String, value: Vec<MarketData>) {
            self.data.lock().unwrap().insert(key, value);
        }
        
        fn invalidate(&self, key: &str) {
            self.data.lock().unwrap().remove(key);
        }
    }
    
    let cache = MockCache::new();
    
    // Prepare test data
    let test_data: Vec<MarketData> = (0..1000).map(|i| {
        MarketData::OHLCV {
            timestamp: UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
            open: 100.0 + (i as f64 * 0.1) % 10.0,
            high: 105.0 + (i as f64 * 0.1) % 10.0,
            low: 95.0 + (i as f64 * 0.1) % 10.0,
            close: 102.0 + (i as f64 * 0.1) % 10.0,
            volume: 1000.0 + (i as f64 * 10.0) % 5000.0,
        }
    }).collect();
    
    group.bench_function("cache_miss", |b| {
        b.iter(|| {
            let key = format!("AAPL_2024_01_01_{}", black_box(rand::random::<u32>()));
            black_box(cache.get(&key))
        })
    });
    
    group.bench_function("cache_set", |b| {
        b.iter(|| {
            let key = format!("AAPL_2024_01_01_{}", black_box(rand::random::<u32>()));
            cache.set(key, test_data.clone());
        })
    });
    
    // Pre-populate cache for hit benchmark
    cache.set("AAPL_2024_01_01_cached".to_string(), test_data.clone());
    
    group.bench_function("cache_hit", |b| {
        b.iter(|| {
            black_box(cache.get("AAPL_2024_01_01_cached"))
        })
    });
    
    group.bench_function("cache_invalidate", |b| {
        b.iter(|| {
            let key = format!("AAPL_2024_01_01_{}", black_box(rand::random::<u32>()));
            cache.set(key.clone(), test_data.clone());
            cache.invalidate(&key);
        })
    });
    
    group.finish();
}

fn bench_concurrent_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_operations");
    group.sample_size(10);
    
    let db = MockDbConnection::new();
    
    for concurrency in [1, 5, 10, 20].iter() {
        group.bench_with_input(BenchmarkId::new("concurrent_queries", concurrency), concurrency, |b, &concurrency| {
            b.iter(|| {
                rt.block_on(async {
                    let mut handles = Vec::new();
                    
                    for i in 0..concurrency {
                        let query = format!("SELECT * FROM market_data WHERE id > {} LIMIT 100", i * 100);
                        let handle = tokio::spawn(async move {
                            let mock_db = MockDbConnection::new();
                            mock_db.execute_query(&query).await
                        });
                        handles.push(handle);
                    }
                    
                    let results: Vec<_> = futures::future::join_all(handles).await
                        .into_iter()
                        .map(|r| r.unwrap())
                        .collect();
                    
                    black_box(results)
                })
            })
        });
    }
    
    group.finish();
}

fn bench_data_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_serialization");
    
    let market_data = MarketData::OHLCV {
        timestamp: SystemTime::now(),
        open: 150.25,
        high: 152.75,
        low: 149.80,
        close: 151.45,
        volume: 1250000.0,
    };
    
    group.bench_function("serialize_json", |b| {
        b.iter(|| {
            black_box(serde_json::to_string(&market_data).unwrap())
        })
    });
    
    let json_data = serde_json::to_string(&market_data).unwrap();
    group.bench_function("deserialize_json", |b| {
        b.iter(|| {
            black_box(serde_json::from_str::<MarketData>(&json_data).unwrap())
        })
    });
    
    group.bench_function("serialize_bincode", |b| {
        b.iter(|| {
            black_box(bincode::serialize(&market_data).unwrap())
        })
    });
    
    let bincode_data = bincode::serialize(&market_data).unwrap();
    group.bench_function("deserialize_bincode", |b| {
        b.iter(|| {
            black_box(bincode::deserialize::<MarketData>(&bincode_data).unwrap())
        })
    });
    
    group.finish();
}

criterion_group!(
    dataloader_benches,
    bench_dataloader_creation,
    bench_market_data_queries,
    bench_query_builder,
    bench_data_processing,
    bench_caching_operations,
    bench_concurrent_operations,
    bench_data_serialization
);

criterion_main!(dataloader_benches);
