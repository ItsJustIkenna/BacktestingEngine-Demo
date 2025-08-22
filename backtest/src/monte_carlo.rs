//! Monte Carlo simulation module for backtesting

use crate::dataloader::{DataLoader, MarketData};
use crate::config::MonteCarloConfig;
use crate::backtest::BacktestEngine;
use crate::metrics::PerformanceMetrics;

pub struct MonteCarloResult {
    pub all_metrics: Vec<PerformanceMetrics>,
    pub mean_metrics: PerformanceMetrics,
    pub stddev_metrics: PerformanceMetrics,
    pub percentile_metrics: Vec<(f64, PerformanceMetrics)>, // (percentile, metrics)
}

/// Helper functions for aggregation
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn stddev(values: &[f64], mean: f64) -> f64 {
    if values.len() < 2 {
        0.0
    } else {
        (values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / (values.len() as f64 - 1.0))
        .sqrt()
    }
}

fn percentile(values: &mut [f64], pct: f64) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((pct / 100.0) * (values.len() as f64 - 1.0)).round() as usize;
    values[idx]
}

/// Aggregate a vector of PerformanceMetrics
fn aggregate_metrics(
    metrics: &[PerformanceMetrics],
) -> (PerformanceMetrics, PerformanceMetrics, Vec<(f64, PerformanceMetrics)>) {
    let n = metrics.len();
    if n == 0 {
        // Return default/empty metrics
        return (PerformanceMetrics::default(), PerformanceMetrics::default(), vec![]);
    }
    macro_rules! collect {
        ($field:ident) => { metrics.iter().map(|m| m.$field).collect::<Vec<_>>() };
    }
    // Collect all fields
    let mut sharpe_ratios = collect!(sharpe_ratio);
    let mut profit_factors = collect!(profit_factor);
    let mut net_profits = collect!(net_profit);
    let mut gross_profits = collect!(gross_profit);
    let mut gross_losses = collect!(gross_loss);
    let mut avg_trade_returns = collect!(average_trade_return);
    let mut median_trade_returns = collect!(median_trade_return);
    let mut num_trades = collect!(number_of_trades).into_iter().map(|v| v as f64).collect::<Vec<_>>();
    let mut max_drawdowns = collect!(max_drawdown);
    let mut volatilities = collect!(volatility);
    let mut win_rates = collect!(win_rate);
    let mut avg_trade_durations = collect!(average_trade_duration);
    let percentiles = [5.0, 50.0, 95.0];
    // Mean
    let mut mean_metrics = PerformanceMetrics::default();
    mean_metrics.sharpe_ratio = mean(&sharpe_ratios);
    mean_metrics.profit_factor = mean(&profit_factors);
    mean_metrics.net_profit = mean(&net_profits);
    mean_metrics.gross_profit = mean(&gross_profits);
    mean_metrics.gross_loss = mean(&gross_losses);
    mean_metrics.average_trade_return = mean(&avg_trade_returns);
    mean_metrics.median_trade_return = mean(&median_trade_returns);
    mean_metrics.number_of_trades = mean(&num_trades).round() as usize;
    mean_metrics.max_drawdown = mean(&max_drawdowns);
    mean_metrics.volatility = mean(&volatilities);
    mean_metrics.win_rate = mean(&win_rates);
    mean_metrics.average_trade_duration = mean(&avg_trade_durations);
    // Stddev
    let mut stddev_metrics = PerformanceMetrics::default();
    stddev_metrics.sharpe_ratio = stddev(&sharpe_ratios, mean_metrics.sharpe_ratio);
    stddev_metrics.profit_factor = stddev(&profit_factors, mean_metrics.profit_factor);
    stddev_metrics.net_profit = stddev(&net_profits, mean_metrics.net_profit);
    stddev_metrics.gross_profit = stddev(&gross_profits, mean_metrics.gross_profit);
    stddev_metrics.gross_loss = stddev(&gross_losses, mean_metrics.gross_loss);
    stddev_metrics.average_trade_return = stddev(&avg_trade_returns, mean_metrics.average_trade_return);
    stddev_metrics.median_trade_return = stddev(&median_trade_returns, mean_metrics.median_trade_return);
    stddev_metrics.number_of_trades = stddev(&num_trades, mean_metrics.number_of_trades as f64).round() as usize;
    stddev_metrics.max_drawdown = stddev(&max_drawdowns, mean_metrics.max_drawdown);
    stddev_metrics.volatility = stddev(&volatilities, mean_metrics.volatility);
    stddev_metrics.win_rate = stddev(&win_rates, mean_metrics.win_rate);
    stddev_metrics.average_trade_duration = stddev(&avg_trade_durations, mean_metrics.average_trade_duration);
    // Percentiles
    let percentile_metrics = percentiles.iter().map(|&pct| {
        let mut m = PerformanceMetrics::default();
        m.sharpe_ratio = percentile(&mut sharpe_ratios.clone(), pct);
        m.profit_factor = percentile(&mut profit_factors.clone(), pct);
        m.net_profit = percentile(&mut net_profits.clone(), pct);
        m.gross_profit = percentile(&mut gross_profits.clone(), pct);
        m.gross_loss = percentile(&mut gross_losses.clone(), pct);
        m.average_trade_return = percentile(&mut avg_trade_returns.clone(), pct);
        m.median_trade_return = percentile(&mut median_trade_returns.clone(), pct);
        m.number_of_trades = percentile(&mut num_trades.clone(), pct).round() as usize;
        m.max_drawdown = percentile(&mut max_drawdowns.clone(), pct);
        m.volatility = percentile(&mut volatilities.clone(), pct);
        m.win_rate = percentile(&mut win_rates.clone(), pct);
        m.average_trade_duration = percentile(&mut avg_trade_durations.clone(), pct);
        (pct, m)
    }).collect();
    (mean_metrics, stddev_metrics, percentile_metrics)
}

/// Run Monte Carlo simulations using the dataloader's built-in simulation logic
pub async fn run_monte_carlo(
    config: &MonteCarloConfig,
    engine_template: &BacktestEngine,
) -> MonteCarloResult {
    // Use the dataloader to generate simulated scenarios
    let simulated_scenarios = config
        .dataloader
        .monte_carlo_simulation(&config.query_params, &config.mc_params)
        .await
        .expect("Failed to generate Monte Carlo scenarios");

    let mut all_metrics = Vec::with_capacity(simulated_scenarios.len());

    for scenario in simulated_scenarios {
        // Convert scenario (Vec<HistoricalOrder>) to Vec<MarketData> as needed
        // (Assume a conversion function exists or implement as needed)
        let market_data: Vec<MarketData> = scenario
            .into_iter()
            .map(MarketData::from_historical_order)
            .collect();

        let mut engine = engine_template.clone();
        let report = engine.run(market_data).await;
        let metrics = PerformanceMetrics::from_report(&report);
        all_metrics.push(metrics);
    }

    let (mean_metrics, stddev_metrics, percentile_metrics) = aggregate_metrics(&all_metrics);

    MonteCarloResult {
        all_metrics,
        mean_metrics,
        stddev_metrics,
        percentile_metrics,
    }
}
