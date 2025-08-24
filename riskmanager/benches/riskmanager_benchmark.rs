//! Benchmarks for the riskmanager module

use riskmanager::{RiskManager, RiskMetrics, RiskLimit, RiskEvent, DrawdownTracker};
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};

// Mock risk metrics for benchmarking
fn create_risk_metrics() -> RiskMetrics {
    RiskMetrics {
        portfolio_value: 1000000.0,
        total_exposure: 800000.0,
        net_exposure: 200000.0,
        gross_exposure: 1000000.0,
        leverage: 1.25,
        var_1_day: 15000.0,
        var_5_day: 35000.0,
        expected_shortfall: 22000.0,
        maximum_drawdown: 0.08,
        current_drawdown: 0.03,
        sharpe_ratio: 1.45,
        sortino_ratio: 1.82,
        beta: 1.15,
        concentration_risk: 0.25,
        sector_concentrations: [
            ("Technology", 0.4),
            ("Healthcare", 0.2),
            ("Financials", 0.15),
            ("Consumer", 0.15),
            ("Energy", 0.1),
        ].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        timestamp: SystemTime::now(),
    }
}

// Mock risk manager for benchmarking
struct BenchRiskManager {
    limits: HashMap<String, RiskLimit>,
    current_metrics: RiskMetrics,
    position_limits: HashMap<String, f64>, // symbol -> max position size
    sector_limits: HashMap<String, f64>,   // sector -> max allocation
    drawdown_tracker: DrawdownTracker,
    risk_events: Vec<RiskEvent>,
}

impl BenchRiskManager {
    fn new() -> Self {
        let limits = [
            ("max_leverage", RiskLimit::Leverage(2.0)),
            ("max_var", RiskLimit::VaR(50000.0)),
            ("max_drawdown", RiskLimit::Drawdown(0.15)),
            ("max_concentration", RiskLimit::Concentration(0.3)),
        ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        
        let position_limits = [
            ("AAPL", 50000.0),
            ("GOOGL", 75000.0),
            ("MSFT", 60000.0),
            ("TSLA", 40000.0),
            ("AMZN", 55000.0),
        ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        
        let sector_limits = [
            ("Technology", 0.5),
            ("Healthcare", 0.3),
            ("Financials", 0.25),
            ("Consumer", 0.2),
            ("Energy", 0.15),
        ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        
        Self {
            limits,
            current_metrics: create_risk_metrics(),
            position_limits,
            sector_limits,
            drawdown_tracker: DrawdownTracker::new(1000000.0),
            risk_events: Vec::new(),
        }
    }
    
    fn check_position_limit(&self, symbol: &str, position_value: f64) -> bool {
        if let Some(&limit) = self.position_limits.get(symbol) {
            position_value <= limit
        } else {
            true
        }
    }
    
    fn check_leverage_limit(&self, leverage: f64) -> bool {
        if let Some(RiskLimit::Leverage(max_leverage)) = self.limits.get("max_leverage") {
            leverage <= *max_leverage
        } else {
            true
        }
    }
    
    fn check_var_limit(&self, var: f64) -> bool {
        if let Some(RiskLimit::VaR(max_var)) = self.limits.get("max_var") {
            var <= *max_var
        } else {
            true
        }
    }
    
    fn check_drawdown_limit(&self, drawdown: f64) -> bool {
        if let Some(RiskLimit::Drawdown(max_drawdown)) = self.limits.get("max_drawdown") {
            drawdown <= *max_drawdown
        } else {
            true
        }
    }
    
    fn check_concentration_limit(&self, concentration: f64) -> bool {
        if let Some(RiskLimit::Concentration(max_concentration)) = self.limits.get("max_concentration") {
            concentration <= *max_concentration
        } else {
            true
        }
    }
    
    fn calculate_var(&self, returns: &[f64], confidence: f64) -> f64 {
        let mut sorted_returns = returns.to_vec();
        sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let index = ((1.0 - confidence) * sorted_returns.len() as f64) as usize;
        sorted_returns[index.min(sorted_returns.len() - 1)] * self.current_metrics.portfolio_value
    }
    
    fn calculate_expected_shortfall(&self, returns: &[f64], confidence: f64) -> f64 {
        let mut sorted_returns = returns.to_vec();
        sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let var_index = ((1.0 - confidence) * sorted_returns.len() as f64) as usize;
        let tail_returns: Vec<f64> = sorted_returns.iter().take(var_index + 1).cloned().collect();
        
        let mean_tail_return = tail_returns.iter().sum::<f64>() / tail_returns.len() as f64;
        mean_tail_return * self.current_metrics.portfolio_value
    }
    
    fn update_metrics(&mut self, portfolio_value: f64, positions: &HashMap<String, f64>) {
        self.current_metrics.portfolio_value = portfolio_value;
        self.current_metrics.total_exposure = positions.values().map(|v| v.abs()).sum();
        self.current_metrics.net_exposure = positions.values().sum();
        self.current_metrics.gross_exposure = positions.values().map(|v| v.abs()).sum();
        self.current_metrics.leverage = self.current_metrics.gross_exposure / portfolio_value;
        
        // Update drawdown
        self.drawdown_tracker.update(portfolio_value);
        self.current_metrics.current_drawdown = self.drawdown_tracker.current_drawdown();
        self.current_metrics.maximum_drawdown = self.drawdown_tracker.maximum_drawdown();
        
        self.current_metrics.timestamp = SystemTime::now();
    }
    
    fn assess_portfolio_risk(&self, positions: &HashMap<String, f64>) -> Vec<RiskEvent> {
        let mut events = Vec::new();
        
        // Check individual position limits
        for (symbol, &value) in positions {
            if !self.check_position_limit(symbol, value.abs()) {
                events.push(RiskEvent::PositionLimitExceeded {
                    symbol: symbol.clone(),
                    current_value: value.abs(),
                    limit: self.position_limits[symbol],
                    timestamp: SystemTime::now(),
                });
            }
        }
        
        // Check leverage limit
        if !self.check_leverage_limit(self.current_metrics.leverage) {
            events.push(RiskEvent::LeverageLimitExceeded {
                current_leverage: self.current_metrics.leverage,
                limit: 2.0,
                timestamp: SystemTime::now(),
            });
        }
        
        // Check VaR limit
        if !self.check_var_limit(self.current_metrics.var_1_day) {
            events.push(RiskEvent::VaRLimitExceeded {
                current_var: self.current_metrics.var_1_day,
                limit: 50000.0,
                timestamp: SystemTime::now(),
            });
        }
        
        // Check drawdown limit
        if !self.check_drawdown_limit(self.current_metrics.current_drawdown) {
            events.push(RiskEvent::DrawdownLimitExceeded {
                current_drawdown: self.current_metrics.current_drawdown,
                limit: 0.15,
                timestamp: SystemTime::now(),
            });
        }
        
        events
    }
}

// Drawdown tracker implementation
struct DrawdownTracker {
    initial_value: f64,
    peak_value: f64,
    current_value: f64,
    max_drawdown: f64,
}

impl DrawdownTracker {
    fn new(initial_value: f64) -> Self {
        Self {
            initial_value,
            peak_value: initial_value,
            current_value: initial_value,
            max_drawdown: 0.0,
        }
    }
    
    fn update(&mut self, value: f64) {
        self.current_value = value;
        
        if value > self.peak_value {
            self.peak_value = value;
        }
        
        let current_drawdown = (self.peak_value - value) / self.peak_value;
        if current_drawdown > self.max_drawdown {
            self.max_drawdown = current_drawdown;
        }
    }
    
    fn current_drawdown(&self) -> f64 {
        (self.peak_value - self.current_value) / self.peak_value
    }
    
    fn maximum_drawdown(&self) -> f64 {
        self.max_drawdown
    }
}

fn bench_risk_manager_creation(c: &mut Criterion) {
    c.bench_function("risk_manager_creation", |b| {
        b.iter(|| {
            black_box(BenchRiskManager::new())
        })
    });
}

fn bench_risk_limit_checks(c: &mut Criterion) {
    let mut group = c.benchmark_group("risk_limit_checks");
    let risk_manager = BenchRiskManager::new();
    
    group.bench_function("position_limit_check", |b| {
        b.iter(|| {
            black_box(risk_manager.check_position_limit("AAPL", 45000.0))
        })
    });
    
    group.bench_function("leverage_limit_check", |b| {
        b.iter(|| {
            black_box(risk_manager.check_leverage_limit(1.8))
        })
    });
    
    group.bench_function("var_limit_check", |b| {
        b.iter(|| {
            black_box(risk_manager.check_var_limit(45000.0))
        })
    });
    
    group.bench_function("drawdown_limit_check", |b| {
        b.iter(|| {
            black_box(risk_manager.check_drawdown_limit(0.12))
        })
    });
    
    group.bench_function("concentration_limit_check", |b| {
        b.iter(|| {
            black_box(risk_manager.check_concentration_limit(0.28))
        })
    });
    
    group.finish();
}

fn bench_var_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("var_calculations");
    let risk_manager = BenchRiskManager::new();
    
    // Generate sample returns data
    let small_returns: Vec<f64> = (0..100).map(|i| (i as f64 * 0.01).sin() * 0.02).collect();
    let large_returns: Vec<f64> = (0..1000).map(|i| (i as f64 * 0.001).sin() * 0.02 + (i as f64 * 0.003).cos() * 0.01).collect();
    
    group.bench_function("var_calculation_small", |b| {
        b.iter(|| {
            black_box(risk_manager.calculate_var(&small_returns, 0.95))
        })
    });
    
    group.bench_function("var_calculation_large", |b| {
        b.iter(|| {
            black_box(risk_manager.calculate_var(&large_returns, 0.95))
        })
    });
    
    group.bench_function("expected_shortfall_small", |b| {
        b.iter(|| {
            black_box(risk_manager.calculate_expected_shortfall(&small_returns, 0.95))
        })
    });
    
    group.bench_function("expected_shortfall_large", |b| {
        b.iter(|| {
            black_box(risk_manager.calculate_expected_shortfall(&large_returns, 0.95))
        })
    });
    
    // Benchmark different confidence levels
    for &confidence in [0.90, 0.95, 0.99].iter() {
        group.bench_with_input(
            BenchmarkId::new("var_confidence_levels", confidence),
            &confidence,
            |b, &conf| {
                b.iter(|| {
                    black_box(risk_manager.calculate_var(&large_returns, conf))
                })
            }
        );
    }
    
    group.finish();
}

fn bench_drawdown_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("drawdown_tracking");
    
    // Generate portfolio value series
    let portfolio_values: Vec<f64> = (0..1000).map(|i| {
        let base_value = 1000000.0;
        let trend = i as f64 * 100.0;
        let volatility = (i as f64 * 0.1).sin() * 50000.0;
        base_value + trend + volatility
    }).collect();
    
    group.bench_function("drawdown_tracker_updates", |b| {
        b.iter(|| {
            let mut tracker = DrawdownTracker::new(1000000.0);
            for &value in &portfolio_values {
                tracker.update(value);
            }
            black_box((tracker.current_drawdown(), tracker.maximum_drawdown()))
        })
    });
    
    group.bench_function("single_drawdown_update", |b| {
        let mut tracker = DrawdownTracker::new(1000000.0);
        b.iter(|| {
            tracker.update(950000.0);
            black_box(tracker.current_drawdown())
        })
    });
    
    group.finish();
}

fn bench_portfolio_risk_assessment(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio_risk_assessment");
    let risk_manager = BenchRiskManager::new();
    
    // Create test portfolios of different sizes
    let small_portfolio: HashMap<String, f64> = [
        ("AAPL", 45000.0),
        ("GOOGL", 70000.0),
        ("MSFT", 55000.0),
    ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    
    let medium_portfolio: HashMap<String, f64> = (0..20).map(|i| {
        (format!("STOCK{:02}", i), 25000.0 + i as f64 * 1000.0)
    }).collect();
    
    let large_portfolio: HashMap<String, f64> = (0..100).map(|i| {
        (format!("STOCK{:03}", i), 10000.0 + i as f64 * 500.0)
    }).collect();
    
    group.bench_function("small_portfolio_assessment", |b| {
        b.iter(|| {
            black_box(risk_manager.assess_portfolio_risk(&small_portfolio))
        })
    });
    
    group.bench_function("medium_portfolio_assessment", |b| {
        b.iter(|| {
            black_box(risk_manager.assess_portfolio_risk(&medium_portfolio))
        })
    });
    
    group.bench_function("large_portfolio_assessment", |b| {
        b.iter(|| {
            black_box(risk_manager.assess_portfolio_risk(&large_portfolio))
        })
    });
    
    group.finish();
}

fn bench_risk_metrics_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("risk_metrics_calculation");
    let mut risk_manager = BenchRiskManager::new();
    
    let positions: HashMap<String, f64> = [
        ("AAPL", 45000.0),
        ("GOOGL", 70000.0),
        ("MSFT", 55000.0),
        ("TSLA", 35000.0),
        ("AMZN", 50000.0),
    ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    
    group.bench_function("update_risk_metrics", |b| {
        b.iter(|| {
            risk_manager.update_metrics(1000000.0, &positions);
            black_box(&risk_manager.current_metrics)
        })
    });
    
    // Benchmark correlation matrix calculation
    let returns_matrix: Vec<Vec<f64>> = (0..10).map(|i| {
        (0..252).map(|j| ((i + j) as f64 * 0.01).sin() * 0.02).collect()
    }).collect();
    
    group.bench_function("correlation_matrix_calculation", |b| {
        b.iter(|| {
            let mut correlation_matrix = vec![vec![0.0; 10]; 10];
            
            for i in 0..10 {
                for j in 0..10 {
                    if i <= j {
                        let correlation = calculate_correlation(&returns_matrix[i], &returns_matrix[j]);
                        correlation_matrix[i][j] = correlation;
                        correlation_matrix[j][i] = correlation;
                    }
                }
            }
            
            black_box(correlation_matrix)
        })
    });
    
    group.finish();
}

fn calculate_correlation(series1: &[f64], series2: &[f64]) -> f64 {
    let n = series1.len() as f64;
    let mean1 = series1.iter().sum::<f64>() / n;
    let mean2 = series2.iter().sum::<f64>() / n;
    
    let numerator: f64 = series1.iter().zip(series2.iter())
        .map(|(&x, &y)| (x - mean1) * (y - mean2))
        .sum();
    
    let var1: f64 = series1.iter().map(|&x| (x - mean1).powi(2)).sum();
    let var2: f64 = series2.iter().map(|&x| (x - mean2).powi(2)).sum();
    
    numerator / (var1 * var2).sqrt()
}

fn bench_stress_testing(c: &mut Criterion) {
    let mut group = c.benchmark_group("stress_testing");
    let risk_manager = BenchRiskManager::new();
    
    // Define stress scenarios
    let stress_scenarios = vec![
        ("Market Crash", vec![-0.20, -0.15, -0.25, -0.18, -0.22]),
        ("Interest Rate Spike", vec![-0.05, -0.03, -0.08, -0.04, -0.06]),
        ("Sector Rotation", vec![-0.10, 0.05, -0.12, 0.08, -0.15]),
        ("Liquidity Crisis", vec![-0.25, -0.30, -0.20, -0.28, -0.32]),
    ];
    
    let base_positions: HashMap<String, f64> = [
        ("AAPL", 100000.0),
        ("GOOGL", 150000.0),
        ("MSFT", 120000.0),
        ("TSLA", 80000.0),
        ("AMZN", 110000.0),
    ].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    
    group.bench_function("stress_test_scenarios", |b| {
        b.iter(|| {
            let mut results = HashMap::new();
            
            for (scenario_name, shocks) in &stress_scenarios {
                let stressed_positions: HashMap<String, f64> = base_positions.iter()
                    .zip(shocks.iter())
                    .map(|((symbol, &value), &shock)| {
                        (symbol.clone(), value * (1.0 + shock))
                    })
                    .collect();
                
                let total_pnl: f64 = stressed_positions.values().sum::<f64>() - base_positions.values().sum::<f64>();
                let risk_events = risk_manager.assess_portfolio_risk(&stressed_positions);
                
                results.insert(scenario_name.to_string(), (total_pnl, risk_events.len()));
            }
            
            black_box(results)
        })
    });
    
    group.finish();
}

fn bench_monte_carlo_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("monte_carlo_simulation");
    group.sample_size(10); // Reduce sample size for expensive operations
    
    // Mock Monte Carlo simulation for risk assessment
    let portfolio_weights = vec![0.2, 0.25, 0.2, 0.15, 0.2];
    let expected_returns = vec![0.08, 0.12, 0.10, 0.15, 0.11];
    let volatilities = vec![0.16, 0.22, 0.18, 0.30, 0.20];
    
    for &num_simulations in [1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("monte_carlo_risk", num_simulations),
            &num_simulations,
            |b, &sims| {
                b.iter(|| {
                    let mut portfolio_returns = Vec::with_capacity(sims);
                    
                    for _ in 0..sims {
                        let mut portfolio_return = 0.0;
                        
                        for i in 0..portfolio_weights.len() {
                            // Simplified random return generation
                            let random_factor = (rand::random::<f64>() - 0.5) * 2.0;
                            let asset_return = expected_returns[i] + volatilities[i] * random_factor;
                            portfolio_return += portfolio_weights[i] * asset_return;
                        }
                        
                        portfolio_returns.push(portfolio_return);
                    }
                    
                    // Calculate VaR from simulation results
                    portfolio_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let var_95 = portfolio_returns[(sims as f64 * 0.05) as usize];
                    
                    black_box(var_95)
                })
            }
        );
    }
    
    group.finish();
}

criterion_group!(
    riskmanager_benches,
    bench_risk_manager_creation,
    bench_risk_limit_checks,
    bench_var_calculations,
    bench_drawdown_tracking,
    bench_portfolio_risk_assessment,
    bench_risk_metrics_calculation,
    bench_stress_testing,
    bench_monte_carlo_simulation
);

criterion_main!(riskmanager_benches);
