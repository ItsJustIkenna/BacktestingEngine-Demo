//! Performance Profiling Tests for Backtest Engine
//! 
//! This module profiles the hot paths to identify bottlenecks.
//! Run with: cargo test --release -p backtest --test performance_profile -- --nocapture

use std::time::{Duration, Instant};
use std::collections::HashMap;

/// Timing accumulator for profiling
#[derive(Default, Debug)]
struct ProfileTimings {
    samples: HashMap<&'static str, Vec<Duration>>,
}

impl ProfileTimings {
    fn record(&mut self, label: &'static str, duration: Duration) {
        self.samples.entry(label).or_default().push(duration);
    }
    
    fn report(&self) {
        println!("\n============================================================");
        println!("  PERFORMANCE PROFILE REPORT");
        println!("============================================================\n");
        
        let mut entries: Vec<_> = self.samples.iter().collect();
        entries.sort_by(|a, b| {
            let total_a: Duration = a.1.iter().sum();
            let total_b: Duration = b.1.iter().sum();
            total_b.cmp(&total_a) // Sort by total time descending
        });
        
        for (label, samples) in entries {
            let count = samples.len();
            let total: Duration = samples.iter().sum();
            let avg = total / count as u32;
            let _min = samples.iter().min().unwrap_or(&Duration::ZERO);
            let _max = samples.iter().max().unwrap_or(&Duration::ZERO);
            
            // Calculate percentiles
            let mut sorted = samples.clone();
            sorted.sort();
            let p50 = sorted.get(count / 2).unwrap_or(&Duration::ZERO);
            let p95 = sorted.get((count as f64 * 0.95) as usize).unwrap_or(&Duration::ZERO);
            let p99 = sorted.get((count as f64 * 0.99) as usize).unwrap_or(&Duration::ZERO);
            
            println!("{:30} | n={:6} | total={:>10.2?} | avg={:>8.2?} | p50={:>8.2?} | p95={:>8.2?} | p99={:>8.2?}",
                label, count, total, avg, p50, p95, p99);
        }
        println!();
    }
}

// ============================================================================
// BIGDECIMAL OPERATIONS PROFILING
// ============================================================================

#[test]
fn profile_bigdecimal_operations() {
    use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};
    
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 100_000;
    
    // Profile BigDecimal creation from f64
    for i in 0..ITERATIONS {
        let price = 50000.0 + (i as f64 * 0.01);
        let start = Instant::now();
        let bd = BigDecimal::from_f64(price).unwrap();
        timings.record("BigDecimal::from_f64", start.elapsed());
        std::hint::black_box(bd);
    }
    
    // Profile BigDecimal to f64 conversion
    let bd = BigDecimal::from_f64(50000.0).unwrap();
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let f = bd.to_f64().unwrap();
        timings.record("BigDecimal::to_f64", start.elapsed());
        std::hint::black_box(f);
    }
    
    // Profile BigDecimal arithmetic
    let bd1 = BigDecimal::from_f64(50000.0).unwrap();
    let bd2 = BigDecimal::from_f64(100.0).unwrap();
    
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = &bd1 + &bd2;
        timings.record("BigDecimal add", start.elapsed());
        std::hint::black_box(result);
    }
    
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = &bd1 * &bd2;
        timings.record("BigDecimal mul", start.elapsed());
        std::hint::black_box(result);
    }
    
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = bd1.clone();
        timings.record("BigDecimal clone", start.elapsed());
        std::hint::black_box(result);
    }
    
    // Profile native f64 operations for comparison
    let f1: f64 = 50000.0;
    let f2: f64 = 100.0;
    
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = f1 + f2;
        timings.record("f64 add (baseline)", start.elapsed());
        std::hint::black_box(result);
    }
    
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = f1 * f2;
        timings.record("f64 mul (baseline)", start.elapsed());
        std::hint::black_box(result);
    }
    
    timings.report();
    
    println!("\n⚠️  BOTTLENECK ANALYSIS:");
    println!("   If BigDecimal operations are 10-100x slower than f64,");
    println!("   consider using f64 for internal calculations and only");
    println!("   using BigDecimal for final output serialization.");
}

// ============================================================================
// HASHMAP vs FxHashMap PROFILING
// ============================================================================

#[test]
fn profile_hashmap_operations() {
    use std::collections::HashMap;
    use rustc_hash::FxHashMap;
    
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 100_000;
    
    // Standard HashMap
    let mut std_map: HashMap<String, f64> = HashMap::new();
    for i in 0..ITERATIONS {
        let key = format!("order_{}", i);
        let start = Instant::now();
        std_map.insert(key.clone(), i as f64);
        timings.record("HashMap insert", start.elapsed());
        
        let start = Instant::now();
        let _ = std_map.get(&key);
        timings.record("HashMap get", start.elapsed());
    }
    
    // FxHashMap (faster for small keys)
    let mut fx_map: FxHashMap<String, f64> = FxHashMap::default();
    for i in 0..ITERATIONS {
        let key = format!("order_{}", i);
        let start = Instant::now();
        fx_map.insert(key.clone(), i as f64);
        timings.record("FxHashMap insert", start.elapsed());
        
        let start = Instant::now();
        let _ = fx_map.get(&key);
        timings.record("FxHashMap get", start.elapsed());
    }
    
    timings.report();
}

// ============================================================================
// STRING ALLOCATION PROFILING
// ============================================================================

#[test]
fn profile_string_operations() {
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 100_000;
    
    // Profile String allocation
    for i in 0..ITERATIONS {
        let start = Instant::now();
        let s = format!("order_{}_symbol_{}", i, i);
        timings.record("String format!", start.elapsed());
        std::hint::black_box(s);
    }
    
    // Profile to_string()
    let base = "BTC/USDT";
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let s = base.to_string();
        timings.record("to_string()", start.elapsed());
        std::hint::black_box(s);
    }
    
    // Profile String clone
    let s = "order_12345_BTC_USDT".to_string();
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let cloned = s.clone();
        timings.record("String clone", start.elapsed());
        std::hint::black_box(cloned);
    }
    
    timings.report();
    
    println!("\n⚠️  BOTTLENECK ANALYSIS:");
    println!("   If string operations dominate, consider:");
    println!("   - Using &str references instead of owned Strings");
    println!("   - Interning common strings");
    println!("   - Using integer IDs instead of string order IDs");
}

// ============================================================================
// VECTOR OPERATIONS PROFILING
// ============================================================================

#[test]
fn profile_vector_operations() {
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 10_000;
    const VEC_SIZE: usize = 1000;
    
    // Profile Vec push without pre-allocation
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let mut v: Vec<f64> = Vec::new();
        for i in 0..VEC_SIZE {
            v.push(i as f64);
        }
        timings.record("Vec push (no prealloc)", start.elapsed());
        std::hint::black_box(v);
    }
    
    // Profile Vec push with pre-allocation
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let mut v: Vec<f64> = Vec::with_capacity(VEC_SIZE);
        for i in 0..VEC_SIZE {
            v.push(i as f64);
        }
        timings.record("Vec push (preallocated)", start.elapsed());
        std::hint::black_box(v);
    }
    
    // Profile Vec retain (common in simulation loop)
    for _ in 0..ITERATIONS {
        let mut v: Vec<String> = (0..100).map(|i| format!("order_{}", i)).collect();
        let start = Instant::now();
        v.retain(|id| !id.ends_with("_50")); // Remove one item
        timings.record("Vec retain", start.elapsed());
        std::hint::black_box(v);
    }
    
    // Profile Vec contains (common for active_order_ids check)
    let v: Vec<String> = (0..100).map(|i| format!("order_{}", i)).collect();
    for i in 0..ITERATIONS {
        let key = format!("order_{}", i % 100);
        let start = Instant::now();
        let _ = v.contains(&key);
        timings.record("Vec contains", start.elapsed());
    }
    
    timings.report();
    
    println!("\n⚠️  BOTTLENECK ANALYSIS:");
    println!("   Vec::contains is O(n) - if active_order_ids is large,");
    println!("   consider using HashSet for O(1) contains check.");
}

// ============================================================================
// DATETIME OPERATIONS PROFILING
// ============================================================================

#[test]
fn profile_datetime_operations() {
    use chrono::{Utc, Duration};
    
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 100_000;
    
    let now = Utc::now();
    
    // Profile Duration creation
    for i in 0..ITERATIONS {
        let start = Instant::now();
        let d = Duration::milliseconds(i as i64);
        timings.record("Duration::milliseconds", start.elapsed());
        std::hint::black_box(d);
    }
    
    // Profile DateTime subtraction
    let later = now + Duration::hours(1);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let diff = later - now;
        timings.record("DateTime subtraction", start.elapsed());
        std::hint::black_box(diff);
    }
    
    // Profile DateTime comparison
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let result = now > later;
        timings.record("DateTime comparison", start.elapsed());
        std::hint::black_box(result);
    }
    
    timings.report();
}

// ============================================================================
// FILTER/ITERATOR CHAIN PROFILING
// ============================================================================

#[test]
fn profile_iterator_chains() {
    use rustc_hash::FxHashMap;
    
    let mut timings = ProfileTimings::default();
    const ITERATIONS: usize = 10_000;
    
    // Simulate order_trackers structure
    #[derive(Clone)]
    #[allow(dead_code)]
    struct OrderTracker {
        order_id: String,
        side: bool, // true = bid
        price: f64,
        placed_at: i64,
    }
    
    let trackers: FxHashMap<String, OrderTracker> = (0..100)
        .map(|i| {
            let id = format!("order_{}", i);
            (id.clone(), OrderTracker {
                order_id: id,
                side: i % 2 == 0,
                price: 50000.0 + (i as f64),
                placed_at: i as i64,
            })
        })
        .collect();
    
    // Profile filter + map + reduce chain (common in simulation)
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let best_bid = trackers.values()
            .filter(|t| t.side)  // Filter for bids
            .map(|t| t.price)
            .reduce(f64::max);
        timings.record("filter+map+reduce chain", start.elapsed());
        std::hint::black_box(best_bid);
    }
    
    // Profile with collect intermediate
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let bid_prices: Vec<f64> = trackers.values()
            .filter(|t| t.side)
            .map(|t| t.price)
            .collect();
        let best = bid_prices.iter().copied().reduce(f64::max);
        timings.record("filter+collect+reduce", start.elapsed());
        std::hint::black_box(best);
    }
    
    timings.report();
}

// ============================================================================
// SUMMARY AND RECOMMENDATIONS
// ============================================================================

#[test]
fn profile_summary() {
    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║             BACKTEST BOTTLENECK ANALYSIS GUIDE               ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                                                              ║");
    println!("║  Run each profile test individually:                         ║");
    println!("║                                                              ║");
    println!("║  1. BigDecimal:  cargo test --release -p backtest \\          ║");
    println!("║                    --test performance_profile \\              ║");
    println!("║                    profile_bigdecimal -- --nocapture         ║");
    println!("║                                                              ║");
    println!("║  2. HashMap:     ...profile_hashmap... --nocapture           ║");
    println!("║  3. Strings:     ...profile_string... --nocapture            ║");
    println!("║  4. Vectors:     ...profile_vector... --nocapture            ║");
    println!("║  5. DateTime:    ...profile_datetime... --nocapture          ║");
    println!("║  6. Iterators:   ...profile_iterator... --nocapture          ║");
    println!("║                                                              ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  COMMON BOTTLENECKS IN BACKTEST ENGINES:                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                                                              ║");
    println!("║  1. BigDecimal arithmetic (100-1000x slower than f64)        ║");
    println!("║     → Use f64 internally, BigDecimal only for output         ║");
    println!("║                                                              ║");
    println!("║  2. String allocations in hot loop                           ║");
    println!("║     → Use string interning or integer IDs                    ║");
    println!("║                                                              ║");
    println!("║  3. Vec::contains on large vectors (O(n))                    ║");
    println!("║     → Use HashSet for O(1) membership check                  ║");
    println!("║                                                              ║");
    println!("║  4. Vec::retain in loop (creates new allocation)             ║");
    println!("║     → Use swap_remove or maintain separate index             ║");
    println!("║                                                              ║");
    println!("║  5. HashMap with default hasher                              ║");
    println!("║     → Use FxHashMap for small keys (already done ✓)          ║");
    println!("║                                                              ║");
    println!("║  6. Clone in iterator chains                                 ║");
    println!("║     → Use references or Copy types                           ║");
    println!("║                                                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("\n");
}
