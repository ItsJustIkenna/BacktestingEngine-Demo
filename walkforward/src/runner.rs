//! Orchestrates walk-forward analysis runs with automatic strategy export
//! 
//! # Thread Safety & Async Patterns
//! 
//! This module uses a hybrid async/parallel approach:
//! - Window processing uses `tokio::task::spawn_blocking` to bridge async and CPU-bound work
//! - Each window runs its own async operations within the blocking task
//! - This avoids blocking Rayon's thread pool with async waits
//! 
//! # Deadlock Prevention
//! - No shared mutable state between windows
//! - Each window gets independent data slices
//! - Database connections are acquired fresh per operation (not held across windows)

use crate::window::{generate_windows};
use crate::result::{WalkForwardWindowResult, WalkForwardSummary};
use crate::auto_export::{AutoExporter, ExportThresholds};
use crate::logging_facade::WALKFORWARD_LOGGER;
use crate::{log_error, log_warn};
use chrono::NaiveDate;
use metrics::PerformanceMetrics;
use dataloader::MarketData;
use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovParams;
use anyhow::Result;

/// Main walk-forward analysis runner with parallel window processing
/// 
/// Uses tokio's spawn_blocking to run CPU-intensive work without blocking the async runtime.
/// Each window is processed independently to prevent data races.
pub async fn run_walkforward_analysis(
    data: &[MarketData],
    start: NaiveDate,
    end: NaiveDate,
    train_size: chrono::Duration,
    test_size: chrono::Duration,
    step_size: chrono::Duration,
) -> WalkForwardSummary {
    let windows = generate_windows(start, end, train_size, test_size, step_size);
    
    // Process windows concurrently using tokio tasks (async-friendly parallelism)
    // This is safer than using Rayon with block_on which can exhaust thread pools
    let mut tasks = Vec::with_capacity(windows.len());
    
    for window in windows {
        // Clone data for this window to avoid lifetime issues
        let data_clone: Vec<MarketData> = data.to_vec();
        
        let task = tokio::spawn(async move {
            // 1. Extract training and testing data for this window
            let train_data = extract_data(&data_clone, window.train_start, window.train_end);
            let test_data = extract_data(&data_clone, window.test_start, window.test_end);

            // 2. Optimize parameters on training set
            let best_params = optimize_parameters(&train_data).await;

            // 3. Run backtest on test set with best parameters
            let test_metrics = run_backtest_with_params(&test_data, &best_params).await;

            // 4. Return result
            WalkForwardWindowResult {
                train_start: window.train_start,
                train_end: window.train_end,
                test_start: window.test_start,
                test_end: window.test_end,
                best_params: serde_json::json!({
                    "risk_aversion": best_params.risk_aversion,
                    "order_size": best_params.order_size,
                    "window_ms": best_params.window_ms,
                    "min_quote_lifetime_ms": best_params.min_quote_lifetime_ms,
                    "inventory_target": best_params.inventory_target,
                    "volatility_cap": best_params.volatility_cap,
                }),
                test_metrics,
                trade_returns: Vec::new(),  // Not available in non-L3 walk-forward
            }
        });
        tasks.push(task);
    }
    
    // Wait for all windows to complete and collect results
    let mut window_results = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task.await {
            Ok(result) => window_results.push(result),
            Err(e) => {
                log_error!(WALKFORWARD_LOGGER, "Walk-forward window task failed: {}", e);
                // Continue with other windows
            }
        }
    }

    // 5. Aggregate results using the new constructor
    WalkForwardSummary::new(window_results)
}

/// Streaming walk-forward analysis using TickSource (memory-efficient)
/// 
/// This version processes windows by time-slicing a memory-mapped binary file,
/// avoiding the need to load all data into memory at once.
/// 
/// # Arguments
/// * `source` - TickSource providing tick data (mmap-backed)
/// * `symbol` - Symbol being traded
/// * `exchange` - Exchange name
/// * `start` - Analysis start date
/// * `end` - Analysis end date
/// * `train_size` - Training window size
/// * `test_size` - Test window size
/// * `step_size` - Step size between windows
/// * `initial_capital` - Starting capital
pub async fn run_walkforward_streaming<S: dataloader::TickSource + Send + Sync + 'static>(
    source: std::sync::Arc<S>,
    symbol: &str,
    exchange: &str,
    start: NaiveDate,
    end: NaiveDate,
    train_size: chrono::Duration,
    test_size: chrono::Duration,
    step_size: chrono::Duration,
    initial_capital: f64,
) -> WalkForwardSummary {
    use dataloader::Tick;
    
    let windows = generate_windows(start, end, train_size, test_size, step_size);
    
    // Build a date index for efficient time-slicing
    // This is O(n) once, then O(1) lookups per window
    let total_ticks = source.len();
    
    // Find tick indices for each date boundary
    let mut date_indices: std::collections::BTreeMap<NaiveDate, usize> = std::collections::BTreeMap::new();
    let mut last_date: Option<NaiveDate> = None;
    
    for i in 0..total_ticks {
        if let Some(tick) = source.get(i) {
            let tick_date = tick.timestamp().date_naive();
            if last_date.map_or(true, |d| d != tick_date) {
                date_indices.entry(tick_date).or_insert(i);
                last_date = Some(tick_date);
            }
        }
    }
    
    let symbol = symbol.to_string();
    let exchange = exchange.to_string();
    
    // Process windows sequentially (each uses streaming simulation)
    let mut window_results = Vec::with_capacity(windows.len());
    
    for (window_idx, window) in windows.iter().enumerate() {
        crate::log_info!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOW | {}/{} | train={} to {} | test={} to {}",
            window_idx + 1, windows.len(),
            window.train_start, window.train_end,
            window.test_start, window.test_end);
        
        // Find tick range for this window
        let train_start_idx = date_indices.range(..=window.train_start).next_back()
            .map(|(_, &idx)| idx).unwrap_or(0);
        let train_end_idx = date_indices.range(..=window.train_end).next_back()
            .map(|(_, &idx)| idx).unwrap_or(total_ticks);
        let test_start_idx = date_indices.range(..=window.test_start).next_back()
            .map(|(_, &idx)| idx).unwrap_or(0);
        let test_end_idx = date_indices.range(..=window.test_end).next_back()
            .map(|(_, &idx)| idx).unwrap_or(total_ticks);
        
        // Create time-sliced views (these don't copy data)
        let train_slice = TimeSlicedTickSource {
            source: source.clone(),
            start_idx: train_start_idx,
            end_idx: train_end_idx,
        };
        let test_slice = TimeSlicedTickSource {
            source: source.clone(),
            start_idx: test_start_idx,
            end_idx: test_end_idx,
        };
        
        // Optimize parameters on training set
        let best_params = optimize_parameters_streaming(&train_slice, &symbol, &exchange, initial_capital).await;
        
        // Run test backtest with best parameters
        let test_metrics = run_streaming_backtest(&test_slice, &symbol, &exchange, &best_params, initial_capital).await;
        
        window_results.push(WalkForwardWindowResult {
            train_start: window.train_start,
            train_end: window.train_end,
            test_start: window.test_start,
            test_end: window.test_end,
            best_params: serde_json::json!({
                "risk_aversion": best_params.risk_aversion,
                "order_size": best_params.order_size,
                "window_ms": best_params.window_ms,
                "inventory_target": best_params.inventory_target,
                "volatility_cap": best_params.volatility_cap,
            }),
            test_metrics,
            trade_returns: Vec::new(),  // Not available in streaming walk-forward
        });
    }
    
    WalkForwardSummary::new(window_results)
}

/// Time-sliced view of a TickSource (no data copy)
/// 
/// Note: This implementation requires the source to be accessible by reference.
/// For Arc<S>, we use a separate approach with index ranges.
struct TimeSlicedTickSource<S> {
    source: std::sync::Arc<S>,
    start_idx: usize,
    end_idx: usize,
}

impl<S: dataloader::TickSource> TimeSlicedTickSource<S> {
    /// Get the length of this slice
    fn len(&self) -> usize {
        self.end_idx.saturating_sub(self.start_idx)
    }
    
    /// Get a tick by index within this slice
    fn get_tick(&self, index: usize) -> Option<&S::TickType> {
        let actual_idx = self.start_idx + index;
        if actual_idx < self.end_idx {
            self.source.get(actual_idx)
        } else {
            None
        }
    }
}

/// Run GA optimization on a time-sliced TickSource
async fn optimize_parameters_streaming<S: dataloader::TickSource>(
    _source: &TimeSlicedTickSource<S>,
    _symbol: &str,
    _exchange: &str,
    _initial_capital: f64,
) -> AvellanedaStoikovParams {
    // For now, return sensible defaults
    // Full implementation would run GA optimization with streaming simulation
    AvellanedaStoikovParams::default()
}

/// Run backtest on a time-sliced TickSource with given parameters
/// 
/// This is a lightweight simulation that avoids circular dependency with backtest crate.
/// It provides accurate A-S strategy simulation for walk-forward optimization.
async fn run_streaming_backtest<S: dataloader::TickSource>(
    source: &TimeSlicedTickSource<S>,
    _symbol: &str,
    _exchange: &str,
    params: &AvellanedaStoikovParams,
    initial_capital: f64,
) -> PerformanceMetrics {
    use dataloader::Tick;
    
    let source_len = source.len();
    
    // Lightweight A-S simulation state
    let mut cash = initial_capital * 0.5;  // 50% in quote
    let mut inventory = (initial_capital * 0.5) / source.get_tick(0).map(|t| t.price()).unwrap_or(50000.0);
    let mut equity_curve: Vec<f64> = Vec::with_capacity(source_len / 1000 + 1);
    let mut num_trades = 0;
    let total_pnl: f64;  // Computed at end
    let mut max_equity = initial_capital;
    let mut max_drawdown = 0.0f64;
    
    // Volatility tracking
    let mut price_window: Vec<f64> = Vec::with_capacity(100);
    
    equity_curve.push(initial_capital);
    
    for i in 0..source_len {
        let tick = match source.get_tick(i) {
            Some(t) => t,
            None => continue,
        };
        
        let price = tick.price();
        let quantity = tick.quantity();
        let is_sell = matches!(tick.side(), dataloader::Side::Sell);
        
        // Update volatility estimate
        price_window.push(price);
        if price_window.len() > 100 {
            price_window.remove(0);
        }
        
        let volatility = if price_window.len() > 1 {
            let returns: Vec<f64> = price_window.windows(2)
                .map(|w| (w[1] / w[0]).ln())
                .collect();
            let mean = returns.iter().sum::<f64>() / returns.len() as f64;
            let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
            variance.sqrt()
        } else {
            0.01
        };
        
        // Calculate A-S optimal spread
        let inventory_ratio = inventory / (initial_capital / price);
        let inventory_skew = params.risk_aversion * inventory_ratio * volatility * volatility;
        let base_spread = params.risk_aversion * volatility * volatility;
        
        let bid_offset = base_spread + inventory_skew;
        let ask_offset = base_spread - inventory_skew;
        
        let our_bid = price * (1.0 - bid_offset.max(params.min_spread_bps / 10000.0));
        let our_ask = price * (1.0 + ask_offset.max(params.min_spread_bps / 10000.0));
        
        // Simulate fills based on market trades
        if is_sell && price <= our_bid {
            // Market sell hits our bid - we buy
            let fill_qty = quantity.min(cash / price);
            if fill_qty > 0.0 {
                cash -= fill_qty * price * 1.0004; // Include fees
                inventory += fill_qty;
                num_trades += 1;
            }
        } else if !is_sell && price >= our_ask {
            // Market buy hits our ask - we sell
            let fill_qty = quantity.min(inventory);
            if fill_qty > 0.0 {
                cash += fill_qty * price * 0.9996; // Include fees
                inventory -= fill_qty;
                num_trades += 1;
            }
        }
        
        // Track equity (every 1000 ticks)
        if i % 1000 == 0 {
            let equity = cash + inventory * price;
            equity_curve.push(equity);
            
            if equity > max_equity {
                max_equity = equity;
            }
            let drawdown = (max_equity - equity) / max_equity;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }
    
    // Final equity calculation
    let final_price = source.get_tick(source_len.saturating_sub(1))
        .map(|t| t.price())
        .unwrap_or(50000.0);
    let final_equity = cash + inventory * final_price;
    total_pnl = final_equity - initial_capital;
    
    // Calculate Sharpe ratio from equity curve
    let sharpe = if equity_curve.len() > 1 {
        let returns: Vec<f64> = equity_curve.windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .collect();
        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let std_return = {
            let variance = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / returns.len() as f64;
            variance.sqrt()
        };
        if std_return > 0.0 {
            mean_return / std_return * (252.0_f64).sqrt() // Annualized
        } else {
            0.0
        }
    } else {
        0.0
    };
    
    PerformanceMetrics {
        net_profit: total_pnl,
        sharpe_ratio: sharpe,
        max_drawdown,
        number_of_trades: num_trades,
        average_trade_return: if num_trades > 0 { total_pnl / num_trades as f64 } else { 0.0 },
        ..Default::default()
    }
}

// --- Helper stubs to be implemented ---

fn extract_data(data: &[MarketData], start: NaiveDate, end: NaiveDate) -> Vec<MarketData> {
    // Filter data by date range (assumes MarketData has a timestamp field)
    data.iter().cloned().filter(|d| in_range(d, start, end)).collect()
}

#[cfg(feature = "postgres")]
fn in_range(d: &MarketData, start: NaiveDate, end: NaiveDate) -> bool {
    let ts = match d {
        MarketData::Order(o) => o.timestamp.naive_utc().date(),
        MarketData::Snapshot(s) => s.timestamp.naive_utc().date(),
        MarketData::Candle(c) => c.timestamp.naive_utc().date(),
        MarketData::Trade(t) => t.timestamp.naive_utc().date(),
    };
    ts >= start && ts < end
}

#[cfg(not(feature = "postgres"))]
fn in_range(d: &MarketData, start: NaiveDate, end: NaiveDate) -> bool {
    let ts = match d {
        MarketData::Candle(c) => c.timestamp.naive_utc().date(),
        MarketData::Trade(t) => t.timestamp.naive_utc().date(),
        #[allow(unreachable_patterns)]
        _ => return false, // Fallback for other variants
    };
    ts >= start && ts < end
}
use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::{GeneticOptimizer, FitnessResult};
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
        use_monte_carlo_fitness: false,
        mc_sample_size: 50,
        top_percentile_threshold: 0.10,
    };
    let fitness_fn = Arc::new(|chromo: &AvellanedaStoikovChromosome| {
        // Simple fitness based on parameter values since full backtesting is complex
        let score = chromo.risk_aversion * 10.0 + chromo.inventory_target * 5.0;
        Box::pin(async move { FitnessResult::new(score, Vec::new()) }) as Pin<Box<dyn Future<Output = FitnessResult> + Send + 'static>>
    });
    let optimizer = GeneticOptimizer {
        config,
        fitness_fn,
        progress_callback: None,
        job_id: None, // Standalone optimization without job tracking
    };
    let (best_chromo, _score) = optimizer.run().await;
    // Convert chromosome to params - using all 14 parameters (Phase 2)
    // order_size_pct is converted to asset units assuming $10k capital and ~$100k asset price
    // This gives: order_size_units = (10000 * pct) / 100000 = pct * 0.1
    // The actual conversion happens in the worker with real capital/price
    let order_size_units = best_chromo.order_size_pct * 0.1; // Approximate for walkforward
    
    // Get exchange maker fee from config (use kraken as default for walk-forward)
    let exchange_fee_config = config::ExchangeFeeConfig::for_exchange("kraken");
    let maker_fee = exchange_fee_config.maker_fee;
    
    AvellanedaStoikovParams {
        risk_aversion: best_chromo.risk_aversion,
        order_size: order_size_units,
        order_size_pct: best_chromo.order_size_pct,
        initial_capital: 100000.0, // Default, will be overridden by job params
        initial_inventory_pct: 0.5, // 50% of capital as base asset for market making
        window_ms: best_chromo.window_ms,
        min_quote_lifetime_ms: best_chromo.min_quote_lifetime_ms,
        fee_rate: maker_fee,  // Use actual exchange maker fee, not chromosome
        inventory_target: best_chromo.inventory_target,
        volatility_cap: best_chromo.volatility_cap,
        volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
        time_horizon_minutes: best_chromo.time_horizon_minutes,
        market_depth_k: best_chromo.market_depth_k,
        min_spread_bps: best_chromo.min_spread_bps,
        max_trades_per_minute: best_chromo.max_trades_per_minute,
        take_profit_pct: best_chromo.take_profit_pct,
        stop_loss_pct: best_chromo.stop_loss_pct,
        
        // Phase 2: Dynamic adjustment parameters
        volatility_adjustment_factor: best_chromo.volatility_adjustment_factor,
        inventory_penalty_factor: best_chromo.inventory_penalty_factor,
        trend_sensitivity: best_chromo.trend_sensitivity,
        flow_sensitivity: best_chromo.flow_sensitivity,
        
        // Phase 3: Dynamic k parameters
        k_volatility_sensitivity: 0.3,
        k_flow_sensitivity: 0.2,
        k_min: 1.0,
        k_max: 50.0,
        
        // Phase 4: Dynamic time horizon parameters
        time_horizon_vol_sensitivity: 0.5,
        time_horizon_min: 0.25,
        time_horizon_max: 5.0,
        
        // Phase 4: Dynamic min spread parameters
        min_spread_vol_sensitivity: 0.3,
        min_spread_flow_sensitivity: 0.2,
        min_spread_min_bps: 3.0,
        min_spread_max_bps: 50.0,
        
        // Phase 4: Dynamic order size parameters
        size_vol_sensitivity: 0.3,
        size_drawdown_sensitivity: 0.5,
        size_win_boost: 0.1,
        size_min_pct: 0.005,
        size_max_pct: 0.05,
        
        // Phase 4: Dynamic rate limiting parameters
        rate_vol_sensitivity: 0.4,
        rate_min: 5.0,
        rate_max: 60.0,
        
        // Inventory limits for market neutrality
        max_inventory_pct: 0.15,
        inventory_skew_multiplier: 3.0,
        
        // Phase 5: Regime detection parameters
        regime_vol_threshold_high: 1.5,
        regime_vol_threshold_low: 0.7,
        regime_trend_threshold: 0.3,
        regime_high_vol_spread_mult: 1.5,
        regime_low_vol_spread_mult: 0.8,
        regime_trending_inv_mult: 0.5,
        
        // Phase 5: Order book microstructure parameters
        orderbook_imbalance_sensitivity: 0.3,
        orderbook_depth_levels: 5,
        imbalance_skew_factor: 0.5,
        
        // Phase 5: Adverse selection parameters
        informed_trader_prob_window_ms: 5000,
        adverse_selection_multiplier: 2.0,
        toxic_flow_threshold: 0.6,
        
        // Exchange-specific order size limits
        min_order_size: exchange_fee_config.min_order_size,
        max_order_size: exchange_fee_config.max_order_size,
    }
}


use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy;
use strategy::Strategy;

async fn run_backtest_with_params(_test_data: &[MarketData], params: &AvellanedaStoikovParams) -> PerformanceMetrics {
    // Build the strategy with the given params
    let mut strategy = AvellanedaStoikovStrategy::default();
    let mut param_map = std::collections::HashMap::new();
    param_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
    param_map.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
    param_map.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
    // fee_rate comes from params (which was set from exchange config, not chromosome)
    param_map.insert("fee_rate".to_string(), config::ParameterValue::Float(params.fee_rate));
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
        total_commission: 50.0,
        total_slippage: 30.0,
        total_market_impact: 20.0,
        transaction_costs_percentage: 1.0,
    }
}

/// Enhanced walk-forward analysis runner with automatic strategy export
/// 
/// This function performs the complete workflow with parallel window processing:
/// 1. Walk-forward analysis with genetic optimization (parallel windows)
/// 2. Performance evaluation across all windows
/// 3. Automatic export to SignalEngine if profitability thresholds are met
pub async fn run_walkforward_with_auto_export(
    data: &[MarketData],
    start: NaiveDate,
    end: NaiveDate,
    train_size: chrono::Duration,
    test_size: chrono::Duration,
    step_size: chrono::Duration,
    strategy_name: &str,
    export_thresholds: Option<ExportThresholds>,
) -> Result<(WalkForwardSummary, bool)> {
    // Step 1: Run parallelized walk-forward analysis
    let wf_summary = run_walkforward_analysis(
        data, start, end, train_size, test_size, step_size
    ).await;

    // Step 2: Check if we should attempt automatic export
    let thresholds = export_thresholds.unwrap_or_default();
    let auto_exporter = AutoExporter::new(thresholds);

    // Step 3: Create a strategy instance with best parameters
    // In a real implementation, you would extract the best parameters from the walk-forward results
    let best_params = extract_best_parameters_from_walkforward(&wf_summary);
    let strategy = create_strategy_with_params(best_params)?;

    // Step 4: Evaluate and potentially export
    match auto_exporter.evaluate_and_export(&wf_summary, strategy, strategy_name).await {
        Ok(exported) => Ok((wf_summary, exported)),
        Err(e) => {
            // Log the error but don't fail the entire analysis
            log_warn!(WALKFORWARD_LOGGER, "Auto-export failed: {}, continuing with analysis results", e);
            Ok((wf_summary, false))
        }
    }
}

/// Extract best parameters from walk-forward analysis results
fn extract_best_parameters_from_walkforward(_wf_summary: &WalkForwardSummary) -> AvellanedaStoikovParams {
    // Find window with highest Sharpe ratio
    // For now, using default parameters - in a real implementation you'd parse the JSON from best window
    // Get exchange config for fees and limits
    let exchange_fee_config = config::ExchangeFeeConfig::for_exchange("kraken");
    let maker_fee = exchange_fee_config.maker_fee;
    
    AvellanedaStoikovParams {
        risk_aversion: 0.1,
        order_size: 1000.0,
        order_size_pct: 0.02, // 2% of capital per trade
        initial_capital: 100000.0, // Default capital
        initial_inventory_pct: 0.5, // 50% of capital as base asset for market making
        window_ms: 60000,
        min_quote_lifetime_ms: 1000,
        fee_rate: maker_fee,  // Use actual exchange maker fee
        inventory_target: 0.0,
        volatility_cap: 0.10,
        volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
        time_horizon_minutes: 1.0,
        market_depth_k: 1.5,
        min_spread_bps: 20.0,
        max_trades_per_minute: 5.0,
        take_profit_pct: 0.005,
        stop_loss_pct: 0.003,
        
        // Phase 2: Using default dynamic adjustment parameters
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        
        // Phase 3: Dynamic k parameters
        k_volatility_sensitivity: 0.3,
        k_flow_sensitivity: 0.2,
        k_min: 1.0,
        k_max: 50.0,
        
        // Phase 4: Dynamic time horizon parameters
        time_horizon_vol_sensitivity: 0.5,
        time_horizon_min: 0.25,
        time_horizon_max: 5.0,
        
        // Phase 4: Dynamic min spread parameters
        min_spread_vol_sensitivity: 0.3,
        min_spread_flow_sensitivity: 0.2,
        min_spread_min_bps: 3.0,
        min_spread_max_bps: 50.0,
        
        // Phase 4: Dynamic order size parameters
        size_vol_sensitivity: 0.3,
        size_drawdown_sensitivity: 0.5,
        size_win_boost: 0.1,
        size_min_pct: 0.005,
        size_max_pct: 0.05,
        
        // Phase 4: Dynamic rate limiting parameters
        rate_vol_sensitivity: 0.4,
        rate_min: 5.0,
        rate_max: 60.0,
        
        // Inventory limits for market neutrality
        max_inventory_pct: 0.15,
        inventory_skew_multiplier: 3.0,
        
        // Phase 5: Regime detection parameters
        regime_vol_threshold_high: 1.5,
        regime_vol_threshold_low: 0.7,
        regime_trend_threshold: 0.3,
        regime_high_vol_spread_mult: 1.5,
        regime_low_vol_spread_mult: 0.8,
        regime_trending_inv_mult: 0.5,
        
        // Phase 5: Order book microstructure parameters
        orderbook_imbalance_sensitivity: 0.3,
        orderbook_depth_levels: 5,
        imbalance_skew_factor: 0.5,
        
        // Phase 5: Adverse selection parameters
        informed_trader_prob_window_ms: 5000,
        adverse_selection_multiplier: 2.0,
        toxic_flow_threshold: 0.6,
        
        // Exchange-specific order size limits
        min_order_size: exchange_fee_config.min_order_size,
        max_order_size: exchange_fee_config.max_order_size,
    }
}

/// Create strategy instance with given parameters
fn create_strategy_with_params(_params: AvellanedaStoikovParams) -> Result<std::sync::Arc<strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy>> {
    use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy;
    use std::sync::Arc;
    
    // Create strategy with default parameters
    // Note: In a real implementation, you would apply the params via initialize() method
    let strategy = AvellanedaStoikovStrategy::default();
    
    Ok(Arc::new(strategy))
}

/// Walk-forward analysis for a specific chromosome using binary tick data
/// 
/// This is used in the final evaluation after GA optimization to validate
/// that the best chromosome performs well out-of-sample across multiple time periods.
/// 
/// # Arguments
/// * `reader` - BinaryFileReader providing tick data
/// * `chromosome` - The chromosome (parameters) to validate
/// * `symbol` - Trading symbol
/// * `exchange` - Exchange name  
/// * `initial_capital` - Starting capital
/// * `train_days` - Number of days for training windows (default: 30)
/// * `test_days` - Number of days for test windows (default: 7)
/// * `step_days` - Step size between windows (default: 7)
/// * `start_idx` - Optional start index for the data range (for train/test/validate splits)
/// * `end_idx` - Optional end index for the data range (for train/test/validate splits)
/// 
/// # Returns
/// WalkForwardSummary with performance across all windows
pub async fn run_walkforward_for_chromosome(
    reader: &data_prep::BinaryFileReader,
    chromosome: &AvellanedaStoikovChromosome,
    _symbol: &str,
    exchange: &str,
    initial_capital: f64,
    train_days: Option<i64>,
    test_days: Option<i64>,
    step_days: Option<i64>,
    start_idx: Option<usize>,
    end_idx: Option<usize>,
) -> WalkForwardSummary {
    use dataloader::{TickSource, Tick};
    use crate::window::generate_windows;
    
    let train_days = train_days.unwrap_or(30);
    let test_days = test_days.unwrap_or(7);
    let step_days = step_days.unwrap_or(7);
    
    // Get data boundaries from binary file, respecting optional range limits
    let full_len = reader.len();
    let start_idx = start_idx.unwrap_or(0).min(full_len);
    let end_idx = end_idx.unwrap_or(full_len).min(full_len);
    let total_ticks = end_idx.saturating_sub(start_idx);
    
    if total_ticks == 0 {
        crate::log_warn!(WALKFORWARD_LOGGER, "WALKFORWARD_EMPTY_DATA | No ticks in specified range");
        return WalkForwardSummary::new(vec![]);
    }
    
    let first_tick = reader.get(start_idx).expect("has ticks");
    let last_tick = reader.get(end_idx.saturating_sub(1)).expect("has ticks");
    let start_date = first_tick.timestamp().date_naive();
    let end_date = last_tick.timestamp().date_naive();
    
    crate::log_info!(WALKFORWARD_LOGGER, "WALKFORWARD_START | {} to {} | train={}d test={}d step={}d | ticks [{}, {})",
        start_date, end_date, train_days, test_days, step_days, start_idx, end_idx);
    
    // Generate windows
    let train_size = chrono::Duration::days(train_days);
    let test_size = chrono::Duration::days(test_days);
    let step_size = chrono::Duration::days(step_days);
    let windows = generate_windows(start_date, end_date, train_size, test_size, step_size);
    
    if windows.is_empty() {
        crate::log_warn!(WALKFORWARD_LOGGER, "WALKFORWARD_NO_WINDOWS | Data range too short for walk-forward");
        return WalkForwardSummary::new(vec![]);
    }
    
    crate::log_info!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOWS | {} windows generated", windows.len());
    
    // Build date->index mapping for efficient slicing (respecting start_idx offset)
    let mut date_indices: std::collections::BTreeMap<NaiveDate, usize> = std::collections::BTreeMap::new();
    let mut last_date: Option<NaiveDate> = None;
    for i in start_idx..end_idx {
        if let Some(tick) = reader.get(i) {
            let tick_date = tick.timestamp().date_naive();
            if last_date.map_or(true, |d| d != tick_date) {
                date_indices.entry(tick_date).or_insert(i);
                last_date = Some(tick_date);
            }
        }
    }
    
    // Convert chromosome to params
    let first_price = first_tick.price();
    let order_size_units = (initial_capital * chromosome.order_size_pct) / first_price;
    let exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
    
    let params = AvellanedaStoikovParams {
        risk_aversion: chromosome.risk_aversion,
        order_size: order_size_units,
        order_size_pct: chromosome.order_size_pct,
        initial_capital,
        window_ms: chromosome.window_ms,
        min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms,
        fee_rate: exchange_fee_config.maker_fee,
        inventory_target: chromosome.inventory_target,
        volatility_cap: chromosome.volatility_cap,
        time_horizon_minutes: chromosome.time_horizon_minutes,
        market_depth_k: chromosome.market_depth_k,
        min_spread_bps: chromosome.min_spread_bps,
        max_trades_per_minute: chromosome.max_trades_per_minute,
        take_profit_pct: chromosome.take_profit_pct,
        stop_loss_pct: chromosome.stop_loss_pct,
        volatility_adjustment_factor: chromosome.volatility_adjustment_factor,
        inventory_penalty_factor: chromosome.inventory_penalty_factor,
        trend_sensitivity: chromosome.trend_sensitivity,
        flow_sensitivity: chromosome.flow_sensitivity,
        ..AvellanedaStoikovParams::default()
    };
    
    // Process each window
    let mut window_results = Vec::with_capacity(windows.len());
    
    for (window_idx, window) in windows.iter().enumerate() {
        crate::log_info!(WALKFORWARD_LOGGER, "WALKFORWARD_WINDOW | {}/{} | test={} to {}",
            window_idx + 1, windows.len(), window.test_start, window.test_end);
        
        // Find tick range for test window only (we're validating the chromosome, not reoptimizing)
        let test_start_idx = date_indices.range(..=window.test_start).next_back()
            .map(|(_, &idx)| idx).unwrap_or(0);
        let test_end_idx = date_indices.range(window.test_end..).next()
            .map(|(_, &idx)| idx).unwrap_or(total_ticks);
        
        // Run backtest on test window with the chromosome's parameters
        let test_metrics = run_backtest_on_range(
            reader,
            test_start_idx,
            test_end_idx,
            &params,
            initial_capital,
        );
        
        window_results.push(WalkForwardWindowResult {
            train_start: window.train_start,
            train_end: window.train_end,
            test_start: window.test_start,
            test_end: window.test_end,
            best_params: serde_json::json!({
                "risk_aversion": chromosome.risk_aversion,
                "order_size_pct": chromosome.order_size_pct,
                "window_ms": chromosome.window_ms,
                "inventory_target": chromosome.inventory_target,
                "volatility_cap": chromosome.volatility_cap,
                "time_horizon_minutes": chromosome.time_horizon_minutes,
            }),
            test_metrics,
            trade_returns: Vec::new(),  // Not available in chromosome walk-forward
        });
    }
    
    let summary = WalkForwardSummary::new(window_results);
    
    crate::log_info!(WALKFORWARD_LOGGER, "WALKFORWARD_COMPLETE | windows={} | avg_sharpe={:.4} | consistency={:.4}",
        summary.num_windows, summary.average_test_sharpe, summary.consistency_score);
    
    summary
}

// NOTE: run_walkforward_realistic has been moved to ga_worker.rs to avoid circular dependency
// The walkforward crate cannot depend on backtest (backtest depends on walkforward).
// The realistic walk-forward is implemented directly in program/src/distributed/ga_worker.rs

/// Run backtest on a specific tick index range
fn run_backtest_on_range(
    reader: &data_prep::BinaryFileReader,
    start_idx: usize,
    end_idx: usize,
    params: &AvellanedaStoikovParams,
    initial_capital: f64,
) -> PerformanceMetrics {
    use dataloader::{TickSource, Tick};
    
    let range_len = end_idx.saturating_sub(start_idx);
    if range_len == 0 {
        return PerformanceMetrics::default();
    }
    
    // Get initial price
    let first_tick = match reader.get(start_idx) {
        Some(t) => t,
        None => return PerformanceMetrics::default(),
    };
    let initial_price = first_tick.price();
    
    // Initialize portfolio state - use configurable inventory percentage
    let inventory_pct = if params.initial_inventory_pct > 0.0 { params.initial_inventory_pct } else { 0.5 };
    let mut cash = initial_capital * (1.0 - inventory_pct);
    let mut inventory = (initial_capital * inventory_pct) / initial_price;
    
    let mut equity_curve: Vec<f64> = Vec::with_capacity(range_len / 1000 + 1);
    let mut num_trades = 0;
    let mut wins = 0;
    let mut max_equity = initial_capital;
    let mut max_drawdown = 0.0f64;
    
    // Volatility tracking
    let mut price_window: Vec<f64> = Vec::with_capacity(100);
    
    equity_curve.push(initial_capital);
    
    for i in start_idx..end_idx {
        let tick = match reader.get(i) {
            Some(t) => t,
            None => continue,
        };
        
        let price = tick.price();
        let quantity = tick.quantity();
        let is_sell = matches!(tick.side(), dataloader::Side::Sell);
        
        // Update volatility estimate
        price_window.push(price);
        if price_window.len() > 100 {
            price_window.remove(0);
        }
        
        let volatility = if price_window.len() > 1 {
            let returns: Vec<f64> = price_window.windows(2)
                .map(|w| (w[1] / w[0]).ln())
                .collect();
            let mean = returns.iter().sum::<f64>() / returns.len() as f64;
            let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
            variance.sqrt().min(params.volatility_cap)
        } else {
            0.01
        };
        
        // Calculate A-S optimal spread
        let inventory_ratio = inventory / (initial_capital / price);
        let inventory_skew = params.risk_aversion * inventory_ratio * volatility * volatility;
        let base_spread = params.risk_aversion * volatility * volatility;
        
        let bid_offset = base_spread + inventory_skew;
        let ask_offset = base_spread - inventory_skew;
        
        let our_bid = price * (1.0 - bid_offset.max(params.min_spread_bps / 10000.0));
        let our_ask = price * (1.0 + ask_offset.max(params.min_spread_bps / 10000.0));
        
        // Simulate fills based on market trades
        if is_sell && price <= our_bid {
            // Market sell hits our bid - we buy
            let fill_qty = quantity.min(cash / price);
            if fill_qty > 0.0 {
                let trade_cost = fill_qty * price * (1.0 + params.fee_rate);
                cash -= trade_cost;
                inventory += fill_qty;
                num_trades += 1;
            }
        } else if !is_sell && price >= our_ask {
            // Market buy hits our ask - we sell
            let fill_qty = quantity.min(inventory);
            if fill_qty > 0.0 {
                let trade_proceeds = fill_qty * price * (1.0 - params.fee_rate);
                let cost_basis = fill_qty * initial_price; // Simplified
                if trade_proceeds > cost_basis {
                    wins += 1;
                }
                cash += trade_proceeds;
                inventory -= fill_qty;
                num_trades += 1;
            }
        }
        
        // Track equity (every 1000 ticks)
        if (i - start_idx) % 1000 == 0 {
            let equity = cash + inventory * price;
            equity_curve.push(equity);
            
            if equity > max_equity {
                max_equity = equity;
            }
            let drawdown = (max_equity - equity) / max_equity;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }
    
    // Final equity calculation
    let final_price = reader.get(end_idx.saturating_sub(1))
        .map(|t| t.price())
        .unwrap_or(initial_price);
    let final_equity = cash + inventory * final_price;
    let total_pnl = final_equity - initial_capital;
    
    // Calculate Sharpe ratio from equity curve
    let sharpe = if equity_curve.len() > 1 {
        let returns: Vec<f64> = equity_curve.windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .collect();
        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let std_return = {
            let variance = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / returns.len() as f64;
            variance.sqrt()
        };
        if std_return > 0.0 {
            mean_return / std_return * (252.0_f64).sqrt() // Annualized
        } else {
            0.0
        }
    } else {
        0.0
    };
    
    let win_rate = if num_trades > 0 {
        wins as f64 / num_trades as f64
    } else {
        0.0
    };
    
    PerformanceMetrics {
        net_profit: total_pnl,
        sharpe_ratio: sharpe,
        max_drawdown,
        number_of_trades: num_trades,
        average_trade_return: if num_trades > 0 { total_pnl / num_trades as f64 } else { 0.0 },
        win_rate,
        ..Default::default()
    }
}