//! Fitness function for Avellaneda-Stoikov strategy chromosome

use crate::chromosome::AvellanedaStoikovChromosome;
use crate::GENETIC_LOGGER;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovStrategy, AvellanedaStoikovParams};
use backtest::simulation::SimulationLoop;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use strategy::StrategyManager;
use bigdecimal::{ToPrimitive, FromPrimitive};

/// Fitness function: runs actual backtest with given chromosome parameters
/// Returns a composite fitness score using weighted combination:
/// - Sharpe ratio (0.5 weight)
/// - Profit Factor (0.3 weight)  
/// - Win Rate (0.2 weight)
/// With penalties for:
/// - Excessive drawdown (>20%)
/// - High commission ratio (>50% of gross profit)
/// - Too few trades (<10)
pub async fn avellaneda_stoikov_fitness(
    chromo: &AvellanedaStoikovChromosome,
    market_data: Vec<dataloader::MarketData>,
    config: &config::BacktestConfig,
) -> f64 {
    // Get exchange maker fee from config or use default
    let exchange = "kraken"; // Default exchange for backtesting
    let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
    let maker_fee = exchange_fee_config.maker_fee;
    
    // Convert chromosome to strategy parameters (Phase 2: all 14 parameters)
    let params = AvellanedaStoikovParams {
        risk_aversion: chromo.risk_aversion,
        order_size: chromo.order_size,
        order_size_pct: chromo.order_size_pct,                   // From chromosome (0.5-5%)
        initial_capital: config.trading.initial_capital,         // Use config's capital
        initial_inventory_pct: 0.5,                              // 50% inventory split
        window_ms: chromo.window_ms,
        min_quote_lifetime_ms: chromo.min_quote_lifetime_ms,
        fee_rate: maker_fee,  // Use actual exchange maker fee, not chromosome
        inventory_target: chromo.inventory_target,
        volatility_cap: chromo.volatility_cap,
        volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
        time_horizon_minutes: chromo.time_horizon_minutes,
        market_depth_k: chromo.market_depth_k,
        min_spread_bps: chromo.min_spread_bps,
        max_trades_per_minute: chromo.max_trades_per_minute,
        take_profit_pct: chromo.take_profit_pct,
        stop_loss_pct: chromo.stop_loss_pct,
        
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
        GENETIC_LOGGER.error_sync(format!("Failed to register strategy: {}", e));
        return 0.0; // Return worst fitness on error
    }
    
    // Initialize backtest components
    let orderbook_config = orderbook::OrderBookConfig::default();
    let mut orderbook = OrderBook::new("BTC/USD".to_string(), "backtest".to_string(), orderbook_config);
    
    // Get initial price from market data for inventory calculation
    let initial_price = market_data.first()
        .and_then(|d| match d {
            dataloader::MarketData::Trade(t) => t.price.to_f64(),
            dataloader::MarketData::Candle(c) => c.close.to_f64(),
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Snapshot(s) => s.mid_price().to_f64(),
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Order(o) => o.price.to_f64(),
        })
        .unwrap_or(50000.0); // Fallback price
    
    // Get first timestamp from market data for proper position tracking
    let first_timestamp = market_data.first()
        .map(|d| match d {
            dataloader::MarketData::Trade(t) => t.timestamp,
            dataloader::MarketData::Candle(c) => c.timestamp,
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Snapshot(s) => s.timestamp,
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Order(o) => o.timestamp,
        })
        .unwrap_or_else(chrono::Utc::now);
    
    // Initialize portfolio with BOTH quote and base assets for proper market making
    // Split capital: 50% as quote (USD), 50% as base asset (BTC/ETH)
    let inventory_pct = 0.5;
    let quote_balance = config.trading.initial_capital * (1.0 - inventory_pct);
    let base_value = config.trading.initial_capital * inventory_pct;
    let base_quantity = if initial_price > 0.0 { base_value / initial_price } else { 0.0 };
    
    let mut portfolio = PortfolioState::with_inventory(
        "BTC/USD".to_string(),
        bigdecimal::BigDecimal::from_f64(quote_balance).unwrap_or_default(),
        base_quantity,
        bigdecimal::BigDecimal::from_f64(initial_price).unwrap_or_default(),
        first_timestamp,
    );
    
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
    
    // ========== COMPREHENSIVE FITNESS FUNCTION ==========
    // Same logic as backtest/src/engine.rs for consistency
    
    use bigdecimal::ToPrimitive;
    let initial_capital = config.trading.initial_capital;
    
    // === Primary Metrics ===
    let sharpe = result.sharpe_ratio.unwrap_or(0.0);
    let sortino = result.sortino_ratio.unwrap_or(sharpe);
    let pf = result.profit_factor.unwrap_or(1.0);
    let win_rate = result.win_rate.unwrap_or(0.0);
    let net_profit_pct = result.net_profit.unwrap_or(0.0) / initial_capital;
    let calmar = result.calmar_ratio.unwrap_or(0.0);
    
    // === Base Fitness (weights sum to 1.0) ===
    let mut fitness = 
        sortino * 0.25 +
        sharpe * 0.15 +
        (pf - 1.0).max(0.0) * 0.20 +
        win_rate * 0.15 +
        net_profit_pct * 0.15 +
        calmar.min(3.0) * 0.10;
    
    // === Penalty: Insufficient Trades ===
    if result.num_trades < 30 {
        fitness *= (result.num_trades as f64 / 30.0).max(0.1);
    }
    
    // === Penalty: Excessive Drawdown ===
    let dd_value = result.max_drawdown.to_f64().unwrap_or(0.0);
    let dd_pct = (dd_value / initial_capital).abs();
    if dd_pct > 0.15 {
        let dd_penalty = if dd_pct > 0.40 { 0.2 } else if dd_pct > 0.25 { 0.5 } else { 0.8 };
        fitness *= dd_penalty;
    }
    
    // === Penalty: Commission Inefficiency ===
    if let (Some(gross_profit), Some(transaction_costs)) = 
        (&result.gross_profit, &result.transaction_costs) 
    {
        let gross = *gross_profit;
        let commission = transaction_costs.total_commission;
        
        if gross > 0.0 {
            let commission_ratio = commission / gross;
            if commission_ratio > 0.40 {
                let penalty = if commission_ratio >= 1.0 { 0.1 }
                    else if commission_ratio >= 0.70 { 0.3 }
                    else { 1.0 - (commission_ratio - 0.4) * 1.5 };
                fitness *= penalty;
            }
        } else if gross <= 0.0 && commission > 0.0 {
            fitness *= 0.05;
        }
    }
    
    // === Penalty: Poor Average Trade ===
    if let (Some(avg_trade), Some(transaction_costs)) = 
        (&result.avg_trade_return, &result.transaction_costs)
    {
        let avg_commission = if result.num_trades > 0 {
            transaction_costs.total_commission / result.num_trades as f64
        } else { 0.0 };
        
        if *avg_trade > 0.0 && *avg_trade < avg_commission * 2.0 {
            fitness *= 0.7;
        }
    }
    
    // === Bonus: Well-Rounded Strategy ===
    if net_profit_pct > 0.05 && pf > 1.5 && win_rate > 0.50 && dd_pct < 0.15 {
        fitness *= 1.2;
    }
    
    fitness.max(0.0)
}

