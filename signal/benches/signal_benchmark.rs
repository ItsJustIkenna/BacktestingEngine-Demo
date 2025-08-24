//! Benchmarks for the signal module

use signal::{TechnicalIndicator, SignalProcessor, SignalFilter, SignalAggregator};
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::collections::VecDeque;
use std::time::{SystemTime, Duration};

// Mock price data structure
#[derive(Clone, Debug)]
struct PriceData {
    timestamp: SystemTime,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

// RSI Technical Indicator Implementation
struct RSI {
    period: usize,
    price_changes: VecDeque<f64>,
    gains: VecDeque<f64>,
    losses: VecDeque<f64>,
    avg_gain: f64,
    avg_loss: f64,
}

impl RSI {
    fn new(period: usize) -> Self {
        Self {
            period,
            price_changes: VecDeque::new(),
            gains: VecDeque::new(),
            losses: VecDeque::new(),
            avg_gain: 0.0,
            avg_loss: 0.0,
        }
    }
    
    fn update(&mut self, price: f64) -> Option<f64> {
        if let Some(&last_price) = self.price_changes.back() {
            let change = price - last_price;
            
            if self.gains.len() >= self.period {
                self.gains.pop_front();
                self.losses.pop_front();
            }
            
            if change > 0.0 {
                self.gains.push_back(change);
                self.losses.push_back(0.0);
            } else {
                self.gains.push_back(0.0);
                self.losses.push_back(-change);
            }
            
            if self.gains.len() == self.period {
                self.avg_gain = self.gains.iter().sum::<f64>() / self.period as f64;
                self.avg_loss = self.losses.iter().sum::<f64>() / self.period as f64;
                
                if self.avg_loss == 0.0 {
                    return Some(100.0);
                }
                
                let rs = self.avg_gain / self.avg_loss;
                let rsi = 100.0 - (100.0 / (1.0 + rs));
                
                self.price_changes.push_back(price);
                if self.price_changes.len() > self.period + 1 {
                    self.price_changes.pop_front();
                }
                
                return Some(rsi);
            }
        }
        
        self.price_changes.push_back(price);
        if self.price_changes.len() > self.period + 1 {
            self.price_changes.pop_front();
        }
        
        None
    }
    
    fn calculate(&self) -> Option<f64> {
        if self.gains.len() < self.period {
            return None;
        }
        
        if self.avg_loss == 0.0 {
            return Some(100.0);
        }
        
        let rs = self.avg_gain / self.avg_loss;
        Some(100.0 - (100.0 / (1.0 + rs)))
    }
}

// Simple Moving Average Implementation
struct SMA {
    period: usize,
    values: VecDeque<f64>,
    sum: f64,
}

impl SMA {
    fn new(period: usize) -> Self {
        Self {
            period,
            values: VecDeque::new(),
            sum: 0.0,
        }
    }
    
    fn update(&mut self, value: f64) -> Option<f64> {
        self.values.push_back(value);
        self.sum += value;
        
        if self.values.len() > self.period {
            if let Some(old_value) = self.values.pop_front() {
                self.sum -= old_value;
            }
        }
        
        if self.values.len() == self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }
    
    fn calculate(&self) -> Option<f64> {
        if self.values.len() == self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }
}

// Exponential Moving Average Implementation
struct EMA {
    period: usize,
    multiplier: f64,
    current_value: Option<f64>,
}

impl EMA {
    fn new(period: usize) -> Self {
        let multiplier = 2.0 / (period as f64 + 1.0);
        Self {
            period,
            multiplier,
            current_value: None,
        }
    }
    
    fn update(&mut self, value: f64) -> Option<f64> {
        match self.current_value {
            None => {
                self.current_value = Some(value);
                Some(value)
            }
            Some(current) => {
                let new_value = value * self.multiplier + current * (1.0 - self.multiplier);
                self.current_value = Some(new_value);
                Some(new_value)
            }
        }
    }
    
    fn calculate(&self) -> Option<f64> {
        self.current_value
    }
}

// Bollinger Bands Implementation
struct BollingerBands {
    sma: SMA,
    period: usize,
    std_dev_multiplier: f64,
    values: VecDeque<f64>,
}

impl BollingerBands {
    fn new(period: usize, std_dev_multiplier: f64) -> Self {
        Self {
            sma: SMA::new(period),
            period,
            std_dev_multiplier,
            values: VecDeque::new(),
        }
    }
    
    fn update(&mut self, value: f64) -> Option<(f64, f64, f64)> { // (upper, middle, lower)
        self.values.push_back(value);
        if self.values.len() > self.period {
            self.values.pop_front();
        }
        
        if let Some(middle) = self.sma.update(value) {
            if self.values.len() == self.period {
                let variance = self.values.iter()
                    .map(|&x| (x - middle).powi(2))
                    .sum::<f64>() / self.period as f64;
                let std_dev = variance.sqrt();
                
                let upper = middle + self.std_dev_multiplier * std_dev;
                let lower = middle - self.std_dev_multiplier * std_dev;
                
                return Some((upper, middle, lower));
            }
        }
        
        None
    }
}

// MACD Implementation
struct MACD {
    fast_ema: EMA,
    slow_ema: EMA,
    signal_ema: EMA,
    macd_values: VecDeque<f64>,
}

impl MACD {
    fn new(fast_period: usize, slow_period: usize, signal_period: usize) -> Self {
        Self {
            fast_ema: EMA::new(fast_period),
            slow_ema: EMA::new(slow_period),
            signal_ema: EMA::new(signal_period),
            macd_values: VecDeque::new(),
        }
    }
    
    fn update(&mut self, price: f64) -> Option<(f64, f64, f64)> { // (macd, signal, histogram)
        let fast = self.fast_ema.update(price)?;
        let slow = self.slow_ema.update(price)?;
        
        let macd = fast - slow;
        let signal = self.signal_ema.update(macd)?;
        let histogram = macd - signal;
        
        Some((macd, signal, histogram))
    }
}

// Signal Filter Implementation
#[derive(Clone, Debug)]
enum SignalStrength {
    Weak,
    Moderate,
    Strong,
}

#[derive(Clone, Debug)]
struct TechnicalSignal {
    indicator_name: String,
    value: f64,
    strength: SignalStrength,
    timestamp: SystemTime,
    buy_sell: i8, // 1 for buy, -1 for sell, 0 for neutral
}

struct BenchSignalProcessor {
    rsi: RSI,
    sma_short: SMA,
    sma_long: SMA,
    ema: EMA,
    bollinger: BollingerBands,
    macd: MACD,
    filters: Vec<SignalFilter>,
}

#[derive(Clone)]
struct SignalFilter {
    min_strength: f64,
    indicator_weights: std::collections::HashMap<String, f64>,
}

impl SignalFilter {
    fn new() -> Self {
        let mut weights = std::collections::HashMap::new();
        weights.insert("RSI".to_string(), 0.3);
        weights.insert("SMA".to_string(), 0.2);
        weights.insert("EMA".to_string(), 0.15);
        weights.insert("MACD".to_string(), 0.25);
        weights.insert("Bollinger".to_string(), 0.1);
        
        Self {
            min_strength: 0.6,
            indicator_weights: weights,
        }
    }
    
    fn apply(&self, signals: &[TechnicalSignal]) -> Vec<TechnicalSignal> {
        signals.iter()
            .filter(|signal| self.passes_filter(signal))
            .cloned()
            .collect()
    }
    
    fn passes_filter(&self, signal: &TechnicalSignal) -> bool {
        let weight = self.indicator_weights.get(&signal.indicator_name).unwrap_or(&0.5);
        signal.value.abs() * weight >= self.min_strength
    }
}

impl BenchSignalProcessor {
    fn new() -> Self {
        Self {
            rsi: RSI::new(14),
            sma_short: SMA::new(10),
            sma_long: SMA::new(20),
            ema: EMA::new(12),
            bollinger: BollingerBands::new(20, 2.0),
            macd: MACD::new(12, 26, 9),
            filters: vec![SignalFilter::new()],
        }
    }
    
    fn process(&mut self, price_data: &PriceData) -> Vec<TechnicalSignal> {
        let mut signals = Vec::new();
        let close = price_data.close;
        
        // RSI Signal
        if let Some(rsi_value) = self.rsi.update(close) {
            let signal = if rsi_value > 70.0 {
                TechnicalSignal {
                    indicator_name: "RSI".to_string(),
                    value: (rsi_value - 70.0) / 30.0,
                    strength: if rsi_value > 80.0 { SignalStrength::Strong } else { SignalStrength::Moderate },
                    timestamp: price_data.timestamp,
                    buy_sell: -1,
                }
            } else if rsi_value < 30.0 {
                TechnicalSignal {
                    indicator_name: "RSI".to_string(),
                    value: (30.0 - rsi_value) / 30.0,
                    strength: if rsi_value < 20.0 { SignalStrength::Strong } else { SignalStrength::Moderate },
                    timestamp: price_data.timestamp,
                    buy_sell: 1,
                }
            } else {
                TechnicalSignal {
                    indicator_name: "RSI".to_string(),
                    value: 0.0,
                    strength: SignalStrength::Weak,
                    timestamp: price_data.timestamp,
                    buy_sell: 0,
                }
            };
            signals.push(signal);
        }
        
        // SMA Crossover Signal
        if let (Some(sma_short), Some(sma_long)) = (self.sma_short.update(close), self.sma_long.calculate()) {
            let crossover = (sma_short - sma_long) / sma_long;
            let signal = TechnicalSignal {
                indicator_name: "SMA".to_string(),
                value: crossover.abs(),
                strength: if crossover.abs() > 0.02 { SignalStrength::Strong } else if crossover.abs() > 0.01 { SignalStrength::Moderate } else { SignalStrength::Weak },
                timestamp: price_data.timestamp,
                buy_sell: if crossover > 0.01 { 1 } else if crossover < -0.01 { -1 } else { 0 },
            };
            signals.push(signal);
        }
        
        // MACD Signal
        if let Some((macd, signal_line, histogram)) = self.macd.update(close) {
            let signal = TechnicalSignal {
                indicator_name: "MACD".to_string(),
                value: histogram.abs(),
                strength: if histogram.abs() > 2.0 { SignalStrength::Strong } else if histogram.abs() > 1.0 { SignalStrength::Moderate } else { SignalStrength::Weak },
                timestamp: price_data.timestamp,
                buy_sell: if histogram > 0.5 { 1 } else if histogram < -0.5 { -1 } else { 0 },
            };
            signals.push(signal);
        }
        
        // Bollinger Bands Signal
        if let Some((upper, middle, lower)) = self.bollinger.update(close) {
            let signal = if close > upper {
                TechnicalSignal {
                    indicator_name: "Bollinger".to_string(),
                    value: (close - upper) / (upper - middle),
                    strength: SignalStrength::Moderate,
                    timestamp: price_data.timestamp,
                    buy_sell: -1,
                }
            } else if close < lower {
                TechnicalSignal {
                    indicator_name: "Bollinger".to_string(),
                    value: (lower - close) / (middle - lower),
                    strength: SignalStrength::Moderate,
                    timestamp: price_data.timestamp,
                    buy_sell: 1,
                }
            } else {
                TechnicalSignal {
                    indicator_name: "Bollinger".to_string(),
                    value: 0.0,
                    strength: SignalStrength::Weak,
                    timestamp: price_data.timestamp,
                    buy_sell: 0,
                }
            };
            signals.push(signal);
        }
        
        signals
    }
    
    fn filter_signals(&self, signals: Vec<TechnicalSignal>) -> Vec<TechnicalSignal> {
        let mut filtered_signals = signals;
        
        for filter in &self.filters {
            filtered_signals = filter.apply(&filtered_signals);
        }
        
        filtered_signals
    }
}

// Signal Aggregator Implementation
struct BenchSignalAggregator {
    weights: std::collections::HashMap<String, f64>,
    consensus_threshold: f64,
}

impl BenchSignalAggregator {
    fn new() -> Self {
        let mut weights = std::collections::HashMap::new();
        weights.insert("RSI".to_string(), 0.25);
        weights.insert("SMA".to_string(), 0.25);
        weights.insert("EMA".to_string(), 0.15);
        weights.insert("MACD".to_string(), 0.25);
        weights.insert("Bollinger".to_string(), 0.10);
        
        Self {
            weights,
            consensus_threshold: 0.6,
        }
    }
    
    fn aggregate(&self, signals: &[TechnicalSignal]) -> Option<TechnicalSignal> {
        if signals.is_empty() {
            return None;
        }
        
        let mut buy_score = 0.0;
        let mut sell_score = 0.0;
        let mut total_weight = 0.0;
        
        for signal in signals {
            let weight = self.weights.get(&signal.indicator_name).unwrap_or(&0.2);
            total_weight += weight;
            
            match signal.buy_sell {
                1 => buy_score += weight * signal.value,
                -1 => sell_score += weight * signal.value,
                _ => {}
            }
        }
        
        let net_score = buy_score - sell_score;
        let normalized_score = net_score / total_weight;
        
        if normalized_score.abs() >= self.consensus_threshold {
            Some(TechnicalSignal {
                indicator_name: "Aggregated".to_string(),
                value: normalized_score.abs(),
                strength: if normalized_score.abs() > 0.8 { 
                    SignalStrength::Strong 
                } else { 
                    SignalStrength::Moderate 
                },
                timestamp: signals[0].timestamp,
                buy_sell: if normalized_score > 0.0 { 1 } else { -1 },
            })
        } else {
            None
        }
    }
}

// Helper function to generate test price data
fn generate_price_data(count: usize, start_price: f64) -> Vec<PriceData> {
    let mut data = Vec::new();
    let mut current_price = start_price;
    
    for i in 0..count {
        let volatility = 0.02;
        let trend = 0.0001; // Slight upward trend
        let random_change = (rand::random::<f64>() - 0.5) * volatility;
        
        current_price *= 1.0 + trend + random_change;
        
        let high = current_price * (1.0 + rand::random::<f64>() * 0.005);
        let low = current_price * (1.0 - rand::random::<f64>() * 0.005);
        
        data.push(PriceData {
            timestamp: std::time::UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
            open: current_price,
            high,
            low,
            close: current_price,
            volume: 1000.0 + rand::random::<f64>() * 5000.0,
        });
    }
    
    data
}

fn bench_technical_indicators(c: &mut Criterion) {
    let mut group = c.benchmark_group("technical_indicators");
    
    let price_data = generate_price_data(1000, 100.0);
    let prices: Vec<f64> = price_data.iter().map(|d| d.close).collect();
    
    group.bench_function("rsi_calculation", |b| {
        b.iter(|| {
            let mut rsi = RSI::new(14);
            let mut results = Vec::new();
            for &price in &prices[0..100] {
                if let Some(value) = rsi.update(price) {
                    results.push(value);
                }
            }
            black_box(results)
        })
    });
    
    group.bench_function("sma_calculation", |b| {
        b.iter(|| {
            let mut sma = SMA::new(20);
            let mut results = Vec::new();
            for &price in &prices[0..100] {
                if let Some(value) = sma.update(price) {
                    results.push(value);
                }
            }
            black_box(results)
        })
    });
    
    group.bench_function("ema_calculation", |b| {
        b.iter(|| {
            let mut ema = EMA::new(12);
            let mut results = Vec::new();
            for &price in &prices[0..100] {
                if let Some(value) = ema.update(price) {
                    results.push(value);
                }
            }
            black_box(results)
        })
    });
    
    group.bench_function("bollinger_bands_calculation", |b| {
        b.iter(|| {
            let mut bollinger = BollingerBands::new(20, 2.0);
            let mut results = Vec::new();
            for &price in &prices[0..100] {
                if let Some(bands) = bollinger.update(price) {
                    results.push(bands);
                }
            }
            black_box(results)
        })
    });
    
    group.bench_function("macd_calculation", |b| {
        b.iter(|| {
            let mut macd = MACD::new(12, 26, 9);
            let mut results = Vec::new();
            for &price in &prices[0..100] {
                if let Some(values) = macd.update(price) {
                    results.push(values);
                }
            }
            black_box(results)
        })
    });
    
    group.finish();
}

fn bench_signal_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_processing");
    
    let price_data = generate_price_data(500, 100.0);
    let mut processor = BenchSignalProcessor::new();
    
    group.bench_function("single_data_point", |b| {
        b.iter(|| {
            black_box(processor.process(&price_data[100]))
        })
    });
    
    group.bench_function("batch_processing", |b| {
        b.iter(|| {
            let mut all_signals = Vec::new();
            for data in &price_data[50..150] {
                let signals = processor.process(data);
                all_signals.extend(signals);
            }
            black_box(all_signals)
        })
    });
    
    group.finish();
}

fn bench_signal_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_filtering");
    
    // Generate mock signals
    let test_signals: Vec<TechnicalSignal> = (0..100).map(|i| {
        TechnicalSignal {
            indicator_name: format!("Indicator{}", i % 5),
            value: rand::random::<f64>(),
            strength: if i % 3 == 0 { SignalStrength::Strong } else if i % 3 == 1 { SignalStrength::Moderate } else { SignalStrength::Weak },
            timestamp: std::time::SystemTime::now(),
            buy_sell: (i % 3 - 1) as i8,
        }
    }).collect();
    
    let filter = SignalFilter::new();
    
    for signal_count in [10, 50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("filter_signals", signal_count),
            signal_count,
            |b, &count| {
                let signals = &test_signals[0..count.min(test_signals.len())];
                b.iter(|| {
                    black_box(filter.apply(signals))
                })
            }
        );
    }
    
    group.finish();
}

fn bench_signal_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_aggregation");
    
    let aggregator = BenchSignalAggregator::new();
    
    // Create test signals with different combinations
    let conflicting_signals = vec![
        TechnicalSignal {
            indicator_name: "RSI".to_string(),
            value: 0.8,
            strength: SignalStrength::Strong,
            timestamp: SystemTime::now(),
            buy_sell: 1,
        },
        TechnicalSignal {
            indicator_name: "MACD".to_string(),
            value: 0.7,
            strength: SignalStrength::Moderate,
            timestamp: SystemTime::now(),
            buy_sell: -1,
        },
        TechnicalSignal {
            indicator_name: "SMA".to_string(),
            value: 0.6,
            strength: SignalStrength::Moderate,
            timestamp: SystemTime::now(),
            buy_sell: 1,
        },
    ];
    
    let consensus_signals = vec![
        TechnicalSignal {
            indicator_name: "RSI".to_string(),
            value: 0.8,
            strength: SignalStrength::Strong,
            timestamp: SystemTime::now(),
            buy_sell: 1,
        },
        TechnicalSignal {
            indicator_name: "MACD".to_string(),
            value: 0.7,
            strength: SignalStrength::Strong,
            timestamp: SystemTime::now(),
            buy_sell: 1,
        },
        TechnicalSignal {
            indicator_name: "SMA".to_string(),
            value: 0.9,
            strength: SignalStrength::Strong,
            timestamp: SystemTime::now(),
            buy_sell: 1,
        },
    ];
    
    group.bench_function("conflicting_signals", |b| {
        b.iter(|| {
            black_box(aggregator.aggregate(&conflicting_signals))
        })
    });
    
    group.bench_function("consensus_signals", |b| {
        b.iter(|| {
            black_box(aggregator.aggregate(&consensus_signals))
        })
    });
    
    for signal_count in [5, 10, 15, 20].iter() {
        let signals: Vec<TechnicalSignal> = (0..*signal_count).map(|i| {
            TechnicalSignal {
                indicator_name: format!("Indicator{}", i),
                value: 0.5 + (i as f64 * 0.1),
                strength: SignalStrength::Moderate,
                timestamp: SystemTime::now(),
                buy_sell: if i % 2 == 0 { 1 } else { -1 },
            }
        }).collect();
        
        group.bench_with_input(
            BenchmarkId::new("aggregate_multiple", signal_count),
            &signals,
            |b, signals| {
                b.iter(|| {
                    black_box(aggregator.aggregate(signals))
                })
            }
        );
    }
    
    group.finish();
}

fn bench_indicator_combinations(c: &mut Criterion) {
    let mut group = c.benchmark_group("indicator_combinations");
    
    let price_data = generate_price_data(1000, 100.0);
    let prices: Vec<f64> = price_data.iter().map(|d| d.close).collect();
    
    // Benchmark different combinations of indicators
    group.bench_function("rsi_sma_combination", |b| {
        b.iter(|| {
            let mut rsi = RSI::new(14);
            let mut sma_short = SMA::new(10);
            let mut sma_long = SMA::new(20);
            let mut signals = Vec::new();
            
            for &price in &prices[0..200] {
                let rsi_val = rsi.update(price);
                let sma_short_val = sma_short.update(price);
                let sma_long_val = sma_long.update(price);
                
                if let (Some(rsi), Some(sma_s), Some(sma_l)) = (rsi_val, sma_short_val, sma_long_val) {
                    // Simple combination logic
                    let combined_signal = if rsi < 30.0 && sma_s > sma_l {
                        1 // Strong buy
                    } else if rsi > 70.0 && sma_s < sma_l {
                        -1 // Strong sell
                    } else {
                        0 // Hold
                    };
                    signals.push(combined_signal);
                }
            }
            
            black_box(signals)
        })
    });
    
    group.bench_function("full_indicator_suite", |b| {
        b.iter(|| {
            let mut processor = BenchSignalProcessor::new();
            let mut all_signals = Vec::new();
            
            for data in &price_data[0..100] {
                let signals = processor.process(data);
                all_signals.extend(signals);
            }
            
            black_box(all_signals)
        })
    });
    
    group.finish();
}

fn bench_real_time_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_time_processing");
    
    let price_data = generate_price_data(1000, 100.0);
    let mut processor = BenchSignalProcessor::new();
    let aggregator = BenchSignalAggregator::new();
    
    // Simulate real-time tick processing
    group.bench_function("tick_by_tick_processing", |b| {
        b.iter(|| {
            for data in &price_data[500..510] { // Small window for real-time simulation
                let raw_signals = processor.process(data);
                let filtered_signals = processor.filter_signals(raw_signals);
                let final_signal = aggregator.aggregate(&filtered_signals);
                black_box(final_signal);
            }
        })
    });
    
    // Benchmark latency-critical operations
    group.bench_function("low_latency_single_update", |b| {
        b.iter(|| {
            let data = &price_data[100];
            let raw_signals = processor.process(data);
            black_box(raw_signals)
        })
    });
    
    group.finish();
}

criterion_group!(
    signal_benches,
    bench_technical_indicators,
    bench_signal_processing,
    bench_signal_filtering,
    bench_signal_aggregation,
    bench_indicator_combinations,
    bench_real_time_processing
);

criterion_main!(signal_benches);
