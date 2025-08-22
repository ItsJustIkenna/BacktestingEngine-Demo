//! Fitness function for Avellaneda-Stoikov strategy chromosome

use crate::chromosome::AvellanedaStoikovChromosome;
use crate::Chromosome;
use backtest::simulation::SimulationLoop;
use backtest::types::BacktestResult;
use config::BacktestConfig;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use strategy::traits::Strategy;
use std::collections::HashMap;
use strategy::ParameterValue;

/// Converts a chromosome to Avellaneda-Stoikov strategy parameters
fn chromosome_to_params(chromo: &AvellanedaStoikovChromosome) -> AvellanedaStoikovParams {
    AvellanedaStoikovParams {
        risk_aversion: chromo.risk_aversion,
        inventory_target: chromo.inventory_target,
        // Use defaults for other params, or make them configurable as needed
        ..Default::default()
    }
}

/// Fitness function: runs a backtest and returns a weighted score
pub async fn avellaneda_stoikov_fitness(
    chromo: &AvellanedaStoikovChromosome,
    market_data: Vec<dataloader::MarketData>,
    config: &BacktestConfig,
) -> f64 {
    // 1. Build strategy with chromosome params
    let mut strategy = AvellanedaStoikovStrategy::default();
    let params = chromosome_to_params(chromo);
    let mut param_map = HashMap::new();
    param_map.insert("risk_aversion".to_string(), ParameterValue::Float(params.risk_aversion));
    param_map.insert("inventory_target".to_string(), ParameterValue::Float(params.inventory_target));
    // Add more if needed
    let _ = strategy.initialize(param_map).await;

    // 2. Set up other components (mock or real as needed)
    let mut strategy_manager = strategy::StrategyManager::default();
    strategy_manager.add_strategy(Box::new(strategy));
    let mut orderbook = orderbook::OrderBook::default();
    let mut portfolio = portfoliomanager::PortfolioState::default();
    let mut risk_manager = riskmanager::RiskManager::default();
    let mut sim = SimulationLoop::new();

    // 3. Run the backtest
    let result: BacktestResult = sim
        .run(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            market_data,
            config,
        )
        .await;

    // 4. Compute fitness (skewed towards Sharpe ratio and profit factor)
    let sharpe = result.sharpe_ratio.unwrap_or(0.0);
    let profit_factor = result.profit_factor.unwrap_or(0.0);
    let win_rate = result.win_rate.unwrap_or(0.0);
    let max_drawdown = result.max_drawdown.to_f64().unwrap_or(0.0);

    // Weights: Sharpe (0.4), Profit Factor (0.4), Win Rate (0.1), Max Drawdown (0.1, negative)
    let fitness = 0.4 * sharpe + 0.4 * profit_factor + 0.1 * win_rate - 0.1 * max_drawdown;
    fitness
}
