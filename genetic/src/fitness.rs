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
        
        // Inventory limits for market neutrality
        max_inventory_pct: 0.15,        // Hard limit: max 15% directional exposure
        inventory_skew_multiplier: 3.0, // Aggressive skew when approaching limit
        
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
        &market_data,  // Pass slice reference
        config,
    ).await;
    
    // ========== STARTUP MM FITNESS FUNCTION ==========
    // Optimized for: Maximum capital efficiency with strict risk control
    // Target profile: High turnover, low drawdown, consistent profits
    
    use bigdecimal::ToPrimitive;
    let initial_capital = config.trading.initial_capital;
    
    // === Extract Metrics ===
    let sharpe = result.sharpe_ratio.unwrap_or(0.0);
    let sortino = result.sortino_ratio.unwrap_or(sharpe);
    let pf = result.profit_factor.unwrap_or(1.0);
    let win_rate = result.win_rate.unwrap_or(0.0);
    let net_profit = result.net_profit.unwrap_or(0.0);
    let net_profit_pct = net_profit / initial_capital;
    
    let dd_value = result.max_drawdown.to_f64().unwrap_or(0.0);
    let dd_pct = (dd_value / initial_capital).abs();
    
    let gross_profit = result.gross_profit.unwrap_or(0.0);
    let total_commission = result.transaction_costs
        .as_ref()
        .map(|tc| tc.total_commission)
        .unwrap_or(0.0);
    
    // Calculate round-trips (completed inventory cycles)
    let (buy_fills, sell_fills) = result.inventory_metrics
        .as_ref()
        .map(|im| (im.buy_fills, im.sell_fills))
        .unwrap_or((0, 0));
    let round_trips = buy_fills.min(sell_fills);
    
    // Estimate trading days from data (rough: assume ~1000 ticks per day for HF data)
    let data_points = result.equity_curve.len().max(1);
    let trading_days = (data_points as f64 / 5000.0).max(1.0);
    
    // === 1. CAPITAL EFFICIENCY (40% weight) ===
    // As a startup, you need MAX returns on LIMITED capital
    // Return per unit of risk taken (drawdown-adjusted return)
    let annualized_return = net_profit_pct * (252.0 / trading_days);
    let capital_efficiency = if dd_pct > 0.001 {
        (annualized_return / dd_pct).max(0.0).min(10.0)  // Cap to avoid outliers
    } else if net_profit_pct > 0.0 {
        5.0  // Good profit with minimal drawdown = excellent
    } else {
        0.0
    };
    let capital_efficiency_normalized = capital_efficiency / 10.0;  // Normalize to [0, 1]
    
    // === 2. TURNOVER BONUS (15% weight) ===
    // More round-trips = more spread captured = better for limited capital
    // Target: 50 round-trips per backtest period
    let turnover_score = (round_trips as f64 / 50.0).min(1.0);
    
    // === 3. RISK-ADJUSTED RETURN (25% weight) ===
    // Sortino is better than Sharpe for MM (we care about downside, not upside vol)
    let sortino_capped = sortino.max(0.0).min(3.0) / 3.0;  // Normalize to [0, 1]
    
    // === 4. COMMISSION EFFICIENCY (10% weight) ===
    // Critical for startup - every bps matters
    let comm_efficiency = if gross_profit > 0.0 {
        (1.0 - (total_commission / gross_profit).min(1.0)).max(0.0)
    } else {
        0.0
    };
    
    // === 5. CONSISTENCY (10% weight) ===
    // Limited capital = can't afford losing streaks
    let consistency = if result.num_trades > 10 {
        let win_component = win_rate.min(1.0) * 0.5;
        let pf_component = (pf.min(3.0) / 3.0) * 0.5;
        win_component + pf_component
    } else {
        0.0
    };
    
    // === BASE FITNESS (weights sum to 1.0) ===
    let mut fitness = 
        capital_efficiency_normalized * 0.40 +
        turnover_score * 0.15 +
        sortino_capped * 0.25 +
        comm_efficiency * 0.10 +
        consistency * 0.10;
    
    // === HARD PENALTIES ===
    
    // Drawdown: Startup can't survive >10% drawdown
    if dd_pct > 0.10 {
        let dd_penalty = if dd_pct > 0.20 { 0.1 } else { 0.5 };
        fitness *= dd_penalty;
    }
    
    // Minimum activity: Need at least 20 trades for statistical significance
    if result.num_trades < 20 {
        fitness *= (result.num_trades as f64 / 20.0).max(0.1);
    }
    
    // Profit required: Negative strategies are worthless
    if net_profit_pct <= 0.0 {
        fitness *= 0.1;
    }
    
    // Commission disaster: If commission > gross profit, strategy is broken
    if gross_profit > 0.0 && total_commission > gross_profit {
        fitness *= 0.05;
    }
    
    // === BONUS: IDEAL STARTUP MM PROFILE ===
    // High turnover + low drawdown + positive = perfect
    if round_trips > 30 && dd_pct < 0.08 && net_profit_pct > 0.03 {
        fitness *= 1.3;
    }
    
    // Extra bonus: Very high turnover with profitability (spread capture machine)
    if round_trips > 50 && net_profit_pct > 0.02 && comm_efficiency > 0.6 {
        fitness *= 1.2;
    }
    
    fitness.max(0.0)
}

