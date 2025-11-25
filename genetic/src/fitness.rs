//! Fitness function for Avellaneda-Stoikov strategy chromosome

use crate::chromosome::AvellanedaStoikovChromosome;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovStrategy, AvellanedaStoikovParams};
use backtest::simulation::SimulationLoop;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use strategy::StrategyManager;

/// Fitness function: runs actual backtest with given chromosome parameters
/// Returns Sharpe ratio as fitness score (higher is better)
pub async fn avellaneda_stoikov_fitness(
    chromo: &AvellanedaStoikovChromosome,
    market_data: Vec<dataloader::MarketData>,
    config: &config::BacktestConfig,
) -> f64 {
    // Convert chromosome to strategy parameters (Phase 2: all 14 parameters)
    let params = AvellanedaStoikovParams {
        risk_aversion: chromo.risk_aversion,
        order_size: chromo.order_size,
        window_ms: chromo.window_ms,
        min_quote_lifetime_ms: chromo.min_quote_lifetime_ms,
        fee_rate: chromo.fee_rate,
        latency_buffer: chromo.latency_buffer,
        inventory_target: chromo.inventory_target,
        volatility_cap: chromo.volatility_cap,
        time_horizon_minutes: chromo.time_horizon_minutes,
        market_depth_k: chromo.market_depth_k,
        
        // Phase 2: Dynamic adjustment parameters
        volatility_adjustment_factor: chromo.volatility_adjustment_factor,
        inventory_penalty_factor: chromo.inventory_penalty_factor,
        trend_sensitivity: chromo.trend_sensitivity,
        flow_sensitivity: chromo.flow_sensitivity,
    };
    
    // Initialize strategy with chromosome parameters
    let strategy = AvellanedaStoikovStrategy::new(params);
    let mut strategy_manager = StrategyManager::new();
    
    // Create a simple strategy config
    let strategy_config = strategy::StrategyConfig {
        id: "avellaneda_stoikov".to_string(),
        name: "Avellaneda-Stoikov Market Making".to_string(),
        strategy_type: "AvellanedaStoikov".to_string(),
        symbol: "BTC/USD".to_string(),
        exchange: "backtest".to_string(),
        parameters: std::collections::HashMap::new(),
        risk_limits: strategy::config::RiskLimits::default(),
        enabled: true,
    };
    
    if let Err(e) = strategy_manager.register_strategy(strategy_config, strategy) {
        eprintln!("Failed to register strategy: {}", e);
        return 0.0; // Return worst fitness on error
    }
    
    // Initialize backtest components
    let orderbook_config = orderbook::OrderBookConfig::default();
    let mut orderbook = OrderBook::new("BTC/USD".to_string(), "backtest".to_string(), orderbook_config);
    
    // Initialize portfolio with default (will be set to initial_capital in simulation)
    let mut portfolio = PortfolioState::default();
    portfolio.balance = bigdecimal::BigDecimal::from(config.trading.initial_capital as i64);
    
    let mut risk_manager = RiskManager::new();
    
    // Run simulation
    let mut sim_loop = SimulationLoop::new();
    let result = sim_loop.run(
        &mut strategy_manager,
        &mut orderbook,
        &mut portfolio,
        &mut risk_manager,
        market_data,
        config,
    ).await;
    
    // Return Sharpe ratio as fitness (or 0.0 if None)
    // Penalize strategies with too few trades
    if result.num_trades < 10 {
        return result.sharpe_ratio.unwrap_or(0.0) * 0.1; // Heavy penalty
    }
    
    // Return Sharpe ratio, but ensure minimum of 0.0
    result.sharpe_ratio.unwrap_or(0.0).max(0.0)
}

