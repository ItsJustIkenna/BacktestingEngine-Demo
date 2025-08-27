//! Orchestrates walk-forward analysis runs

use crate::window::{generate_windows};
use crate::result::{WalkForwardWindowResult, WalkForwardSummary};
use chrono::NaiveDate;
use metrics::PerformanceMetrics;
use dataloader::MarketData;
use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovParams;

/// Main walk-forward analysis runner
pub async fn run_walkforward_analysis(
    data: &[MarketData],
    start: NaiveDate,
    end: NaiveDate,
    train_size: chrono::Duration,
    test_size: chrono::Duration,
    step_size: chrono::Duration,
) -> WalkForwardSummary {
    let windows = generate_windows(start, end, train_size, test_size, step_size);
    let mut window_results = Vec::new();

    for window in windows {
        // 1. Extract training and testing data for this window
        let train_data = extract_data(data, window.train_start, window.train_end);
        let test_data = extract_data(data, window.test_start, window.test_end);

        // 2. Optimize parameters on training set (call your optimizer here)
        let best_params = optimize_parameters(&train_data).await;

        // 3. Run backtest on test set with best parameters
        let test_metrics = run_backtest_with_params(&test_data, &best_params).await;

        // 4. Store results
        window_results.push(WalkForwardWindowResult {
            train_start: window.train_start,
            train_end: window.train_end,
            test_start: window.test_start,
            test_end: window.test_end,
            best_params: serde_json::json!({
                "risk_aversion": best_params.risk_aversion,
                "order_size": best_params.order_size,
                "window_ms": best_params.window_ms,
                "min_quote_lifetime_ms": best_params.min_quote_lifetime_ms,
                "fee_rate": best_params.fee_rate,
                "latency_buffer": best_params.latency_buffer,
                "inventory_target": best_params.inventory_target,
                "volatility_cap": best_params.volatility_cap,
            }),
            test_metrics,
        });
    }

    // 5. Aggregate results (example: average test Sharpe, parameter stability, etc.)
    let average_test_sharpe = if !window_results.is_empty() {
        window_results.iter().map(|r| r.test_metrics.sharpe_ratio).sum::<f64>() / window_results.len() as f64
    } else { 0.0 };
    let parameter_stability = 0.0; // Placeholder: implement your own logic

    WalkForwardSummary {
        window_results,
        average_test_sharpe,
        parameter_stability,
    }
}

// --- Helper stubs to be implemented ---

fn extract_data(data: &[MarketData], start: NaiveDate, end: NaiveDate) -> Vec<MarketData> {
    // Filter data by date range (assumes MarketData has a timestamp field)
    data.iter().cloned().filter(|d| in_range(d, start, end)).collect()
}

fn in_range(d: &MarketData, start: NaiveDate, end: NaiveDate) -> bool {
    // Extract timestamp from MarketData and compare
    let ts = match d {
        #[cfg(feature = "postgres")]
        MarketData::Order(o) => o.get_timestamp().naive_utc().date(),
        #[cfg(feature = "postgres")]
        MarketData::Snapshot(s) => s.get_timestamp().naive_utc().date(),
        MarketData::Candle(c) => c.timestamp.naive_utc().date(),
        MarketData::Trade(t) => t.timestamp.naive_utc().date(),
    };
    ts >= start && ts < end
}


use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::GeneticOptimizer;
use config::GeneticConfig;
use std::sync::Arc;
use std::pin::Pin;
use std::future::Future;

async fn optimize_parameters(_train_data: &[MarketData]) -> AvellanedaStoikovParams {
    // Example config, adjust as needed
    let config = GeneticConfig {
        population_size: 20,
        generations: 10,
        crossover_rate: 0.7,
        mutation_rate: 0.2,
    };
    let fitness_fn = Arc::new(|chromo: &AvellanedaStoikovChromosome| {
        // Simple fitness based on parameter values since full backtesting is complex
        let score = chromo.risk_aversion * 10.0 + chromo.inventory_target * 5.0;
        Box::pin(async move { score }) as Pin<Box<dyn Future<Output = f64> + Send + 'static>>
    });
    let optimizer = GeneticOptimizer {
        config,
        fitness_fn,
    };
    let (best_chromo, _score) = optimizer.run().await;
    // Convert chromosome to params - using all 8 parameters now
    AvellanedaStoikovParams {
        risk_aversion: best_chromo.risk_aversion,
        order_size: best_chromo.order_size,
        window_ms: best_chromo.window_ms,
        min_quote_lifetime_ms: best_chromo.min_quote_lifetime_ms,
        fee_rate: best_chromo.fee_rate,
        latency_buffer: best_chromo.latency_buffer,
        inventory_target: best_chromo.inventory_target,
        volatility_cap: best_chromo.volatility_cap,
    }
}


use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy;
use strategy::Strategy;

async fn run_backtest_with_params(_test_data: &[MarketData], params: &AvellanedaStoikovParams) -> PerformanceMetrics {
    // Build the strategy with the given params
    let mut strategy = AvellanedaStoikovStrategy::default();
    let mut param_map = std::collections::HashMap::new();
    param_map.insert("risk_aversion".to_string(), strategy::ParameterValue::Float(params.risk_aversion));
    param_map.insert("order_size".to_string(), strategy::ParameterValue::Float(params.order_size));
    param_map.insert("inventory_target".to_string(), strategy::ParameterValue::Float(params.inventory_target));
    param_map.insert("fee_rate".to_string(), strategy::ParameterValue::Float(params.fee_rate));
    let _ = strategy.initialize(param_map).await;

    // For now, return a mock result since the full backtest setup is complex
    // In a real implementation, you would run the full simulation here
    PerformanceMetrics {
        sharpe_ratio: 1.2,
        profit_factor: 1.5,
        net_profit: 1000.0,
        gross_profit: 1500.0,
        gross_loss: -500.0,
        average_trade_return: 0.02,
        median_trade_return: 0.01,
        number_of_trades: 100,
        max_drawdown: 0.05,
        volatility: 0.15,
        win_rate: 0.6,
        average_trade_duration: 300.0,
    }
}
