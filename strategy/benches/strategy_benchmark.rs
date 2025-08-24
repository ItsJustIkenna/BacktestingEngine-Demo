//! Benchmarks for the strategy module

use strategy::{Strategy, StrategyManager, StrategySignal, StrategyParameters, PerformanceMetrics};
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::collections::HashMap;
use std::time::{SystemTime, Duration, UNIX_EPOCH};

// Mock market data structure for strategy testing
#[derive(Clone, Debug)]
struct MarketData {
    timestamp: SystemTime,
    symbol: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

// Moving Average Strategy for benchmarking
struct MovingAverageStrategy {
    id: String,
    symbol: String,
    short_window: usize,
    long_window: usize,
    parameters: StrategyParameters,
    price_history: Vec<f64>,
    performance: PerformanceMetrics,
}

impl MovingAverageStrategy {
    fn new(symbol: String, short_window: usize, long_window: usize) -> Self {
        let mut parameters = StrategyParameters::new();
        parameters.set("short_window", short_window as f64);
        parameters.set("long_window", long_window as f64);
        parameters.set("threshold", 0.02);
        
        Self {
            id: format!("MA_{}_{}", short_window, long_window),
            symbol,
            short_window,
            long_window,
            parameters,
            price_history: Vec::new(),
            performance: PerformanceMetrics::new(),
        }
    }
    
    fn calculate_sma(&self, window: usize) -> Option<f64> {
        if self.price_history.len() < window {
            return None;
        }
        
        let sum: f64 = self.price_history.iter().rev().take(window).sum();
        Some(sum / window as f64)
    }
    
    fn generate_signal(&self) -> Option<StrategySignal> {
        let short_ma = self.calculate_sma(self.short_window)?;
        let long_ma = self.calculate_sma(self.long_window)?;
        
        let signal_strength = (short_ma - long_ma) / long_ma;
        let threshold = self.parameters.get("threshold").unwrap_or(0.02);
        
        if signal_strength > threshold {
            Some(StrategySignal::Buy {
                symbol: self.symbol.clone(),
                strength: signal_strength.min(1.0),
                price: self.price_history.last().copied().unwrap_or(0.0),
                timestamp: SystemTime::now(),
            })
        } else if signal_strength < -threshold {
            Some(StrategySignal::Sell {
                symbol: self.symbol.clone(),
                strength: (-signal_strength).min(1.0),
                price: self.price_history.last().copied().unwrap_or(0.0),
                timestamp: SystemTime::now(),
            })
        } else {
            Some(StrategySignal::Hold {
                symbol: self.symbol.clone(),
                timestamp: SystemTime::now(),
            })
        }
    }
    
    fn update(&mut self, market_data: &MarketData) -> Option<StrategySignal> {
        self.price_history.push(market_data.close);
        
        // Keep only necessary history
        let max_window = self.long_window.max(self.short_window);
        if self.price_history.len() > max_window * 2 {
            self.price_history.drain(0..max_window);
        }
        
        self.generate_signal()
    }
}

// RSI Strategy for benchmarking
struct RSIStrategy {
    id: String,
    symbol: String,
    period: usize,
    overbought: f64,
    oversold: f64,
    parameters: StrategyParameters,
    price_changes: Vec<f64>,
    performance: PerformanceMetrics,
}

impl RSIStrategy {
    fn new(symbol: String, period: usize) -> Self {
        let mut parameters = StrategyParameters::new();
        parameters.set("period", period as f64);
        parameters.set("overbought", 70.0);
        parameters.set("oversold", 30.0);
        
        Self {
            id: format!("RSI_{}", period),
            symbol,
            period,
            overbought: 70.0,
            oversold: 30.0,
            parameters,
            price_changes: Vec::new(),
            performance: PerformanceMetrics::new(),
        }
    }
    
    fn calculate_rsi(&self) -> Option<f64> {
        if self.price_changes.len() < self.period {
            return None;
        }
        
        let recent_changes: Vec<f64> = self.price_changes.iter().rev().take(self.period).cloned().collect();
        
        let gains: f64 = recent_changes.iter().filter(|&&x| x > 0.0).sum();
        let losses: f64 = recent_changes.iter().filter(|&&x| x < 0.0).map(|x| -x).sum();
        
        let avg_gain = gains / self.period as f64;
        let avg_loss = losses / self.period as f64;
        
        if avg_loss == 0.0 {
            return Some(100.0);
        }
        
        let rs = avg_gain / avg_loss;
        Some(100.0 - (100.0 / (1.0 + rs)))
    }
    
    fn generate_signal(&self) -> Option<StrategySignal> {
        let rsi = self.calculate_rsi()?;
        
        if rsi > self.overbought {
            Some(StrategySignal::Sell {
                symbol: self.symbol.clone(),
                strength: (rsi - self.overbought) / (100.0 - self.overbought),
                price: 0.0, // Would be filled by market data
                timestamp: SystemTime::now(),
            })
        } else if rsi < self.oversold {
            Some(StrategySignal::Buy {
                symbol: self.symbol.clone(),
                strength: (self.oversold - rsi) / self.oversold,
                price: 0.0, // Would be filled by market data
                timestamp: SystemTime::now(),
            })
        } else {
            Some(StrategySignal::Hold {
                symbol: self.symbol.clone(),
                timestamp: SystemTime::now(),
            })
        }
    }
    
    fn update(&mut self, market_data: &MarketData) -> Option<StrategySignal> {
        if let Some(last_price) = self.price_changes.last() {
            let change = market_data.close - last_price;
            self.price_changes.push(change);
        } else {
            self.price_changes.push(0.0);
        }
        
        // Keep only necessary history
        if self.price_changes.len() > self.period * 2 {
            self.price_changes.drain(0..self.period);
        }
        
        self.generate_signal()
    }
}

// Mock strategy manager for benchmarking
struct BenchStrategyManager {
    strategies: HashMap<String, Box<dyn BenchStrategy>>,
    performance_cache: HashMap<String, PerformanceMetrics>,
}

trait BenchStrategy {
    fn id(&self) -> &str;
    fn update(&mut self, market_data: &MarketData) -> Option<StrategySignal>;
    fn get_parameters(&self) -> &StrategyParameters;
    fn set_parameters(&mut self, params: StrategyParameters);
    fn get_performance(&self) -> &PerformanceMetrics;
}

impl BenchStrategy for MovingAverageStrategy {
    fn id(&self) -> &str { &self.id }
    fn update(&mut self, market_data: &MarketData) -> Option<StrategySignal> {
        self.update(market_data)
    }
    fn get_parameters(&self) -> &StrategyParameters { &self.parameters }
    fn set_parameters(&mut self, params: StrategyParameters) { self.parameters = params; }
    fn get_performance(&self) -> &PerformanceMetrics { &self.performance }
}

impl BenchStrategy for RSIStrategy {
    fn id(&self) -> &str { &self.id }
    fn update(&mut self, market_data: &MarketData) -> Option<StrategySignal> {
        self.update(market_data)
    }
    fn get_parameters(&self) -> &StrategyParameters { &self.parameters }
    fn set_parameters(&mut self, params: StrategyParameters) { self.parameters = params; }
    fn get_performance(&self) -> &PerformanceMetrics { &self.performance }
}

impl BenchStrategyManager {
    fn new() -> Self {
        Self {
            strategies: HashMap::new(),
            performance_cache: HashMap::new(),
        }
    }
    
    fn add_strategy(&mut self, strategy: Box<dyn BenchStrategy>) {
        self.strategies.insert(strategy.id().to_string(), strategy);
    }
    
    fn update_all(&mut self, market_data: &MarketData) -> Vec<StrategySignal> {
        let mut signals = Vec::new();
        
        for (_, strategy) in &mut self.strategies {
            if let Some(signal) = strategy.update(market_data) {
                signals.push(signal);
            }
        }
        
        signals
    }
    
    fn get_strategy(&self, id: &str) -> Option<&Box<dyn BenchStrategy>> {
        self.strategies.get(id)
    }
    
    fn calculate_aggregate_performance(&self) -> PerformanceMetrics {
        let mut aggregate = PerformanceMetrics::new();
        
        for (_, strategy) in &self.strategies {
            let perf = strategy.get_performance();
            aggregate.total_return += perf.total_return;
            aggregate.win_rate = (aggregate.win_rate + perf.win_rate) / 2.0;
            aggregate.sharpe_ratio = (aggregate.sharpe_ratio + perf.sharpe_ratio) / 2.0;
        }
        
        aggregate
    }
}

// Mock structures for benchmarking
#[derive(Clone)]
struct StrategyParameters {
    params: HashMap<String, f64>,
}

impl StrategyParameters {
    fn new() -> Self {
        Self {
            params: HashMap::new(),
        }
    }
    
    fn set(&mut self, key: &str, value: f64) {
        self.params.insert(key.to_string(), value);
    }
    
    fn get(&self, key: &str) -> Option<f64> {
        self.params.get(key).copied()
    }
}

#[derive(Clone, Debug)]
struct PerformanceMetrics {
    total_return: f64,
    win_rate: f64,
    sharpe_ratio: f64,
    max_drawdown: f64,
    total_trades: u64,
    winning_trades: u64,
    losing_trades: u64,
}

impl PerformanceMetrics {
    fn new() -> Self {
        Self {
            total_return: 0.0,
            win_rate: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
        }
    }
}

#[derive(Clone, Debug)]
enum StrategySignal {
    Buy { symbol: String, strength: f64, price: f64, timestamp: SystemTime },
    Sell { symbol: String, strength: f64, price: f64, timestamp: SystemTime },
    Hold { symbol: String, timestamp: SystemTime },
}

// Helper function to generate test market data
fn generate_market_data(symbol: &str, count: usize, start_price: f64) -> Vec<MarketData> {
    let mut data = Vec::new();
    let mut current_price = start_price;
    
    for i in 0..count {
        let volatility = 0.02;
        let random_change = (rand::random::<f64>() - 0.5) * volatility * current_price;
        current_price += random_change;
        
        let high = current_price * (1.0 + rand::random::<f64>() * 0.01);
        let low = current_price * (1.0 - rand::random::<f64>() * 0.01);
        
        data.push(MarketData {
            timestamp: UNIX_EPOCH + Duration::from_secs(i as u64 * 60),
            symbol: symbol.to_string(),
            open: current_price,
            high,
            low,
            close: current_price,
            volume: 1000.0 + rand::random::<f64>() * 5000.0,
        });
    }
    
    data
}

fn bench_strategy_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_creation");
    
    group.bench_function("moving_average_strategy", |b| {
        b.iter(|| {
            black_box(MovingAverageStrategy::new("AAPL".to_string(), 10, 20))
        })
    });
    
    group.bench_function("rsi_strategy", |b| {
        b.iter(|| {
            black_box(RSIStrategy::new("AAPL".to_string(), 14))
        })
    });
    
    group.bench_function("strategy_manager", |b| {
        b.iter(|| {
            black_box(BenchStrategyManager::new())
        })
    });
    
    group.finish();
}

fn bench_signal_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("signal_generation");
    
    let market_data = generate_market_data("AAPL", 100, 150.0);
    let mut ma_strategy = MovingAverageStrategy::new("AAPL".to_string(), 10, 20);
    let mut rsi_strategy = RSIStrategy::new("AAPL".to_string(), 14);
    
    // Pre-populate with some data
    for data in &market_data[0..50] {
        ma_strategy.update(data);
        rsi_strategy.update(data);
    }
    
    group.bench_function("ma_signal_generation", |b| {
        b.iter(|| {
            for data in &market_data[50..60] {
                black_box(ma_strategy.update(data));
            }
        })
    });
    
    group.bench_function("rsi_signal_generation", |b| {
        b.iter(|| {
            for data in &market_data[50..60] {
                black_box(rsi_strategy.update(data));
            }
        })
    });
    
    group.finish();
}

fn bench_strategy_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_calculations");
    
    let market_data = generate_market_data("AAPL", 1000, 150.0);
    let mut ma_strategy = MovingAverageStrategy::new("AAPL".to_string(), 50, 200);
    
    // Pre-populate strategy with data
    for data in &market_data[0..250] {
        ma_strategy.update(data);
    }
    
    group.bench_function("sma_calculation_short", |b| {
        b.iter(|| {
            black_box(ma_strategy.calculate_sma(10))
        })
    });
    
    group.bench_function("sma_calculation_long", |b| {
        b.iter(|| {
            black_box(ma_strategy.calculate_sma(200))
        })
    });
    
    let mut rsi_strategy = RSIStrategy::new("AAPL".to_string(), 14);
    for data in &market_data[0..50] {
        rsi_strategy.update(data);
    }
    
    group.bench_function("rsi_calculation", |b| {
        b.iter(|| {
            black_box(rsi_strategy.calculate_rsi())
        })
    });
    
    group.finish();
}

fn bench_strategy_manager_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("strategy_manager_operations");
    
    let market_data = generate_market_data("AAPL", 100, 150.0);
    
    for strategy_count in [5, 10, 25, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new("update_multiple_strategies", strategy_count),
            strategy_count,
            |b, &count| {
                let mut manager = BenchStrategyManager::new();
                
                // Add strategies
                for i in 0..count {
                    if i % 2 == 0 {
                        manager.add_strategy(Box::new(MovingAverageStrategy::new(
                            "AAPL".to_string(), 
                            10 + i, 
                            20 + i * 2
                        )));
                    } else {
                        manager.add_strategy(Box::new(RSIStrategy::new(
                            "AAPL".to_string(), 
                            14 + i / 2
                        )));
                    }
                }
                
                b.iter(|| {
                    for data in &market_data[0..10] {
                        black_box(manager.update_all(data));
                    }
                })
            }
        );
    }
    
    group.finish();
}

fn bench_parameter_optimization(c: &mut Criterion) {
    let mut group = c.benchmark_group("parameter_optimization");
    
    let market_data = generate_market_data("AAPL", 500, 150.0);
    
    // Benchmark parameter sweeps
    group.bench_function("ma_parameter_sweep", |b| {
        b.iter(|| {
            let mut best_return = -1.0;
            let mut best_params = (0, 0);
            
            for short in [5, 10, 15, 20] {
                for long in [25, 50, 75, 100] {
                    if short >= long { continue; }
                    
                    let mut strategy = MovingAverageStrategy::new("AAPL".to_string(), short, long);
                    let mut signals = Vec::new();
                    
                    for data in &market_data {
                        if let Some(signal) = strategy.update(data) {
                            signals.push(signal);
                        }
                    }
                    
                    // Simplified return calculation
                    let total_return = signals.len() as f64 * 0.01; // Mock calculation
                    
                    if total_return > best_return {
                        best_return = total_return;
                        best_params = (short, long);
                    }
                }
            }
            
            black_box((best_return, best_params))
        })
    });
    
    group.finish();
}

fn bench_backtesting_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("backtesting_simulation");
    group.sample_size(10); // Reduce sample size for expensive operations
    
    let market_data = generate_market_data("AAPL", 1000, 150.0);
    
    // Benchmark full backtesting runs
    for data_points in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("full_backtest", data_points),
            data_points,
            |b, &points| {
                b.iter(|| {
                    let mut strategy = MovingAverageStrategy::new("AAPL".to_string(), 10, 20);
                    let mut portfolio_value = 100000.0;
                    let mut position = 0.0;
                    let mut trade_count = 0;
                    
                    for data in &market_data[0..points] {
                        if let Some(signal) = strategy.update(data) {
                            match signal {
                                StrategySignal::Buy { strength, price, .. } => {
                                    if position <= 0.0 {
                                        let shares_to_buy = (portfolio_value * 0.1 * strength) / price;
                                        position += shares_to_buy;
                                        portfolio_value -= shares_to_buy * price;
                                        trade_count += 1;
                                    }
                                }
                                StrategySignal::Sell { strength, price, .. } => {
                                    if position > 0.0 {
                                        let shares_to_sell = position * strength;
                                        position -= shares_to_sell;
                                        portfolio_value += shares_to_sell * price;
                                        trade_count += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    
                    // Calculate final portfolio value
                    let final_value = portfolio_value + position * market_data[points - 1].close;
                    let total_return = (final_value - 100000.0) / 100000.0;
                    
                    black_box((total_return, trade_count))
                })
            }
        );
    }
    
    group.finish();
}

fn bench_strategy_performance_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("performance_analysis");
    
    // Mock trade history for performance analysis
    let trade_returns: Vec<f64> = (0..1000).map(|i| {
        let base_return = 0.001;
        let volatility = 0.02;
        let random_component = (rand::random::<f64>() - 0.5) * volatility;
        base_return + random_component + (i as f64 * 0.00001).sin() * 0.005
    }).collect();
    
    group.bench_function("calculate_sharpe_ratio", |b| {
        b.iter(|| {
            let mean_return = trade_returns.iter().sum::<f64>() / trade_returns.len() as f64;
            let variance = trade_returns.iter()
                .map(|r| (r - mean_return).powi(2))
                .sum::<f64>() / trade_returns.len() as f64;
            let std_dev = variance.sqrt();
            let risk_free_rate = 0.02 / 252.0;
            
            let sharpe_ratio = (mean_return - risk_free_rate) / std_dev;
            black_box(sharpe_ratio)
        })
    });
    
    group.bench_function("calculate_max_drawdown", |b| {
        b.iter(|| {
            let mut cumulative_returns = Vec::new();
            let mut cumulative = 1.0;
            
            for &ret in &trade_returns {
                cumulative *= 1.0 + ret;
                cumulative_returns.push(cumulative);
            }
            
            let mut max_value = cumulative_returns[0];
            let mut max_drawdown = 0.0;
            
            for &value in &cumulative_returns {
                if value > max_value {
                    max_value = value;
                }
                let drawdown = (max_value - value) / max_value;
                if drawdown > max_drawdown {
                    max_drawdown = drawdown;
                }
            }
            
            black_box(max_drawdown)
        })
    });
    
    group.bench_function("calculate_win_rate", |b| {
        b.iter(|| {
            let winning_trades = trade_returns.iter().filter(|&&r| r > 0.0).count();
            let win_rate = winning_trades as f64 / trade_returns.len() as f64;
            black_box(win_rate)
        })
    });
    
    group.finish();
}

criterion_group!(
    strategy_benches,
    bench_strategy_creation,
    bench_signal_generation,
    bench_strategy_calculations,
    bench_strategy_manager_operations,
    bench_parameter_optimization,
    bench_backtesting_simulation,
    bench_strategy_performance_analysis
);

criterion_main!(strategy_benches);
