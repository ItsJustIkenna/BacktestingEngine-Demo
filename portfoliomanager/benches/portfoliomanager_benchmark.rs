//! Benchmarks for the portfoliomanager module

use portfoliomanager::{PortfolioManager, Position, PositionType, PortfolioState};
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::runtime::Runtime;

// Mock position for benchmarking
fn create_test_position(symbol: &str, quantity: f64, avg_price: f64) -> Position {
    Position {
        symbol: symbol.to_string(),
        position_type: if quantity > 0.0 { PositionType::Long } else { PositionType::Short },
        quantity: quantity.abs(),
        average_price: avg_price,
        market_price: avg_price * 1.02,
        unrealized_pnl: quantity * avg_price * 0.02,
        realized_pnl: 0.0,
        timestamp: SystemTime::now(),
    }
}

// Mock portfolio state for benchmarking
fn create_test_portfolio_state() -> PortfolioState {
    let mut positions = HashMap::new();
    let symbols = ["AAPL", "GOOGL", "MSFT", "TSLA", "AMZN"];
    
    for (i, symbol) in symbols.iter().enumerate() {
        positions.insert(
            symbol.to_string(),
            create_test_position(symbol, 100.0 + i as f64 * 50.0, 150.0 + i as f64 * 20.0)
        );
    }
    
    PortfolioState {
        positions,
        cash_balance: 50000.0,
        total_equity: 100000.0,
        unrealized_pnl: 2500.0,
        realized_pnl: 1200.0,
        timestamp: SystemTime::now(),
    }
}

// Mock portfolio manager for benchmarking
struct BenchPortfolioManager {
    state: PortfolioState,
    transaction_history: Vec<(String, f64, f64, SystemTime)>, // symbol, quantity, price, timestamp
}

impl BenchPortfolioManager {
    fn new() -> Self {
        Self {
            state: create_test_portfolio_state(),
            transaction_history: Vec::new(),
        }
    }
    
    fn get_position(&self, symbol: &str) -> Option<&Position> {
        self.state.positions.get(symbol)
    }
    
    fn add_position(&mut self, symbol: &str, quantity: f64, price: f64) {
        let position = create_test_position(symbol, quantity, price);
        self.state.positions.insert(symbol.to_string(), position);
        self.transaction_history.push((symbol.to_string(), quantity, price, SystemTime::now()));
        self.update_portfolio_metrics();
    }
    
    fn update_position(&mut self, symbol: &str, quantity_change: f64, price: f64) {
        if let Some(position) = self.state.positions.get_mut(symbol) {
            let new_quantity = if position.position_type == PositionType::Long {
                position.quantity + quantity_change
            } else {
                position.quantity - quantity_change
            };
            
            if new_quantity <= 0.0 {
                self.state.positions.remove(symbol);
            } else {
                // Recalculate average price
                let total_cost = position.average_price * position.quantity + price * quantity_change.abs();
                position.average_price = total_cost / new_quantity;
                position.quantity = new_quantity;
            }
        }
        self.transaction_history.push((symbol.to_string(), quantity_change, price, SystemTime::now()));
        self.update_portfolio_metrics();
    }
    
    fn calculate_portfolio_value(&self, market_prices: &HashMap<String, f64>) -> f64 {
        let mut total_value = self.state.cash_balance;
        
        for (symbol, position) in &self.state.positions {
            if let Some(market_price) = market_prices.get(symbol) {
                let position_value = position.quantity * market_price;
                total_value += if position.position_type == PositionType::Long {
                    position_value
                } else {
                    -position_value
                };
            }
        }
        
        total_value
    }
    
    fn calculate_unrealized_pnl(&self, market_prices: &HashMap<String, f64>) -> f64 {
        let mut total_pnl = 0.0;
        
        for (symbol, position) in &self.state.positions {
            if let Some(market_price) = market_prices.get(symbol) {
                let pnl = (market_price - position.average_price) * position.quantity;
                total_pnl += if position.position_type == PositionType::Long {
                    pnl
                } else {
                    -pnl
                };
            }
        }
        
        total_pnl
    }
    
    fn update_portfolio_metrics(&mut self) {
        // Update cash balance, total equity, etc.
        let market_prices: HashMap<String, f64> = self.state.positions
            .iter()
            .map(|(symbol, pos)| (symbol.clone(), pos.market_price))
            .collect();
        
        self.state.total_equity = self.calculate_portfolio_value(&market_prices);
        self.state.unrealized_pnl = self.calculate_unrealized_pnl(&market_prices);
        self.state.timestamp = SystemTime::now();
    }
    
    fn get_portfolio_statistics(&self) -> HashMap<String, f64> {
        let mut stats = HashMap::new();
        stats.insert("total_equity".to_string(), self.state.total_equity);
        stats.insert("cash_balance".to_string(), self.state.cash_balance);
        stats.insert("unrealized_pnl".to_string(), self.state.unrealized_pnl);
        stats.insert("realized_pnl".to_string(), self.state.realized_pnl);
        stats.insert("position_count".to_string(), self.state.positions.len() as f64);
        
        // Calculate additional metrics
        let total_position_value: f64 = self.state.positions.values()
            .map(|pos| pos.quantity * pos.market_price)
            .sum();
        stats.insert("total_position_value".to_string(), total_position_value);
        
        if total_position_value > 0.0 {
            stats.insert("leverage".to_string(), total_position_value / self.state.total_equity);
        }
        
        stats
    }
}

fn bench_portfolio_creation(c: &mut Criterion) {
    c.bench_function("portfolio_manager_creation", |b| {
        b.iter(|| {
            black_box(BenchPortfolioManager::new())
        })
    });
    
    c.bench_function("portfolio_state_creation", |b| {
        b.iter(|| {
            black_box(create_test_portfolio_state())
        })
    });
}

fn bench_position_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("position_operations");
    let mut portfolio = BenchPortfolioManager::new();
    
    group.bench_function("get_position", |b| {
        b.iter(|| {
            black_box(portfolio.get_position("AAPL"))
        })
    });
    
    group.bench_function("add_position", |b| {
        let mut test_portfolio = BenchPortfolioManager::new();
        let mut counter = 0;
        b.iter(|| {
            counter += 1;
            let symbol = format!("TEST{}", counter);
            test_portfolio.add_position(&symbol, 100.0, 150.0);
            black_box(&test_portfolio.state)
        })
    });
    
    group.bench_function("update_position", |b| {
        let mut test_portfolio = BenchPortfolioManager::new();
        b.iter(|| {
            test_portfolio.update_position("AAPL", 10.0, 155.0);
            black_box(&test_portfolio.state)
        })
    });
    
    group.finish();
}

fn bench_portfolio_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio_calculations");
    let portfolio = BenchPortfolioManager::new();
    
    // Create market prices for testing
    let market_prices: HashMap<String, f64> = [
        ("AAPL", 175.0),
        ("GOOGL", 2800.0),
        ("MSFT", 350.0),
        ("TSLA", 250.0),
        ("AMZN", 3300.0),
    ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    
    group.bench_function("calculate_portfolio_value", |b| {
        b.iter(|| {
            black_box(portfolio.calculate_portfolio_value(&market_prices))
        })
    });
    
    group.bench_function("calculate_unrealized_pnl", |b| {
        b.iter(|| {
            black_box(portfolio.calculate_unrealized_pnl(&market_prices))
        })
    });
    
    group.bench_function("get_portfolio_statistics", |b| {
        b.iter(|| {
            black_box(portfolio.get_portfolio_statistics())
        })
    });
    
    group.finish();
}

fn bench_portfolio_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio_scaling");
    
    for position_count in [10, 50, 100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("large_portfolio_value_calculation", position_count),
            position_count,
            |b, &count| {
                // Create portfolio with many positions
                let mut portfolio = BenchPortfolioManager::new();
                let mut market_prices = HashMap::new();
                
                for i in 0..count {
                    let symbol = format!("STOCK{:04}", i);
                    portfolio.add_position(&symbol, 100.0, 100.0 + i as f64 * 0.1);
                    market_prices.insert(symbol, 105.0 + i as f64 * 0.1);
                }
                
                b.iter(|| {
                    black_box(portfolio.calculate_portfolio_value(&market_prices))
                })
            }
        );
    }
    
    group.finish();
}

fn bench_transaction_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction_processing");
    
    // Benchmark batch transaction processing
    for batch_size in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("batch_transactions", batch_size),
            batch_size,
            |b, &batch_size| {
                b.iter(|| {
                    let mut portfolio = BenchPortfolioManager::new();
                    
                    for i in 0..batch_size {
                        let symbol = format!("STOCK{:03}", i % 10);
                        let quantity = if i % 2 == 0 { 50.0 } else { -25.0 };
                        let price = 100.0 + (i as f64 * 0.5) % 50.0;
                        
                        if portfolio.get_position(&symbol).is_some() {
                            portfolio.update_position(&symbol, quantity, price);
                        } else {
                            portfolio.add_position(&symbol, quantity.abs(), price);
                        }
                    }
                    
                    black_box(portfolio.get_portfolio_statistics())
                })
            }
        );
    }
    
    group.finish();
}

fn bench_portfolio_rebalancing(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio_rebalancing");
    
    // Simulate portfolio rebalancing operations
    group.bench_function("rebalance_portfolio", |b| {
        b.iter(|| {
            let mut portfolio = BenchPortfolioManager::new();
            
            // Target allocation percentages
            let target_allocations: HashMap<String, f64> = [
                ("AAPL", 0.25),
                ("GOOGL", 0.25),
                ("MSFT", 0.20),
                ("TSLA", 0.15),
                ("AMZN", 0.15),
            ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            
            let current_market_prices: HashMap<String, f64> = [
                ("AAPL", 175.0),
                ("GOOGL", 2800.0),
                ("MSFT", 350.0),
                ("TSLA", 250.0),
                ("AMZN", 3300.0),
            ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            
            let total_value = portfolio.calculate_portfolio_value(&current_market_prices);
            
            // Calculate rebalancing trades
            for (symbol, target_pct) in &target_allocations {
                let target_value = total_value * target_pct;
                let current_position = portfolio.get_position(symbol);
                
                let current_value = if let Some(pos) = current_position {
                    pos.quantity * current_market_prices.get(symbol).unwrap_or(&0.0)
                } else {
                    0.0
                };
                
                let value_diff = target_value - current_value;
                let price = current_market_prices.get(symbol).unwrap_or(&100.0);
                let quantity_diff = value_diff / price;
                
                if quantity_diff.abs() > 0.01 {
                    if current_position.is_some() {
                        portfolio.update_position(symbol, quantity_diff, *price);
                    } else {
                        portfolio.add_position(symbol, quantity_diff.abs(), *price);
                    }
                }
            }
            
            black_box(portfolio.get_portfolio_statistics())
        })
    });
    
    group.finish();
}

fn bench_risk_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("risk_calculations");
    let portfolio = BenchPortfolioManager::new();
    
    group.bench_function("position_concentration_risk", |b| {
        b.iter(|| {
            let total_value = portfolio.state.total_equity;
            let mut concentrations = HashMap::new();
            
            for (symbol, position) in &portfolio.state.positions {
                let position_value = position.quantity * position.market_price;
                let concentration = position_value / total_value;
                concentrations.insert(symbol.clone(), concentration);
            }
            
            // Find maximum concentration
            let max_concentration = concentrations.values().fold(0.0f64, |acc, &x| acc.max(x));
            
            black_box((concentrations, max_concentration))
        })
    });
    
    group.bench_function("sector_allocation_analysis", |b| {
        b.iter(|| {
            // Simulate sector mapping
            let sector_mapping: HashMap<String, String> = [
                ("AAPL", "Technology"),
                ("GOOGL", "Technology"),
                ("MSFT", "Technology"),
                ("TSLA", "Automotive"),
                ("AMZN", "Consumer Discretionary"),
            ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            
            let mut sector_allocations: HashMap<String, f64> = HashMap::new();
            let total_value = portfolio.state.total_equity;
            
            for (symbol, position) in &portfolio.state.positions {
                if let Some(sector) = sector_mapping.get(symbol) {
                    let position_value = position.quantity * position.market_price;
                    let allocation = position_value / total_value;
                    *sector_allocations.entry(sector.clone()).or_insert(0.0) += allocation;
                }
            }
            
            black_box(sector_allocations)
        })
    });
    
    group.finish();
}

fn bench_portfolio_performance_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("performance_metrics");
    
    // Create historical portfolio values for performance calculation
    let historical_values: Vec<(SystemTime, f64)> = (0..252) // One year of daily values
        .map(|i| {
            let timestamp = UNIX_EPOCH + Duration::from_secs(i * 24 * 3600);
            let value = 100000.0 * (1.0 + (i as f64 * 0.001).sin() * 0.1 + i as f64 * 0.0002);
            (timestamp, value)
        })
        .collect();
    
    group.bench_function("calculate_returns", |b| {
        b.iter(|| {
            let daily_returns: Vec<f64> = historical_values.windows(2)
                .map(|window| {
                    let prev_value = window[0].1;
                    let curr_value = window[1].1;
                    (curr_value - prev_value) / prev_value
                })
                .collect();
            
            black_box(daily_returns)
        })
    });
    
    group.bench_function("calculate_sharpe_ratio", |b| {
        b.iter(|| {
            let daily_returns: Vec<f64> = historical_values.windows(2)
                .map(|window| {
                    let prev_value = window[0].1;
                    let curr_value = window[1].1;
                    (curr_value - prev_value) / prev_value
                })
                .collect();
            
            let mean_return = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
            let variance = daily_returns.iter()
                .map(|&r| (r - mean_return).powi(2))
                .sum::<f64>() / daily_returns.len() as f64;
            let std_dev = variance.sqrt();
            
            let risk_free_rate = 0.02 / 252.0; // 2% annual risk-free rate
            let sharpe_ratio = (mean_return - risk_free_rate) / std_dev;
            
            black_box(sharpe_ratio)
        })
    });
    
    group.bench_function("calculate_maximum_drawdown", |b| {
        b.iter(|| {
            let mut max_value = historical_values[0].1;
            let mut max_drawdown = 0.0;
            
            for &(_, value) in &historical_values {
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
    
    group.finish();
}

criterion_group!(
    portfoliomanager_benches,
    bench_portfolio_creation,
    bench_position_operations,
    bench_portfolio_calculations,
    bench_portfolio_scaling,
    bench_transaction_processing,
    bench_portfolio_rebalancing,
    bench_risk_calculations,
    bench_portfolio_performance_metrics
);

criterion_main!(portfoliomanager_benches);
