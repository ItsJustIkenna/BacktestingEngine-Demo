use criterion::{criterion_group, criterion_main, Criterion};
use strategy::{Strategy, StrategyManager, Signal, SignalType, SignalStrength, SignalReason, StrategyContext, StrategyResult, StrategyMetrics};
use config::ParameterValue;
use dataloader::MarketData;
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};
use std::collections::HashMap;
use async_trait::async_trait;

// Simple test strategy for benchmarking
#[derive(Debug)]
struct BenchStrategy {
    name: String,
    parameters: HashMap<String, ParameterValue>,
}

impl BenchStrategy {
    fn new() -> Self {
        Self {
            name: "BenchStrategy".to_string(),
            parameters: HashMap::new(),
        }
    }
}

#[async_trait]
impl Strategy for BenchStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    async fn initialize(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        self.parameters = parameters;
        Ok(())
    }

    async fn generate_signals(
        &mut self,
        _market_data: &MarketData,
        context: &StrategyContext,
    ) -> StrategyResult<Vec<Signal>> {
        let signal = Signal {
            id: "test_signal_1".to_string(),
            timestamp: context.timestamp,
            signal_type: SignalType::Buy,
            strength: SignalStrength::Medium,
            price: Some(BigDecimal::from_f64(50000.0).unwrap_or_default()),
            quantity: Some(BigDecimal::from_f64(0.1).unwrap_or_default()),
            reason: SignalReason::TechnicalAnalysis("benchmark test".to_string()),
            metadata: HashMap::new(),
        };
        Ok(vec![signal])
    }

    fn get_metrics(&self) -> Option<StrategyMetrics> {
        Some(StrategyMetrics {
            total_signals: 1,
            buy_signals: 1,
            sell_signals: 0,
            avg_signal_strength: 0.65,
            uptime_percentage: 100.0,
            last_signal_time: Some(Utc::now()),
            custom_metrics: HashMap::new(),
        })
    }

    fn get_parameters(&self) -> HashMap<String, ParameterValue> {
        self.parameters.clone()
    }

    async fn update_parameters(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        self.parameters = parameters;
        Ok(())
    }

    fn get_risk_limits(&self) -> riskmanager::RiskLimits {
        riskmanager::RiskLimits::default()
    }
}

fn bench_strategy_creation(c: &mut Criterion) {
    c.bench_function("create_bench_strategy", |b| {
        b.iter(|| {
            BenchStrategy::new()
        })
    });
}

fn bench_strategy_manager_creation(c: &mut Criterion) {
    c.bench_function("create_strategy_manager", |b| {
        b.iter(|| {
            StrategyManager::new()
        })
    });
}

fn bench_strategy_metrics(c: &mut Criterion) {
    c.bench_function("get_strategy_metrics", |b| {
        b.iter(|| {
            let strategy = BenchStrategy::new();
            let _ = strategy.get_metrics();
        })
    });
}

criterion_group!(benches, bench_strategy_creation, bench_strategy_manager_creation, bench_strategy_metrics);
criterion_main!(benches);
