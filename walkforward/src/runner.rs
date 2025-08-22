//! Orchestrates walk-forward analysis runs

use crate::window::{WalkForwardWindow, generate_windows};
use crate::result::{WalkForwardWindowResult, WalkForwardSummary};
use chrono::{NaiveDate, DateTime, Utc};
use metrics::PerformanceMetrics;
use dataloader::MarketData;
use serde::{Serialize, Deserialize};
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
            best_params: serde_json::to_value(&best_params).unwrap_or_default(),
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
        MarketData::Order(o) => o.get_timestamp().naive_utc(),
        MarketData::Snapshot(s) => s.get_timestamp().naive_utc(),
        MarketData::Candle(c) => c.timestamp.naive_utc(),
        MarketData::Trade(t) => t.timestamp.naive_utc(),
    };
    ts >= start && ts < end
}


use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::{GeneticConfig, GeneticOptimizer};
use genetic::fitness::avellaneda_stoikov_fitness;
use std::sync::Arc;
use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovParams;
use config::BacktestConfig;

async fn optimize_parameters(train_data: &[MarketData]) -> AvellanedaStoikovParams {
    // Example config, adjust as needed
    let config = GeneticConfig {
        population_size: 20,
        generations: 10,
        crossover_rate: 0.7,
        mutation_rate: 0.2,
    };
    let train_data_vec = train_data.to_vec();
    let backtest_config = BacktestConfig::default();
    let fitness_fn = Arc::new(move |chromo: &AvellanedaStoikovChromosome| {
        let data = train_data_vec.clone();
        let config = backtest_config.clone();
        Box::pin(avellaneda_stoikov_fitness(chromo, data, &config))
    });
    let optimizer = GeneticOptimizer {
        config,
        fitness_fn,
    };
    let (best_chromo, _score) = optimizer.run().await;
    // Convert chromosome to params
    AvellanedaStoikovParams {
        risk_aversion: best_chromo.risk_aversion,
        inventory_target: best_chromo.inventory_target,
        quote_width: best_chromo.quote_width,
        ..Default::default()
    }
}


use backtest::BacktestEngine;
use backtest::simulation::SimulationLoop;
use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy;
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use metrics::PerformanceMetrics;

async fn run_backtest_with_params(test_data: &[MarketData], params: &AvellanedaStoikovParams) -> PerformanceMetrics {
    // Build the strategy with the given params
    let mut strategy = AvellanedaStoikovStrategy::default();
    let mut param_map = std::collections::HashMap::new();
    param_map.insert("risk_aversion".to_string(), strategy::ParameterValue::Float(params.risk_aversion));
    param_map.insert("inventory_target".to_string(), strategy::ParameterValue::Float(params.inventory_target));
    param_map.insert("quote_width".to_string(), strategy::ParameterValue::Float(params.quote_width));
    let _ = strategy.initialize(param_map).await;

    let mut strategy_manager = StrategyManager::default();
    strategy_manager.add_strategy(Box::new(strategy));
    let mut orderbook = OrderBook::default();
    let mut portfolio = PortfolioState::default();
    let mut risk_manager = RiskManager::default();
    let mut sim = SimulationLoop::new();

    // Run the backtest
    let result = sim
        .run(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            test_data.to_vec(),
            &config::BacktestConfig::default(),
        )
        .await;

    // Convert BacktestResult to PerformanceMetrics
    PerformanceMetrics {
        sharpe_ratio: result.sharpe_ratio.unwrap_or(0.0),
        profit_factor: result.profit_factor.unwrap_or(0.0),
        net_profit: result.net_profit.unwrap_or(0.0),
        gross_profit: result.gross_profit.unwrap_or(0.0),
        gross_loss: result.gross_loss.unwrap_or(0.0),
        average_trade_return: result.avg_trade_return.unwrap_or(0.0),
        median_trade_return: result.median_trade_return.unwrap_or(0.0),
        number_of_trades: result.num_trades,
        max_drawdown: result.max_drawdown.unwrap_or(0.0),
        volatility: result.volatility.unwrap_or(0.0),
        win_rate: result.win_rate.unwrap_or(0.0),
        average_trade_duration: result.avg_trade_duration.unwrap_or(0.0),
    }
}
