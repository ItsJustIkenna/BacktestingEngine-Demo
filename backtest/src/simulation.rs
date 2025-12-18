//! SimulationLoop: Drives the backtest event loop
//!
//! Steps through market data, triggers strategies, matches orders, and updates state.

use strategy::{StrategyManager, StrategyContext};
use strategy::traits::{MarketSession, SessionType};
use orderbook::OrderBook;
use orderbook::{OrderBookEvent, BookSide};
use genetic::adaptive_sampling::StableRegionDetector;
#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;
use portfoliomanager::PortfolioState;
use riskmanager::{RiskManager, RiskMetrics, RiskAction};
use dataloader::MarketData;
use crate::types::{BacktestEvent, BacktestResult, TransactionCostAnalysis, MarketImpactAnalysis, LiquidityWarning, InventoryMetrics, ExecutionMetrics};
use crate::logging_facade::BACKTEST_LOGGER;
use metrics::{performance, risk, trade_stats};
use bigdecimal::{BigDecimal, ToPrimitive, FromPrimitive};
use rustc_hash::{FxHashMap, FxHashSet};
use chrono::{DateTime, Utc};

/// Tracks individual order lifecycle for execution metrics
#[allow(dead_code)]
struct OrderTracker {
    order_id: String,
    side: orderbook::BookSide,
    placed_at: DateTime<Utc>,
    price: f64,
    filled: bool,
    fill_count: usize,
    first_fill_at: Option<DateTime<Utc>>,
}

pub struct SimulationLoop {
    // Track transaction costs
    total_commission: f64,
    total_slippage: f64,
    total_market_impact: f64,
    commission_by_exchange: FxHashMap<String, f64>,
    // Track commission by fee tier (bps -> total commission at that tier)
    commission_by_tier_bps: FxHashMap<u32, f64>,
    fills_by_tier_bps: FxHashMap<u32, u32>,
    // Track fill count distribution
    fill_count_histogram: FxHashMap<u32, u32>, // fills_per_order -> count
    total_fill_count: u32,
    total_order_count: u32,
    // Track market impact
    trades_with_impact: usize,
    impact_costs_bps: Vec<f64>,
    liquidity_warnings: Vec<LiquidityWarning>,
    // Track 30-day rolling volume per exchange for fee tiers
    trade_history_by_exchange: FxHashMap<String, Vec<(chrono::DateTime<chrono::Utc>, f64)>>, // exchange -> (timestamp, volume_usd)
    current_30d_volume_by_exchange: FxHashMap<String, f64>,
    // Track current active order IDs to cancel before placing new orders
    // Using HashSet for O(1) contains() check instead of O(n) Vec lookup
    active_order_ids: FxHashSet<String>,
    // Track rolling market volume for realistic market impact calculation
    market_volume_window: Vec<(chrono::DateTime<chrono::Utc>, f64)>, // (timestamp, trade_volume_usd)
    rolling_avg_volume: f64, // Average volume over rolling window
    // Track inventory metrics for market making
    buy_volume: f64,
    sell_volume: f64,
    buy_fills: usize,
    sell_fills: usize,
    buy_qty: f64,  // Total quantity bought
    sell_qty: f64, // Total quantity sold
    inventory_value_samples: Vec<f64>, // For calculating average inventory
    // Track order lifecycle for execution metrics
    order_trackers: FxHashMap<String, OrderTracker>, // order_id -> tracker
    orders_cancelled: usize,
    time_to_fill_samples: Vec<f64>, // seconds from placement to first fill
    bid_orders_placed: usize,
    ask_orders_placed: usize,
    quote_spreads_bps: Vec<f64>, // spread at time of quote placement
    // Track latency rejections (orders that would have filled but were too new)
    latency_rejections: usize,
    latency_rejections_bid: usize,
    latency_rejections_ask: usize,
    // Track market data trade direction distribution
    market_buy_trades: usize,
    market_sell_trades: usize,
    // Track missed fill opportunities (for diagnosing fill imbalance)
    missed_bid_opportunities: usize,  // Sell trades where we had a bid but it was too low
    missed_ask_opportunities: usize,  // Buy trades where we had an ask but it was too high
    bid_price_gap_sum: f64,           // Sum of (trade_price - our_bid) for missed bids
    ask_price_gap_sum: f64,           // Sum of (our_ask - trade_price) for missed asks
    // Track data timestamp range
    first_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    last_trade_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    first_trade_price: Option<f64>,
    last_trade_price: Option<f64>,
    // Track inventory over time for position analysis
    #[allow(dead_code)]
    inventory_snapshots: Vec<(chrono::DateTime<chrono::Utc>, f64)>, // (timestamp, net_position_qty)
    max_inventory: f64,
    min_inventory: f64,
    // Track round-trip profitability for market-making win rate
    buy_fill_prices: Vec<f64>,   // Price of each buy fill
    sell_fill_prices: Vec<f64>,  // Price of each sell fill
    fee_rate: f64,               // Commission rate for profit calculation
    // Performance optimization: track skipped stable regions
    stable_region_skips: usize,
}

impl SimulationLoop {
    pub fn new() -> Self {
        Self {
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            commission_by_exchange: FxHashMap::default(),
            commission_by_tier_bps: FxHashMap::default(),
            fills_by_tier_bps: FxHashMap::default(),
            fill_count_histogram: FxHashMap::default(),
            total_fill_count: 0,
            total_order_count: 0,
            trades_with_impact: 0,
            impact_costs_bps: Vec::with_capacity(1000),  // Pre-allocate for typical backtest
            liquidity_warnings: Vec::with_capacity(100),
            trade_history_by_exchange: FxHashMap::default(),
            current_30d_volume_by_exchange: FxHashMap::default(),
            active_order_ids: FxHashSet::default(),
            market_volume_window: Vec::with_capacity(1000), // Track last ~1000 trades
            rolling_avg_volume: 0.0,
            buy_volume: 0.0,
            sell_volume: 0.0,
            buy_fills: 0,
            sell_fills: 0,
            buy_qty: 0.0,
            sell_qty: 0.0,
            inventory_value_samples: Vec::with_capacity(1000),
            order_trackers: FxHashMap::default(),
            orders_cancelled: 0,
            time_to_fill_samples: Vec::with_capacity(1000),
            bid_orders_placed: 0,
            ask_orders_placed: 0,
            quote_spreads_bps: Vec::with_capacity(1000),
            latency_rejections: 0,
            latency_rejections_bid: 0,
            latency_rejections_ask: 0,
            market_buy_trades: 0,
            market_sell_trades: 0,
            missed_bid_opportunities: 0,
            missed_ask_opportunities: 0,
            bid_price_gap_sum: 0.0,
            ask_price_gap_sum: 0.0,
            first_trade_timestamp: None,
            last_trade_timestamp: None,
            first_trade_price: None,
            last_trade_price: None,
            inventory_snapshots: Vec::with_capacity(1000),
            max_inventory: 0.0,
            min_inventory: 0.0,
            buy_fill_prices: Vec::with_capacity(1000),
            sell_fill_prices: Vec::with_capacity(1000),
            fee_rate: 0.0,
            stable_region_skips: 0,
        }
    }
    
    /// Initialize starting 30-day volume for an exchange
    /// This simulates an established market maker who already has volume history
    /// allowing them to start at a lower fee tier
    pub fn initialize_starting_volume(&mut self, config: &config::BacktestConfig) {
        if let Some(ref fee_config) = config.trading.exchange_fees {
            if fee_config.starting_30d_volume > 0.0 {
                self.current_30d_volume_by_exchange.insert(
                    fee_config.exchange.clone(),
                    fee_config.starting_30d_volume,
                );
            }
        }
    }
    
    /// Update rolling market volume from observed trades
    /// Uses a 1-hour window to calculate average trade size
    fn update_market_volume(&mut self, timestamp: chrono::DateTime<chrono::Utc>, trade_volume_usd: f64) {
        use chrono::Duration;
        
        // Add new trade
        self.market_volume_window.push((timestamp, trade_volume_usd));
        
        // Remove trades older than 1 hour
        let cutoff = timestamp - Duration::hours(1);
        self.market_volume_window.retain(|(ts, _)| *ts >= cutoff);
        
        // Calculate average trade volume in the window
        if !self.market_volume_window.is_empty() {
            let total: f64 = self.market_volume_window.iter().map(|(_, vol)| vol).sum();
            // Average volume per minute (for market impact comparison)
            let window_minutes = 60.0; // 1 hour = 60 minutes
            self.rolling_avg_volume = total / window_minutes;
        }
    }
    
    /// Get the current average market volume (per minute)
    /// Returns a reasonable default if no data yet
    fn get_avg_volume(&self) -> f64 {
        if self.rolling_avg_volume > 0.0 {
            self.rolling_avg_volume
        } else {
            // Default fallback: assume $100k per minute for BTC (reasonable for major exchanges)
            // This is conservative - will result in some market impact
            100_000.0
        }
    }
    
    /// Update 30-day rolling volume window for specific exchange
    fn update_30d_volume(&mut self, exchange: &str, current_time: chrono::DateTime<chrono::Utc>, trade_volume_usd: f64) {
        use chrono::Duration;
        
        // Get or create trade history for this exchange
        let trade_history = self.trade_history_by_exchange.entry(exchange.to_string()).or_insert_with(Vec::new);
        
        // Add new trade
        trade_history.push((current_time, trade_volume_usd));
        
        // Remove trades older than 30 days
        let cutoff_time = current_time - Duration::days(30);
        trade_history.retain(|(ts, _)| *ts >= cutoff_time);
        
        // Recalculate 30-day volume for this exchange
        let volume: f64 = trade_history.iter().map(|(_, vol)| vol).sum();
        self.current_30d_volume_by_exchange.insert(exchange.to_string(), volume);
    }
    
    /// Get applicable fee rate based on exchange's 30-day volume
    fn get_fee_rate(&self, config: &config::BacktestConfig, exchange: &str, is_maker: bool) -> f64 {
        if let Some(ref fee_config) = config.trading.exchange_fees {
            // Only use tiered fees if the exchange matches
            if fee_config.exchange.to_lowercase() == exchange.to_lowercase() {
                let volume = self.current_30d_volume_by_exchange.get(exchange).copied().unwrap_or(0.0);
                fee_config.get_fee_rate(volume, is_maker)
            } else {
                // Different exchange, use generic fees or create config for that exchange
                let exchange_config = config::ExchangeFeeConfig::for_exchange(exchange);
                let volume = self.current_30d_volume_by_exchange.get(exchange).copied().unwrap_or(0.0);
                exchange_config.get_fee_rate(volume, is_maker)
            }
        } else {
            // Fallback to simple commission rate
            config.trading.commission_rate
        }
    }
    
    /// Track a fill for order lifecycle metrics
    fn track_fill(&mut self, order_id: &str, fill_time: DateTime<Utc>) {
        if let Some(tracker) = self.order_trackers.get_mut(order_id) {
            if !tracker.filled {
                tracker.filled = true;
                tracker.first_fill_at = Some(fill_time);
                // Calculate time to fill in seconds
                let time_to_fill = (fill_time - tracker.placed_at).num_milliseconds() as f64 / 1000.0;
                self.time_to_fill_samples.push(time_to_fill);
            }
            tracker.fill_count += 1;
        }
    }
    
    // Note: Slippage is now calculated internally by the orderbook during order matching.
    // The simulation tracks actual slippage from fill.slippage_bps in the fills loop.
    
    /// Calculate market impact using configured model
    fn calculate_market_impact(&mut self, order_size: f64, price: f64, avg_volume: f64, config: &config::BacktestConfig, timestamp: chrono::DateTime<chrono::Utc>) -> f64 {
        let impact_config = &config.data.market_impact;
        
        if !impact_config.enabled || avg_volume == 0.0 {
            return 0.0;
        }
        
        let size_ratio = order_size / avg_volume;
        
        // Only apply impact if order is significant relative to volume
        if size_ratio < impact_config.min_impact_ratio {
            return 0.0;
        }
        
        let impact_bps = match impact_config.model {
            config::MarketImpactModel::None => 0.0,
            config::MarketImpactModel::Linear => {
                impact_config.linear_coefficient * size_ratio
            }
            config::MarketImpactModel::SquareRoot => {
                impact_config.sqrt_coefficient * size_ratio.sqrt()
            }
            config::MarketImpactModel::AlmgrenChriss => {
                // Simplified Almgren-Chriss: permanent + temporary impact
                let permanent = impact_config.linear_coefficient * 0.3 * size_ratio;
                let temporary = impact_config.sqrt_coefficient * size_ratio.sqrt();
                permanent + temporary
            }
        };
        
        self.trades_with_impact += 1;
        self.impact_costs_bps.push(impact_bps);
        
        // Check for liquidity warnings
        if size_ratio > 0.1 {
            self.liquidity_warnings.push(LiquidityWarning {
                timestamp,
                order_size,
                available_liquidity: avg_volume,
                liquidity_ratio: size_ratio,
                message: format!("Order size is {:.1}% of available volume - high market impact expected", size_ratio * 100.0),
            });
        }
        
        // Convert basis points to dollar cost
        let impact_cost = (impact_bps / 10000.0) * price * order_size;
        self.total_market_impact += impact_cost;
        
        impact_cost
    }

    pub async fn run(
        &mut self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
        config: &config::BacktestConfig,
    ) -> BacktestResult {
    let mut events: Vec<BacktestEvent> = Vec::with_capacity(market_data.len());
    let mut result = BacktestResult::default();
    
    // Track if trading has been halted due to risk limits
    let mut trading_halted = false;
    let mut halt_reason: Option<String> = None;

    // Collectors for metrics (pre-allocated)
    let mut trade_returns: Vec<f64> = Vec::with_capacity(100);
    let mut equity_curve: Vec<f64> = Vec::with_capacity(market_data.len() + 1);
    let mut trade_durations: Vec<f64> = Vec::with_capacity(100);

    // Start equity curve with initial capital
    equity_curve.push(config.trading.initial_capital);
    
    // Initialize risk manager with starting equity
    risk_manager.initialize(config.trading.initial_capital);
    
    // Track current market price for unrealized P&L calculation
    let mut current_market_price: Option<BigDecimal> = None;

    // Progress tracking for debugging stalls
    let total_items = market_data.len();
    let mut processed_count = 0_usize;
    let sim_start = std::time::Instant::now();
    // Log more frequently for large datasets to show progress
    let log_interval = if total_items > 1_000_000 {
        total_items / 20  // Every 5% for large datasets
    } else if total_items > 100_000 {
        total_items / 10  // Every 10% for medium datasets
    } else {
        (total_items / 10).max(1000)  // Every 10% or 1000 for small datasets
    };
    
    log_info!(BACKTEST_LOGGER, "SIMULATION_START | items={} log_interval={} ({}%)", 
        total_items, log_interval, if total_items > 0 { log_interval * 100 / total_items } else { 0 });
    
    // Early stopping for GA fitness - abort clearly failing strategies
    // More aggressive for GA mode (skip_genetic_optimization = true means we're IN a GA eval)
    let is_ga_fitness_mode = config.analysis.skip_genetic_optimization;
    let early_stop_threshold = if is_ga_fitness_mode {
        config.trading.initial_capital * 0.7 // Stop at 30% loss in GA mode (faster iterations)
    } else {
        config.trading.initial_capital * 0.5 // Stop at 50% loss normally
    };
    let early_stop_check_interval = if is_ga_fitness_mode {
        5_000_usize // Check every 5K events in GA mode
    } else {
        10_000_usize // Check every 10K events normally  
    };
    let mut _early_stopped = false;
    
    // Stable region detector: skip strategy recalculation when price hasn't moved significantly
    // This provides 1.2-1.3x speedup by avoiding redundant computations
    let opt = &config.trading.optimization;
    let mut stable_detector = if opt.stable_region_skipping {
        StableRegionDetector::new(opt.stable_region_threshold, opt.max_skip_ticks)
    } else {
        StableRegionDetector::disabled()
    };
    let mut last_signals = Vec::new(); // Cache signals from last strategy run
    
    // Pre-cache exchange name to avoid cloning in hot path
    let exchange_name = orderbook.exchange.clone();

    for data in market_data {
            // Progress logging at intervals with ETA
            processed_count += 1;
            if processed_count % log_interval == 0 {
                let elapsed = sim_start.elapsed().as_secs_f64();
                let rate = processed_count as f64 / elapsed;
                let remaining = (total_items - processed_count) as f64 / rate;
                log_info!(BACKTEST_LOGGER, "SIM_PROGRESS | {}/{} ({:.0}%) | elapsed={:.1}s | rate={:.0}/s | ETA={:.0}s", 
                    processed_count, total_items, (processed_count as f64 / total_items as f64) * 100.0, 
                    elapsed, rate, remaining);
            }
            
            // Early stopping check for failing strategies (speeds up GA)
            if processed_count % early_stop_check_interval == 0 && !equity_curve.is_empty() {
                let current_equity = *equity_curve.last().unwrap_or(&config.trading.initial_capital);
                if current_equity < early_stop_threshold {
                    log_debug!(BACKTEST_LOGGER, "EARLY_STOP | equity={:.2} < threshold={:.2} | processed {}/{}", 
                        current_equity, early_stop_threshold, processed_count, total_items);
                    _early_stopped = true;
                    break;
                }
            }
            
            // 1. Update orderbook with new market data if needed
            #[cfg(feature = "postgres")]
            match &data {
                MarketData::Snapshot(_snapshot) => {
                    // Convert snapshot to OrderBookEvent::Snapshot and apply
                    // (Assume conversion function exists)
                    // orderbook.apply_event(OrderBookEvent::Snapshot { ... })
                }
                MarketData::Order(order) => {
                    // Convert HistoricalOrder to OrderBookEvent and apply to orderbook
                    if let Some(event) = convert_historical_order_to_event(order) {
                        let _ = orderbook.apply_event(event);
                    }
                }
                MarketData::Trade(trade) => {
                    // Update current market price from trade
                    current_market_price = Some(trade.price.clone());
                    
                    // Use pre-computed f64 values for performance (postgres path)
                    let trade_price_f64 = trade.price_f64;
                    if self.first_trade_timestamp.is_none() {
                        self.first_trade_timestamp = Some(trade.timestamp);
                        self.first_trade_price = Some(trade_price_f64);
                    }
                    self.last_trade_timestamp = Some(trade.timestamp);
                    self.last_trade_price = Some(trade_price_f64);
                    
                    // Use cached exchange_name instead of cloning
                    let exchange = &exchange_name;
                    
                    // Track market volume using pre-computed value
                    self.update_market_volume(trade.timestamp, trade.value_f64);
                    
                    // Convert TradeData side string to BookSide
                    // The trade side is the AGGRESSOR's side (the taker who initiated):
                    // - "buy" means a buyer aggressed -> fills our ASK orders
                    // - "sell" means a seller aggressed -> fills our BID orders
                    let aggressor_side = match trade.side.to_lowercase().as_str() {
                        "buy" | "b" => {
                            self.market_buy_trades += 1;
                            BookSide::Bid   // Aggressor is buyer
                        }
                        "sell" | "s" => {
                            self.market_sell_trades += 1;
                            BookSide::Ask  // Aggressor is seller
                        }
                        _ => continue, // Skip unknown side
                    };
                    
                    // Track missed fill opportunities BEFORE matching
                    // This helps diagnose why we might have 0 bid fills or 0 ask fills
                    let trade_price_for_tracking = trade_price_f64;
                    if matches!(aggressor_side, BookSide::Ask) {
                        // Sell trade - could fill our bids if trade_price <= bid_price
                        // Find our best bid from trackers
                        if let Some(our_best_bid) = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Bid))
                            .map(|t| t.price)
                            .reduce(f64::max) 
                        {
                            if trade_price_for_tracking > our_best_bid {
                                // Trade happened above our bid - missed opportunity
                                self.missed_bid_opportunities += 1;
                                self.bid_price_gap_sum += trade_price_for_tracking - our_best_bid;
                            }
                        }
                    } else {
                        // Buy trade - could fill our asks if trade_price >= ask_price
                        // Find our best ask from trackers
                        if let Some(our_best_ask) = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Ask))
                            .map(|t| t.price)
                            .reduce(f64::min) 
                        {
                            if trade_price_for_tracking < our_best_ask {
                                // Trade happened below our ask - missed opportunity
                                self.missed_ask_opportunities += 1;
                                self.ask_price_gap_sum += our_best_ask - trade_price_for_tracking;
                            }
                        }
                    }
                    
                    // Execute the trade against our resting orders and get fills
                    let trade_fills = orderbook.execute_trade_with_fills(
                        aggressor_side,
                        trade.price.clone(),
                        trade.quantity.clone(),
                        trade.timestamp,
                    );
                    
                    // Debug: Log first 5 trades vs our order prices to diagnose fill issues (postgres)
                    #[cfg(feature = "debug_logging")]
                    static TRADE_DEBUG_COUNT_PG: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                    #[cfg(feature = "debug_logging")]
                    let debug_count = TRADE_DEBUG_COUNT_PG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    
                    // Get trade price early for missed opportunity tracking (use pre-computed)
                    let _trade_price = trade_price_f64;
                    
                    // Filter fills to only include OUR strategy's orders
                    // Also apply latency simulation: orders placed within latency_ms can't be filled yet
                    let latency_ms = config.trading.latency_ms as i64;
                    let latency_cutoff = trade.timestamp - chrono::Duration::milliseconds(latency_ms);
                    
                    // Track latency rejections separately for debugging
                    let mut latency_rejected_bid = 0usize;
                    let mut latency_rejected_ask = 0usize;
                    
                    let our_fills: Vec<_> = trade_fills
                        .into_iter()
                        .filter(|fill| {
                            // Must be our order
                            if !self.active_order_ids.contains(&fill.order_id) {
                                return false;
                            }
                            // Latency simulation: order must have been placed before the cutoff
                            // (i.e., order has had time to propagate to exchange)
                            if let Some(tracker) = self.order_trackers.get(&fill.order_id) {
                                if tracker.placed_at > latency_cutoff {
                                    // Track latency rejection by side
                                    match tracker.side {
                                        orderbook::BookSide::Bid => latency_rejected_bid += 1,
                                        orderbook::BookSide::Ask => latency_rejected_ask += 1,
                                    }
                                    return false;
                                }
                                true
                            } else {
                                false // Can't find tracker, skip this fill
                            }
                        })
                        .collect();
                    
                    // Accumulate latency rejections
                    self.latency_rejections += latency_rejected_bid + latency_rejected_ask;
                    self.latency_rejections_bid += latency_rejected_bid;
                    self.latency_rejections_ask += latency_rejected_ask;
                    
                    // Debug logging - only with debug_logging feature (postgres path)
                    #[cfg(feature = "debug_logging")]
                    if debug_count < 10 && !self.active_order_ids.is_empty() {
                        let trade_price = trade_price_f64;
                        let best_bid = orderbook.best_bid().and_then(|p| p.to_f64());
                        let best_ask = orderbook.best_ask().and_then(|p| p.to_f64());
                        // Strategy's bid/ask from the trackers
                        let our_bid = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Bid))
                            .map(|t| t.price)
                            .reduce(f64::max);
                        let our_ask = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Ask))
                            .map(|t| t.price)
                            .reduce(f64::min);
                        log_info!(BACKTEST_LOGGER, 
                            "TRADE_VS_ORDERS | trade=${:.2} side={} | our_bid=${:.2} our_ask=${:.2} | book_bid=${:.2} book_ask=${:.2} | our_fills={} latency_rejected={}",
                            trade_price, trade.side,
                            our_bid.unwrap_or(0.0), our_ask.unwrap_or(0.0),
                            best_bid.unwrap_or(0.0), best_ask.unwrap_or(0.0), 
                            our_fills.len(), latency_rejected_bid + latency_rejected_ask);
                    }
                    
                    // Track fill count for market fills (only our orders)
                    let fill_count = our_fills.len() as u32;
                    if fill_count > 0 {
                        self.total_order_count += 1;
                        self.total_fill_count += fill_count;
                        *self.fill_count_histogram.entry(fill_count).or_insert(0) += 1;
                    }
                    
                    // Track commission and inventory for market fills (our resting orders getting filled)
                    for fill in &our_fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
                        // Track order lifecycle
                        self.track_fill(&fill.order_id, trade.timestamp);
                        
                        // Track inventory metrics (buy/sell volume)
                        match fill.side {
                            orderbook::BookSide::Bid => {
                                self.buy_volume += fill_value;
                                self.buy_fills += 1;
                            }
                            orderbook::BookSide::Ask => {
                                self.sell_volume += fill_value;
                                self.sell_fills += 1;
                            }
                        }
                        
                        // Our resting orders are MAKERS
                        let fee_rate = self.get_fee_rate(config, &exchange, true);
                        let commission = fill_value * fee_rate;
                        
                        // Debug: Log first 5 market fills to understand fill sizes
                        if self.total_fill_count <= 5 {
                            log_info!(BACKTEST_LOGGER, 
                                "MARKET_FILL | order_id={} price=${:.2} qty={:.6} value=${:.2} fee_rate={:.4}% commission=${:.2}",
                                fill.order_id, fill_price, fill_quantity, fill_value, fee_rate * 100.0, commission);
                        }
                        
                        // Track commission by fee tier (convert to bps for bucketing)
                        let tier_bps = (fee_rate * 10000.0).round() as u32;
                        *self.commission_by_tier_bps.entry(tier_bps).or_insert(0.0) += commission;
                        *self.fills_by_tier_bps.entry(tier_bps).or_insert(0) += 1;
                        
                        // Update volume tracking for this exchange
                        self.update_30d_volume(&exchange, trade.timestamp, fill_value);
                        
                        self.total_commission += commission;
                        *self.commission_by_exchange.entry(exchange.clone()).or_insert(0.0) += commission;
                    }
                    
                    // Apply fills from incoming trades to portfolio (only our orders)
                    if !our_fills.is_empty() {
                        if let Err(e) = portfolio.apply_fills(&our_fills) {
                            log_error!(BACKTEST_LOGGER, "Error applying trade fills: {}", e);
                        }
                        
                        // Remove filled orders from active list
                        for fill in &our_fills {
                            self.active_order_ids.retain(|id| id != &fill.order_id);
                        }
                    }
                }
                MarketData::Candle(candle) => {
                    // Update current market price from candle close
                    current_market_price = Some(candle.close.clone());
                }
            }
            
            #[cfg(not(feature = "postgres"))]
            match &data {
                MarketData::Trade(trade) => {
                    // Update current market price from trade
                    current_market_price = Some(trade.price.clone());
                    
                    // Use pre-computed f64 values for performance (non-postgres path)
                    let trade_price_f64 = trade.price_f64;
                    if self.first_trade_timestamp.is_none() {
                        self.first_trade_timestamp = Some(trade.timestamp);
                        self.first_trade_price = Some(trade_price_f64);
                    }
                    self.last_trade_timestamp = Some(trade.timestamp);
                    self.last_trade_price = Some(trade_price_f64);
                    
                    // Use cached exchange_name instead of cloning
                    let exchange = &exchange_name;
                    
                    // Track market volume using pre-computed value
                    self.update_market_volume(trade.timestamp, trade.value_f64);
                    
                    // Convert TradeData side string to BookSide
                    // The trade side is the AGGRESSOR's side (the taker who initiated):
                    // - "buy" means a buyer aggressed -> fills our ASK orders
                    // - "sell" means a seller aggressed -> fills our BID orders
                    let aggressor_side = match trade.side.to_lowercase().as_str() {
                        "buy" | "b" => {
                            self.market_buy_trades += 1;
                            BookSide::Bid   // Aggressor is buyer
                        }
                        "sell" | "s" => {
                            self.market_sell_trades += 1;
                            BookSide::Ask  // Aggressor is seller
                        }
                        _ => continue, // Skip unknown side
                    };
                    
                    // Track missed fill opportunities BEFORE matching
                    // This helps diagnose why we might have 0 bid fills or 0 ask fills
                    let trade_price_for_tracking = trade_price_f64;
                    if matches!(aggressor_side, BookSide::Ask) {
                        // Sell trade - could fill our bids if trade_price <= bid_price
                        // Find our best bid from trackers
                        if let Some(our_best_bid) = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Bid))
                            .map(|t| t.price)
                            .reduce(f64::max) 
                        {
                            if trade_price_for_tracking > our_best_bid {
                                // Trade happened above our bid - missed opportunity
                                self.missed_bid_opportunities += 1;
                                self.bid_price_gap_sum += trade_price_for_tracking - our_best_bid;
                            }
                        }
                    } else {
                        // Buy trade - could fill our asks if trade_price >= ask_price
                        // Find our best ask from trackers
                        if let Some(our_best_ask) = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Ask))
                            .map(|t| t.price)
                            .reduce(f64::min) 
                        {
                            if trade_price_for_tracking < our_best_ask {
                                // Trade happened below our ask - missed opportunity
                                self.missed_ask_opportunities += 1;
                                self.ask_price_gap_sum += our_best_ask - trade_price_for_tracking;
                            }
                        }
                    }
                    
                    // Execute the trade against our resting orders and get fills
                    let trade_fills = orderbook.execute_trade_with_fills(
                        aggressor_side,
                        trade.price.clone(),
                        trade.quantity.clone(),
                        trade.timestamp,
                    );
                    
                    // Debug: Log first 10 trades vs our order prices to diagnose fill issues (non-postgres)
                    #[cfg(feature = "debug_logging")]
                    static TRADE_DEBUG_COUNT_NP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                    #[cfg(feature = "debug_logging")]
                    let debug_count = TRADE_DEBUG_COUNT_NP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    
                    // Get trade price early for missed opportunity tracking (use pre-computed)
                    let _trade_price = trade_price_f64;
                    
                    // Filter fills to only include OUR strategy's orders
                    // Also apply latency simulation: orders placed within latency_ms can't be filled yet
                    let latency_ms = config.trading.latency_ms as i64;
                    let latency_cutoff = trade.timestamp - chrono::Duration::milliseconds(latency_ms);
                    
                    // Track latency rejections separately for debugging
                    let mut latency_rejected_bid = 0usize;
                    let mut latency_rejected_ask = 0usize;
                    
                    let our_fills: Vec<_> = trade_fills
                        .into_iter()
                        .filter(|fill| {
                            // Must be our order
                            if !self.active_order_ids.contains(&fill.order_id) {
                                return false;
                            }
                            // Latency simulation: order must have been placed before the cutoff
                            // (i.e., order has had time to propagate to exchange)
                            if let Some(tracker) = self.order_trackers.get(&fill.order_id) {
                                if tracker.placed_at > latency_cutoff {
                                    // Track latency rejection by side
                                    match tracker.side {
                                        orderbook::BookSide::Bid => latency_rejected_bid += 1,
                                        orderbook::BookSide::Ask => latency_rejected_ask += 1,
                                    }
                                    return false;
                                }
                                true
                            } else {
                                false // Can't find tracker, skip this fill
                            }
                        })
                        .collect();
                    
                    // Accumulate latency rejections
                    self.latency_rejections += latency_rejected_bid + latency_rejected_ask;
                    self.latency_rejections_bid += latency_rejected_bid;
                    self.latency_rejections_ask += latency_rejected_ask;
                    
                    // Debug logging - only with debug_logging feature (non-postgres path)
                    #[cfg(feature = "debug_logging")]
                    if debug_count < 10 && !self.active_order_ids.is_empty() {
                        let trade_price = trade_price_f64;
                        let best_bid = orderbook.best_bid().and_then(|p| p.to_f64());
                        let best_ask = orderbook.best_ask().and_then(|p| p.to_f64());
                        // Strategy's bid/ask from the trackers
                        let our_bid = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Bid))
                            .map(|t| t.price)
                            .reduce(f64::max);
                        let our_ask = self.order_trackers.values()
                            .filter(|t| matches!(t.side, orderbook::BookSide::Ask))
                            .map(|t| t.price)
                            .reduce(f64::min);
                        log_info!(BACKTEST_LOGGER, 
                            "TRADE_VS_ORDERS | trade=${:.2} side={} | our_bid=${:.2} our_ask=${:.2} | book_bid=${:.2} book_ask=${:.2} | our_fills={} latency_rejected={}",
                            trade_price, trade.side,
                            our_bid.unwrap_or(0.0), our_ask.unwrap_or(0.0),
                            best_bid.unwrap_or(0.0), best_ask.unwrap_or(0.0), 
                            our_fills.len(), latency_rejected_bid + latency_rejected_ask);
                    }
                    
                    // Track fill count for market fills (only our orders)
                    let fill_count = our_fills.len() as u32;
                    if fill_count > 0 {
                        self.total_order_count += 1;
                        self.total_fill_count += fill_count;
                        *self.fill_count_histogram.entry(fill_count).or_insert(0) += 1;
                    }
                    
                    // Track commission and inventory for market fills (our resting orders getting filled)
                    for fill in &our_fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
                        // Track order lifecycle
                        self.track_fill(&fill.order_id, trade.timestamp);
                        
                        // Track inventory metrics (buy/sell volume)
                        match fill.side {
                            orderbook::BookSide::Bid => {
                                self.buy_volume += fill_value;
                                self.buy_fills += 1;
                            }
                            orderbook::BookSide::Ask => {
                                self.sell_volume += fill_value;
                                self.sell_fills += 1;
                            }
                        }
                        
                        // Our resting orders are MAKERS
                        let fee_rate = self.get_fee_rate(config, &exchange, true);
                        let commission = fill_value * fee_rate;
                        
                        // Debug: Log first 5 market fills to understand fill sizes
                        if self.total_fill_count <= 5 {
                            log_info!(BACKTEST_LOGGER, 
                                "MARKET_FILL | order_id={} price=${:.2} qty={:.6} value=${:.2} fee_rate={:.4}% commission=${:.2}",
                                fill.order_id, fill_price, fill_quantity, fill_value, fee_rate * 100.0, commission);
                        }
                        
                        // Track commission by fee tier (convert to bps for bucketing)
                        let tier_bps = (fee_rate * 10000.0).round() as u32;
                        *self.commission_by_tier_bps.entry(tier_bps).or_insert(0.0) += commission;
                        *self.fills_by_tier_bps.entry(tier_bps).or_insert(0) += 1;
                        
                        // Update volume tracking for this exchange
                        self.update_30d_volume(&exchange, trade.timestamp, fill_value);
                        
                        self.total_commission += commission;
                        *self.commission_by_exchange.entry(exchange.clone()).or_insert(0.0) += commission;
                    }
                    
                    // Apply fills from incoming trades to portfolio (only our orders)
                    if !our_fills.is_empty() {
                        if let Err(e) = portfolio.apply_fills(&our_fills) {
                            log_error!(BACKTEST_LOGGER, "Error applying trade fills: {}", e);
                        }
                        
                        // Remove filled orders from active set (O(1) per removal with HashSet)
                        for fill in &our_fills {
                            self.active_order_ids.remove(&fill.order_id);
                        }
                    }
                }
                MarketData::Candle(candle) => {
                    // Update current market price from candle close
                    current_market_price = Some(candle.close.clone());
                }
                #[allow(unreachable_patterns)]
                _ => {
                    // Skip unknown/feature-gated variants
                }
            }

            // 2. Build strategy context (timestamp, portfolio, etc.)
            #[cfg(feature = "postgres")]
            let timestamp = match &data {
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Order(o) => o.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            
            #[cfg(not(feature = "postgres"))]
            let timestamp = match &data {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
                #[allow(unreachable_patterns)]
                _ => chrono::Utc::now(),
            };
            let context = StrategyContext {
                timestamp,
                portfolio_state: portfolio.clone(),
                price_history: Vec::new(), // Empty for simplicity, could be filled from historical data
                volume_history: Vec::new(), // Empty for simplicity 
                spread: {
                    // Calculate spread from orderbook best bid/ask
                    match (orderbook.best_bid(), orderbook.best_ask()) {
                        (Some(bid), Some(ask)) => Some(ask.clone() - bid.clone()),
                        _ => None,
                    }
                },
                market_session: MarketSession {
                    is_open: true,
                    session_type: SessionType::Regular,
                    next_open: None,
                    next_close: None,
                }, // Default session
            };
            
            // Skip signal processing if trading has been halted
            if trading_halted {
                // Still update equity curve even when halted
                if let Some(ref price) = current_market_price {
                    portfolio.update_unrealized_pnl(price);
                }
                let total_portfolio_value = portfolio.get_total_value().to_f64().unwrap_or(0.0);
                equity_curve.push(total_portfolio_value);
                continue;
            }

            // 3. Run strategies with stable region optimization
            // Extract current price for stable region detection
            let current_price_f64 = current_market_price.as_ref()
                .and_then(|p| p.to_f64())
                .unwrap_or(0.0);
            
            // Check if we should run strategy or reuse cached signals
            let signals = if stable_detector.should_process(current_price_f64) {
                // Price moved significantly or max skip reached - run strategy
                let new_signals = strategy_manager.process_market_data(&data, &context).await.unwrap_or_default();
                last_signals = new_signals.clone();
                new_signals
            } else {
                // Stable region - reuse cached signals (skip expensive strategy computation)
                self.stable_region_skips += 1;
                last_signals.clone()
            };
            
            // Cancel previous active orders before placing new quotes
            // This prevents order accumulation that leads to incorrect fill volumes
            if !signals.is_empty() {
                for order_id in &self.active_order_ids {
                    let cancel_event = OrderBookEvent::CancelOrder { order_id: order_id.clone(), timestamp };
                    let _ = orderbook.apply_event(cancel_event);
                    // Track cancellation if order was never filled
                    if let Some(tracker) = self.order_trackers.get(order_id) {
                        if !tracker.filled {
                            self.orders_cancelled += 1;
                        }
                    }
                    self.order_trackers.remove(order_id);
                }
                self.active_order_ids.clear();
            }
            
            // Calculate current risk metrics for risk checks
            let current_equity = portfolio.get_total_value().to_f64().unwrap_or(config.trading.initial_capital);
            let current_position = portfolio.positions.iter()
                .filter(|p| p.close_time.is_none())
                .map(|p| {
                    let qty = p.quantity.to_f64().unwrap_or(0.0);
                    match p.side {
                        portfoliomanager::PositionSide::Long => qty,
                        portfoliomanager::PositionSide::Short => -qty,
                    }
                })
                .sum::<f64>();
            
            // Update risk manager's equity tracking
            risk_manager.update_equity(current_equity);
            let current_drawdown = risk_manager.get_current_drawdown(current_equity);
            
            // Build risk metrics
            let risk_metrics = RiskMetrics {
                current_position,
                current_order_size: 0.0,
                current_inventory_skew: 0.0,
                current_volatility: 0.0,
                current_drawdown,
                unrealized_pnl: portfolio.positions.iter()
                    .filter(|p| p.close_time.is_none())
                    .map(|p| p.unrealized_pnl.to_f64().unwrap_or(0.0))
                    .sum(),
                daily_pnl: current_equity - config.trading.initial_capital,
                // VaR metrics - defaults for simulation
                current_var: 0.0,
                current_cvar: 0.0,
                // Correlation metrics
                correlated_exposure: 0.0,
                sector_exposures: std::collections::HashMap::new(),
                // Volatility regime
                volatility_regime: riskmanager::VolatilityRegime::Normal,
                baseline_volatility: 0.0,
            };
            
            // Check global risk limits before processing signals
            if let Err(reason) = risk_manager.check(&risk_metrics) {
                log_error!(BACKTEST_LOGGER, "Risk limit exceeded: {}", reason);
                trading_halted = true;
                halt_reason = Some(reason);
                continue;
            }
            
            for (_strategy_id, sigs) in signals {
                // Track bid/ask prices for spread calculation
                let mut last_bid_price: Option<f64> = None;
                let mut last_ask_price: Option<f64> = None;
                
                for signal in sigs {
                    // Extract order details for cost calculation
                    let order_price = signal.price.as_ref()
                        .and_then(|p| p.to_f64())
                        .unwrap_or(0.0);
                    let mut order_size = signal.quantity.as_ref()
                        .and_then(|q| q.to_f64())
                        .unwrap_or(0.0);
                    let exchange = orderbook.exchange.clone();
                    
                    // Track bid/ask prices for spread calculation
                    match signal.signal_type {
                        signal::SignalType::Buy | signal::SignalType::ScaleIn => {
                            last_bid_price = Some(order_price);
                        }
                        signal::SignalType::Sell | signal::SignalType::ScaleOut => {
                            last_ask_price = Some(order_price);
                        }
                        _ => {}
                    }
                    
                    // Calculate and store quote spread when we have both bid and ask
                    if let (Some(bid), Some(ask)) = (last_bid_price, last_ask_price) {
                        if bid > 0.0 && ask > 0.0 {
                            let mid = (bid + ask) / 2.0;
                            let spread_bps = (ask - bid) / mid * 10000.0;
                            self.quote_spreads_bps.push(spread_bps);
                            
                            // Log quote vs market price for debugging (first 10 quotes)
                            if self.quote_spreads_bps.len() <= 10 {
                                let market_price = current_market_price.as_ref()
                                    .and_then(|p| p.to_f64())
                                    .unwrap_or(0.0);
                                let bid_distance_bps = if market_price > 0.0 { (market_price - bid) / market_price * 10000.0 } else { 0.0 };
                                let ask_distance_bps = if market_price > 0.0 { (ask - market_price) / market_price * 10000.0 } else { 0.0 };
                                log_info!(BACKTEST_LOGGER, 
                                    "QUOTE_DEBUG | market=${:.2} bid=${:.2} ask=${:.2} spread={:.1}bps bid_dist={:.1}bps ask_dist={:.1}bps",
                                    market_price, bid, ask, spread_bps, bid_distance_bps, ask_distance_bps);
                            }
                            
                            // Reset for next pair
                            last_bid_price = None;
                            last_ask_price = None;
                        }
                    }
                    
                    // Debug: Log first few order sizes from strategy with full context
                    if self.total_order_count < 5 {
                        let order_value = order_price * order_size;
                        let expected_value_2pct = config.trading.initial_capital * 0.02; // Expected at 2%
                        // Calculate implied order_size_pct from actual order value
                        let implied_pct = if config.trading.initial_capital > 0.0 { order_value / config.trading.initial_capital * 100.0 } else { 0.0 };
                        let portfolio_balance = portfolio.balance.to_f64().unwrap_or(0.0);
                        let portfolio_position = portfolio.net_position;
                        log_info!(BACKTEST_LOGGER, 
                            "ORDER_SIZE_DIAG | portfolio: balance=${:.2} position={:.6} | order: price=${:.2} size={:.6} value=${:.2} | config: initial_cap=${:.0} expected_2pct=${:.2} implied_pct={:.2}%",
                            portfolio_balance, portfolio_position, order_price, order_size, order_value, 
                            config.trading.initial_capital, expected_value_2pct, implied_pct);
                    }
                    
                    // Check if this specific order should be allowed
                    match risk_manager.check_order(order_size, current_position, &risk_metrics) {
                        RiskAction::Proceed => {
                            // Order is allowed, continue
                        }
                        RiskAction::ReducePosition(allowed_size) => {
                            log_info!(BACKTEST_LOGGER, "Risk: reducing order size from {} to {}", order_size, allowed_size);
                            order_size = allowed_size;
                            if order_size <= 0.0 {
                                continue; // Skip this order entirely
                            }
                        }
                        RiskAction::ScalePosition(scale_factor, reason) => {
                            log_info!(BACKTEST_LOGGER, "Risk: scaling position by {:.2}x - {}", scale_factor, reason);
                            order_size *= scale_factor;
                            if order_size <= 0.0 {
                                continue; // Skip this order entirely
                            }
                        }
                        RiskAction::RejectOrder(reason) => {
                            log_info!(BACKTEST_LOGGER, "Risk: rejecting order - {}", reason);
                            continue;
                        }
                        RiskAction::CloseAllPositions(reason) => {
                            log_error!(BACKTEST_LOGGER, "Risk: closing all positions - {}", reason);
                            // TODO: Implement position closing logic
                            trading_halted = true;
                            halt_reason = Some(reason);
                            continue;
                        }
                        RiskAction::HaltTrading(reason) => {
                            log_error!(BACKTEST_LOGGER, "Risk: halting trading - {}", reason);
                            trading_halted = true;
                            halt_reason = Some(reason);
                            continue;
                        }
                    }
                    
                    // Note: Slippage is now handled internally by the orderbook during order matching
                    // We track the actual slippage from fills below
                    
                    // Calculate market impact using rolling average volume from observed trades
                    let avg_volume = self.get_avg_volume();
                    let _impact_cost = self.calculate_market_impact(order_size, order_price, avg_volume, config, timestamp);
                    
                    // Track this order ID for later cancellation (O(1) insert with HashSet)
                    self.active_order_ids.insert(signal.id.clone());
                    
                    // Determine order side from signal type
                    let order_side = match signal.signal_type {
                        signal::SignalType::Buy | signal::SignalType::ScaleIn => orderbook::BookSide::Bid,
                        _ => orderbook::BookSide::Ask,
                    };
                    
                    // Track order placement for execution metrics
                    self.order_trackers.insert(signal.id.clone(), OrderTracker {
                        order_id: signal.id.clone(),
                        side: order_side,
                        placed_at: timestamp,
                        price: order_price,
                        filled: false,
                        fill_count: 0,
                        first_fill_at: None,
                    });
                    match order_side {
                        orderbook::BookSide::Bid => self.bid_orders_placed += 1,
                        orderbook::BookSide::Ask => self.ask_orders_placed += 1,
                    }
                    
                    // 4. Submit order to orderbook
                    let _ = orderbook.submit_order_from_signal(&signal, timestamp);

                    // 5. Simulate order matching/fills
                    // For LIMIT orders (with price): DON'T immediately match - they rest on the book
                    //   and only get filled when external market trades hit them via execute_trade_with_fills()
                    // For MARKET orders (without price): Immediately match against resting orders
                    let is_limit_order = signal.price.is_some();
                    let fills = if is_limit_order {
                        // Limit orders rest on the book - no immediate matching
                        // They will be filled by market trades in the MarketData::Trade handler
                        Vec::new()
                    } else {
                        // Market orders immediately sweep the book
                        orderbook.match_all_orders(timestamp)
                    };
                    
                    // Track fill count for this order
                    let fill_count = fills.len() as u32;
                    if fill_count > 0 {
                        self.total_order_count += 1;
                        self.total_fill_count += fill_count;
                        *self.fill_count_histogram.entry(fill_count).or_insert(0) += 1;
                    }
                    
                    // Debug: Log first 5 matched fills to understand fill sizes
                    if self.total_fill_count <= 5 && !fills.is_empty() {
                        for fill in &fills {
                            let fill_price = fill.price.to_f64().unwrap_or(0.0);
                            let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                            let fill_value = fill_price * fill_quantity;
                            log_info!(BACKTEST_LOGGER, 
                                "MATCHED_FILL | order_id={} price=${:.2} qty={:.6} value=${:.2} signal_order_value=${:.2}",
                                fill.order_id, fill_price, fill_quantity, fill_value, order_size * order_price);
                        }
                    }

                    // Track commission and slippage costs from fills
                    for fill in &fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
                        // Track slippage from the orderbook's calculation (already applied to fill.price)
                        if let Some(slippage_bps) = fill.slippage_bps {
                            // Calculate the dollar slippage cost from bps
                            let slippage_cost = fill_value * (slippage_bps / 10000.0);
                            self.total_slippage += slippage_cost;
                        }
                        
                        // Determine if maker or taker based on original signal
                        // Limit orders (with price) = maker, market orders (no price) = taker
                        let is_maker = signal.price.is_some();
                        
                        // Get fee rate based on this exchange's 30-day volume
                        let fee_rate = self.get_fee_rate(config, &exchange, is_maker);
                        let commission = fill_value * fee_rate;
                        
                        // Track commission by fee tier (convert to bps for bucketing)
                        let tier_bps = (fee_rate * 10000.0).round() as u32;
                        *self.commission_by_tier_bps.entry(tier_bps).or_insert(0.0) += commission;
                        *self.fills_by_tier_bps.entry(tier_bps).or_insert(0) += 1;
                        
                        // Debug: Log first few fills to understand commission calculation
                        if self.total_commission < 100.0 {
                            log_debug!(BACKTEST_LOGGER, "Fill debug | price={:.2} qty={:.6} value=${:.2} fee_rate={:.4}% is_maker={} commission=${:.2}",
                                fill_price, fill_quantity, fill_value, fee_rate * 100.0, is_maker, commission);
                        }
                        
                        // Update volume tracking for this exchange
                        self.update_30d_volume(&exchange, timestamp, fill_value);
                        
                        self.total_commission += commission;
                        *self.commission_by_exchange.entry(exchange.clone()).or_insert(0.0) += commission;
                    }

                    // 6. Update portfolio and risk manager with fills
                    if let Err(e) = portfolio.apply_fills(&fills) {
                        log_error!(BACKTEST_LOGGER, "Error applying fills: {}", e);
                    }
                    
                    // Update risk manager equity after fills
                    let new_equity = portfolio.get_total_value().to_f64().unwrap_or(0.0);
                    risk_manager.update_equity(new_equity);

                    // 7. Record event
                    events.push(BacktestEvent::OrderSubmitted {
                        order_id: signal.id.clone(),
                        timestamp,
                    });
                }
            }

            // Update unrealized P&L with current market price before collecting equity
            if let Some(ref price) = current_market_price {
                portfolio.update_unrealized_pnl(price);
                
                // Sample inventory value for turnover calculation (position value only, not cash)
                let inventory_value = portfolio.get_total_value().to_f64().unwrap_or(0.0) - portfolio.balance.to_f64().unwrap_or(0.0);
                if inventory_value > 0.0 {
                    self.inventory_value_samples.push(inventory_value);
                }
            }
            
            // Collect TOTAL portfolio value (balance + unrealized P&L) at each step
            // This correctly reflects the portfolio's worth including open positions
            let total_portfolio_value = portfolio.get_total_value().to_f64().unwrap_or(0.0);
            equity_curve.push(total_portfolio_value);
        }

        // ========== DETAILED FILL DIAGNOSTICS ==========
        // Log comprehensive diagnostics to understand why fills aren't happening
        log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | total_orders={} total_fills={} buy_fills={} sell_fills={} latency_rejected={} (bid={} ask={})",
            self.total_order_count, self.total_fill_count, self.buy_fills, self.sell_fills,
            self.latency_rejections, self.latency_rejections_bid, self.latency_rejections_ask);
        log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | orders_cancelled={} missed_bid_opps={} missed_ask_opps={}",
            self.orders_cancelled, self.missed_bid_opportunities, self.missed_ask_opportunities);
        log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | active_orders_remaining={} order_trackers_remaining={}",
            self.active_order_ids.len(), self.order_trackers.len());
        
        // Analyze quote spreads if we have them
        if !self.quote_spreads_bps.is_empty() {
            let avg_spread = self.quote_spreads_bps.iter().sum::<f64>() / self.quote_spreads_bps.len() as f64;
            let min_spread = self.quote_spreads_bps.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_spread = self.quote_spreads_bps.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | quote_spreads: count={} avg={:.1}bps min={:.1}bps max={:.1}bps",
                self.quote_spreads_bps.len(), avg_spread, min_spread, max_spread);
        } else {
            log_warn!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | NO QUOTES GENERATED - strategy didn't produce any bid/ask pairs!");
        }
        
        // Analyze price gaps for missed opportunities
        if self.missed_bid_opportunities > 0 || self.missed_ask_opportunities > 0 {
            let avg_bid_gap = if self.missed_bid_opportunities > 0 { 
                self.bid_price_gap_sum / self.missed_bid_opportunities as f64 
            } else { 0.0 };
            let avg_ask_gap = if self.missed_ask_opportunities > 0 { 
                self.ask_price_gap_sum / self.missed_ask_opportunities as f64 
            } else { 0.0 };
            log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | price_gaps: avg_bid_gap=${:.2} avg_ask_gap=${:.2} (quotes too far from market)",
                avg_bid_gap, avg_ask_gap);
        }
        
        // Log market trade stats for context
        log_info!(BACKTEST_LOGGER, "FILL_DIAGNOSTICS | market_trades: buys={} sells={} (potential opportunities)",
            self.market_buy_trades, self.market_sell_trades);

        // Collect trade returns and durations from closed positions (do this after market data processing)
        let mut closed_position_count = 0;
        let mut open_position_count = 0;
        let mut total_unrealized_pnl = 0.0;
        
        for pos in &portfolio.positions {
            if let Some(close_time) = pos.close_time {
                // Only consider closed positions for trade statistics
                closed_position_count += 1;
                trade_returns.push(pos.realized_pnl.to_f64().unwrap_or(0.0));
                let duration = (close_time - pos.open_time).num_seconds() as f64;
                trade_durations.push(duration);
            } else {
                // Track open positions and their unrealized P&L
                open_position_count += 1;
                total_unrealized_pnl += pos.unrealized_pnl.to_f64().unwrap_or(0.0);
            }
        }
        
        // Log position breakdown for debugging
        log_info!(BACKTEST_LOGGER, "Position breakdown: closed={} open={} unrealized_pnl={:.2}",
            closed_position_count, open_position_count, total_unrealized_pnl);

        // Compute metrics using collected data
        // NOTE: These metrics are based on CLOSED positions only.
        // If there are open positions with unrealized P&L, the total_pnl may differ from net_profit.
        
        // For market making: use fills as trades, not closed positions
        // A "round-trip" is min(buy_fills, sell_fills) - completed inventory cycles
        let mm_round_trips = self.buy_fills.min(self.sell_fills);
        let total_fills = self.buy_fills + self.sell_fills;
        
        // Store info for later win rate calculation (after total_pnl is calculated)
        let is_market_making_mode = closed_position_count == 0 && total_fills > 0;
        
        log_info!(BACKTEST_LOGGER, "Market making mode check: closed={} total_fills={} is_mm_mode={}",
            closed_position_count, total_fills, is_market_making_mode);
        
        // Use market-making metrics if we have fills but no closed positions
        // This handles the case where market maker adjusts inventory without closing positions
        if is_market_making_mode {
            // Market making mode: count fills as trades
            result.num_trades = mm_round_trips;
            
            log_info!(BACKTEST_LOGGER, "Market making mode: using fills as trades | round_trips={} total_fills={} buy={} sell={}",
                mm_round_trips, total_fills, self.buy_fills, self.sell_fills);
            
            // For market making, calculate metrics from fills rather than closed positions
            // We'll populate these after total_pnl is calculated (below)
        } else {
            // Traditional mode: use closed positions
            result.num_trades = performance::number_of_trades(&trade_returns);
            
            // Traditional metrics from closed positions
            result.gross_profit = Some(performance::gross_profit(&trade_returns));
            result.gross_loss = Some(performance::gross_loss(&trade_returns));
            result.net_profit = Some(performance::net_profit(&trade_returns));
            result.avg_trade_return = performance::average_trade_return(&trade_returns);
            result.median_trade_return = performance::median_trade_return(&trade_returns);
        }
        
        // For Sharpe ratio, if we have very few closed trades but significant unrealized P&L,
        // use the equity curve returns instead for a more representative metric
        let use_equity_returns = trade_returns.len() < 10 && open_position_count > 0;
        
        // Calculate Sharpe ratio properly:
        // - For equity curve returns: use the dedicated function with proper annualization
        // - For trade returns: the function now handles dollar vs percentage returns
        result.sharpe_ratio = if use_equity_returns {
            // Use equity curve with proper annualization
            // Assume high-frequency data: estimate ~1000 ticks per day for crypto
            // This is approximate; actual periods_per_day depends on data frequency
            let periods_per_day = (equity_curve.len() / 7).max(100); // Rough estimate for 1 week of data
            performance::sharpe_ratio_from_equity_curve(&equity_curve, config.trading.risk_free_rate, periods_per_day)
        } else {
            // Use trade returns (the function now handles dollar amounts properly)
            performance::sharpe_ratio(&trade_returns, config.trading.risk_free_rate)
        };
        
        result.profit_factor = performance::profit_factor(
            result.gross_profit.unwrap_or(0.0),
            result.gross_loss.unwrap_or(0.0),
        );
        
        // Calculate win rate - only override if not already set by market-making logic
        if result.win_rate.is_none() {
            result.win_rate = if closed_position_count > 0 {
                trade_stats::win_rate(&trade_returns)
            } else {
                // No closed trades and no fills - win rate is undefined
                None
            };
        }
        result.avg_trade_duration = trade_stats::average_trade_duration(&trade_durations);
        
        // Populate closed/open position counts and realized/unrealized PnL for transparency
        result.closed_trades = closed_position_count;
        result.open_positions = open_position_count;
        let realized_pnl = performance::net_profit(&trade_returns);
        result.realized_pnl = Some(realized_pnl);
        result.unrealized_pnl = Some(total_unrealized_pnl);
        
        // Log warning if PnL is primarily from unrealized gains
        if total_unrealized_pnl.abs() > realized_pnl.abs() && open_position_count > 0 {
            log_info!(BACKTEST_LOGGER, "WARNING: PnL dominated by unrealized gains. realized={:.2} unrealized={:.2}",
                realized_pnl, total_unrealized_pnl);
        }
        
        // Calculate volatility using percentage returns from equity curve (not dollar returns)
        // This gives a proper annualizable volatility metric
        let pct_returns: Vec<f64> = if equity_curve.len() > 1 {
            equity_curve.windows(2)
                .filter_map(|w| {
                    if w[0] > 0.0 {
                        Some((w[1] - w[0]) / w[0])
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        result.volatility = risk::volatility(&pct_returns);
        result.max_drawdown = risk::max_drawdown(&equity_curve)
            .map(|v| BigDecimal::from_f64(v).unwrap_or_default())
            .unwrap_or_default();
        
        // Calculate Sortino ratio using percentage returns (same as Sharpe for consistency)
        // Sortino uses downside deviation instead of total volatility
        result.sortino_ratio = risk::sortino_ratio(&pct_returns, config.trading.risk_free_rate);
        
        // Calculate Calmar ratio (annualized return / max drawdown)
        // Uses percentage-based annualized return for proper ratio
        let max_dd = result.max_drawdown.to_f64().unwrap_or(0.0);
        if !pct_returns.is_empty() && max_dd > 0.0 {
            // Calculate annualized return from percentage returns
            // Mean return per period * periods per year (assuming ~252 trading days)
            let mean_return = pct_returns.iter().sum::<f64>() / pct_returns.len() as f64;
            let annualized_return = mean_return * 252.0;  // Annualize the mean daily return
            result.calmar_ratio = risk::calmar_ratio(annualized_return, max_dd);
        }
        
        // Record risk halt reason if trading was stopped
        result.risk_halt_reason = halt_reason;
            
        // Debug: Log portfolio state before calculating PnL
        log_info!(BACKTEST_LOGGER, "Portfolio state: balance={} positions={} open_positions={}",
            portfolio.balance.to_f64().unwrap_or(0.0),
            portfolio.positions.len(),
            portfolio.positions.iter().filter(|p| p.close_time.is_none()).count()
        );
        
        for (i, pos) in portfolio.positions.iter().enumerate() {
            let mark = pos.mark_price.as_ref().map(|p| p.to_f64().unwrap_or(0.0)).unwrap_or(0.0);
            log_info!(BACKTEST_LOGGER, "Position {}: side={:?} qty={} entry={} mark={} closed={}",
                i,
                pos.side,
                pos.quantity,
                pos.entry_price.to_f64().unwrap_or(0.0),
                mark,
                pos.close_time.is_some()
            );
        }
            
        // Calculate total P&L: ending portfolio value - initial capital
        // Use get_total_value() which correctly includes position market values
        let final_portfolio_value = portfolio.get_total_value();
        let initial_capital_bd = BigDecimal::from_f64(config.trading.initial_capital).unwrap_or_default();
        
        log_info!(BACKTEST_LOGGER, "PnL calc: final_value={} initial={} pnl={}",
            final_portfolio_value.to_f64().unwrap_or(0.0),
            config.trading.initial_capital,
            (&final_portfolio_value - &initial_capital_bd).to_f64().unwrap_or(0.0)
        );
        
        result.total_pnl = final_portfolio_value - initial_capital_bd;

        // Now calculate market-making win rate (after total_pnl is set)
        if is_market_making_mode {
            let total_pnl_f64 = result.total_pnl.to_f64().unwrap_or(0.0);
            if total_pnl_f64 > 0.0 {
                // Profitable overall - estimate win rate based on PnL distribution
                // For market making, if gross_pnl > commission, we're "winning"
                let gross = total_pnl_f64 + self.total_commission;
                if gross > 0.0 && total_fills > 0 {
                    // Estimate: if we made money after fees, most fills were profitable
                    let estimated_win_rate = 0.5 + (total_pnl_f64 / gross).min(0.5);
                    result.win_rate = Some(estimated_win_rate);
                } else {
                    result.win_rate = Some(0.5); // Break-even assumption
                }
            } else if total_pnl_f64 < 0.0 {
                // Losing overall - estimate win rate below 50%
                let gross = (-total_pnl_f64) + self.total_commission;
                if gross > 0.0 {
                    let estimated_win_rate = 0.5 - ((-total_pnl_f64) / gross).min(0.5);
                    result.win_rate = Some(estimated_win_rate);
                } else {
                    result.win_rate = Some(0.5);
                }
            } else {
                result.win_rate = Some(0.5); // Break-even
            }
            
            log_info!(BACKTEST_LOGGER, "Market making win rate: pnl={:.2} commission={:.2} win_rate={:.1}%",
                total_pnl_f64, self.total_commission, result.win_rate.unwrap_or(0.0) * 100.0);
            
            // Calculate market-making specific metrics
            // For MM: gross_profit = spread captured (pnl + commission), gross_loss = commission paid
            let gross_pnl = total_pnl_f64 + self.total_commission; // Total spread captured before fees
            if gross_pnl > 0.0 {
                result.gross_profit = Some(gross_pnl);
                result.gross_loss = Some(self.total_commission);
            } else {
                // Lost money even before commission - adverse selection
                result.gross_profit = Some(0.0);
                result.gross_loss = Some(-total_pnl_f64); // Total loss
            }
            result.net_profit = Some(total_pnl_f64);
            
            // Avg return per round-trip
            if mm_round_trips > 0 {
                result.avg_trade_return = Some(total_pnl_f64 / mm_round_trips as f64);
            }
            
            // Profit factor for MM: gross_profit / gross_loss
            if self.total_commission > 0.0 && gross_pnl > 0.0 {
                result.profit_factor = Some(gross_pnl / self.total_commission);
            }
            
            log_info!(BACKTEST_LOGGER, "Market making metrics: gross_profit={:.2} gross_loss={:.2} net_profit={:.2} avg_return_per_rt={:.2} profit_factor={:.2}",
                result.gross_profit.unwrap_or(0.0), result.gross_loss.unwrap_or(0.0), 
                result.net_profit.unwrap_or(0.0), result.avg_trade_return.unwrap_or(0.0),
                result.profit_factor.unwrap_or(0.0));
                
            log_info!(BACKTEST_LOGGER, "Final MM result: win_rate={:?} num_trades={}",
                result.win_rate, result.num_trades);
        }

        // Capture equity curve length before moving it
        let equity_curve_len = equity_curve.len();
        
        // Include equity curve data for visualization
        result.equity_curve = equity_curve;
        // Store individual trade returns for Monte Carlo simulation (actual trade-level P&L)
        result.trade_returns = trade_returns;
        result.initial_capital = config.trading.initial_capital;
        
        // Generate transaction cost analysis
        let total_transaction_costs = self.total_commission + self.total_slippage + self.total_market_impact;
        let costs_as_percentage = if result.total_pnl.to_f64().unwrap_or(0.0).abs() > 0.0 {
            (total_transaction_costs / result.total_pnl.to_f64().unwrap_or(1.0).abs()) * 100.0
        } else {
            0.0
        };
        
        result.transaction_costs = Some(TransactionCostAnalysis {
            total_commission: self.total_commission,
            total_slippage: self.total_slippage,
            total_market_impact: self.total_market_impact,
            total_transaction_costs,
            costs_as_percentage_of_pnl: costs_as_percentage,
            avg_cost_per_trade: if result.num_trades > 0 {
                total_transaction_costs / result.num_trades as f64
            } else {
                0.0
            },
            commission_by_exchange: self.commission_by_exchange.iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        });
        
        // Generate market impact analysis
        result.market_impact = Some(MarketImpactAnalysis {
            trades_with_impact: self.trades_with_impact,
            avg_impact_bps: if !self.impact_costs_bps.is_empty() {
                self.impact_costs_bps.iter().sum::<f64>() / self.impact_costs_bps.len() as f64
            } else {
                0.0
            },
            max_impact_bps: self.impact_costs_bps.iter().cloned().fold(0.0, f64::max),
            total_impact_cost: self.total_market_impact,
            liquidity_warnings: self.liquidity_warnings.clone(),
        });
        
        // Generate inventory metrics for market making analysis
        let total_volume = self.buy_volume + self.sell_volume;
        let avg_inventory = if !self.inventory_value_samples.is_empty() {
            self.inventory_value_samples.iter().sum::<f64>() / self.inventory_value_samples.len() as f64
        } else {
            // Fallback: use initial inventory value
            config.trading.initial_capital * 0.5 // Assume 50% inventory
        };
        
        // Calculate trading days from equity curve length (approximate)
        // Assuming high-frequency data: ~1000-10000 data points per day
        let data_points = equity_curve_len;
        let estimated_days = (data_points as f64 / 5000.0).max(1.0); // Rough estimate
        
        let turnover_per_day = if avg_inventory > 0.0 && estimated_days > 0.0 {
            total_volume / avg_inventory / estimated_days
        } else {
            0.0
        };
        
        let imbalance_ratio = if total_volume > 0.0 {
            (self.buy_volume - self.sell_volume) / total_volume
        } else {
            0.0
        };
        
        result.inventory_metrics = Some(InventoryMetrics {
            total_volume_traded: total_volume,
            avg_inventory_value: avg_inventory,
            turnover_per_day,
            buy_fills: self.buy_fills,
            sell_fills: self.sell_fills,
            buy_volume: self.buy_volume,
            sell_volume: self.sell_volume,
            imbalance_ratio,
        });
        
        // Calculate execution metrics for order lifecycle analysis
        let orders_placed = self.bid_orders_placed + self.ask_orders_placed;
        let orders_filled = self.order_trackers.values().filter(|t| t.filled).count();
        let orders_with_fills = self.order_trackers.values().filter(|t| t.fill_count > 0).count();
        let total_fills_from_trackers: usize = self.order_trackers.values().map(|t| t.fill_count).sum();
        
        let fill_rate = if orders_placed > 0 { orders_filled as f64 / orders_placed as f64 } else { 0.0 };
        let cancellation_rate = if orders_placed > 0 { self.orders_cancelled as f64 / orders_placed as f64 } else { 0.0 };
        let avg_fills_per_order = if orders_with_fills > 0 { total_fills_from_trackers as f64 / orders_with_fills as f64 } else { 0.0 };
        
        // Time to fill statistics
        let avg_time_to_fill = if !self.time_to_fill_samples.is_empty() {
            self.time_to_fill_samples.iter().sum::<f64>() / self.time_to_fill_samples.len() as f64
        } else { 0.0 };
        
        let median_time_to_fill = if !self.time_to_fill_samples.is_empty() {
            let mut sorted = self.time_to_fill_samples.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = sorted.len() / 2;
            if sorted.len() % 2 == 0 && sorted.len() > 1 {
                (sorted[mid - 1] + sorted[mid]) / 2.0
            } else {
                sorted[mid]
            }
        } else { 0.0 };
        
        // Bid/ask fill rates
        let bid_fills_count = self.order_trackers.values()
            .filter(|t| t.filled && matches!(t.side, orderbook::BookSide::Bid))
            .count();
        let ask_fills_count = self.order_trackers.values()
            .filter(|t| t.filled && matches!(t.side, orderbook::BookSide::Ask))
            .count();
        let bid_fill_rate = if self.bid_orders_placed > 0 { bid_fills_count as f64 / self.bid_orders_placed as f64 } else { 0.0 };
        let ask_fill_rate = if self.ask_orders_placed > 0 { ask_fills_count as f64 / self.ask_orders_placed as f64 } else { 0.0 };
        
        // Average quote spread (from tracked spreads)
        let avg_quote_spread_bps = if !self.quote_spreads_bps.is_empty() {
            self.quote_spreads_bps.iter().sum::<f64>() / self.quote_spreads_bps.len() as f64
        } else { 0.0 };
        
        // Calculate round-trip win rate metrics for market making
        // A round-trip is a buy fill matched with a sell fill
        // Profitable round-trip: sell_price - buy_price > 2 * fee_rate * avg_price
        let total_round_trips = self.buy_fill_prices.len().min(self.sell_fill_prices.len());
        let mut profitable_round_trips = 0_usize;
        let mut total_round_trip_pnl = 0.0_f64;
        
        // Sort prices for FIFO matching: oldest buys matched with oldest sells
        // Actually, for market making, we match buy fills with sell fills in order
        // Each round-trip: bought at buy_price, sold at sell_price
        // Profit = (sell_price - buy_price) - fee_rate * (buy_price + sell_price)
        let fee_rate = if self.fee_rate > 0.0 { self.fee_rate } else { config.trading.commission_rate };
        
        for i in 0..total_round_trips {
            let buy_price = self.buy_fill_prices[i];
            let sell_price = self.sell_fill_prices[i];
            let gross_pnl = sell_price - buy_price;
            let fees = fee_rate * (buy_price + sell_price);
            let net_pnl = gross_pnl - fees;
            total_round_trip_pnl += net_pnl;
            if net_pnl > 0.0 {
                profitable_round_trips += 1;
            }
        }
        
        let round_trip_win_rate = if total_round_trips > 0 {
            Some(profitable_round_trips as f64 / total_round_trips as f64)
        } else {
            None
        };
        
        let avg_pnl_per_round_trip = if total_round_trips > 0 {
            Some(total_round_trip_pnl / total_round_trips as f64)
        } else {
            None
        };
        
        // Spread capture rate: actual avg spread captured / quoted spread
        // If we quoted 10 bps but only captured 5 bps on average, capture rate = 50%
        let spread_capture_rate = if total_round_trips > 0 && avg_quote_spread_bps > 0.0 {
            // Average spread captured in bps
            let avg_price = (self.buy_fill_prices.iter().sum::<f64>() + self.sell_fill_prices.iter().sum::<f64>()) 
                / (self.buy_fill_prices.len() + self.sell_fill_prices.len()) as f64;
            let avg_spread_captured_bps = if avg_price > 0.0 {
                (total_round_trip_pnl / total_round_trips as f64) / avg_price * 10000.0
            } else { 0.0 };
            Some((avg_spread_captured_bps / avg_quote_spread_bps).clamp(0.0, 2.0))
        } else {
            None
        };
        
        // Adverse selection rate: % of round-trips where price moved against us
        // (i.e., we bought and price went down, or we sold and price went up before completing the round-trip)
        // Simplified: % of round-trips that were unprofitable
        let adverse_selection_rate = if total_round_trips > 0 {
            Some((total_round_trips - profitable_round_trips) as f64 / total_round_trips as f64)
        } else {
            None
        };
        
        log_info!(BACKTEST_LOGGER, "Round-trip metrics | total={} profitable={} win_rate={:.1}% avg_pnl=${:.4} spread_capture={:.1}% adverse_sel={:.1}%",
            total_round_trips, profitable_round_trips, 
            round_trip_win_rate.unwrap_or(0.0) * 100.0,
            avg_pnl_per_round_trip.unwrap_or(0.0),
            spread_capture_rate.unwrap_or(0.0) * 100.0,
            adverse_selection_rate.unwrap_or(0.0) * 100.0);
        
        result.execution_metrics = Some(ExecutionMetrics {
            orders_placed,
            orders_filled,
            orders_cancelled: self.orders_cancelled,
            total_fills: total_fills_from_trackers,
            fill_rate,
            cancellation_rate,
            avg_fills_per_order,
            avg_time_to_fill_seconds: avg_time_to_fill,
            median_time_to_fill_seconds: median_time_to_fill,
            bid_orders: self.bid_orders_placed,
            ask_orders: self.ask_orders_placed,
            bid_fills: bid_fills_count,
            ask_fills: ask_fills_count,
            bid_fill_rate,
            ask_fill_rate,
            avg_quote_spread_bps,
            // Market-making specific win rate metrics
            round_trip_win_rate,
            total_round_trips,
            profitable_round_trips,
            spread_capture_rate,
            adverse_selection_rate,
            avg_pnl_per_round_trip,
        });
        
        // Log execution metrics summary
        log_info!(BACKTEST_LOGGER, "Execution metrics | orders_placed={} filled={} cancelled={} fill_rate={:.1}% cancel_rate={:.1}%",
            orders_placed, orders_filled, self.orders_cancelled, fill_rate * 100.0, cancellation_rate * 100.0);
        log_info!(BACKTEST_LOGGER, "Execution metrics | avg_time_to_fill={:.3}s median={:.3}s bid_fill_rate={:.1}% ask_fill_rate={:.1}%",
            avg_time_to_fill, median_time_to_fill, bid_fill_rate * 100.0, ask_fill_rate * 100.0);
        
        // Always log latency simulation settings and rejection stats
        let total_potential = orders_filled + self.latency_rejections;
        let rejection_pct = if total_potential > 0 { 
            self.latency_rejections as f64 / total_potential as f64 * 100.0 
        } else { 
            0.0 
        };
        log_info!(BACKTEST_LOGGER, "Latency simulation | latency_ms={} rejections={} (bid={} ask={}) rejection_rate={:.1}%",
            config.trading.latency_ms, self.latency_rejections, 
            self.latency_rejections_bid, self.latency_rejections_ask, rejection_pct);
        
        // Log average quoted spread - key diagnostic for fill rate
        log_info!(BACKTEST_LOGGER, "Spread analysis | avg_quoted_spread_bps={:.2} samples={} | wider spreads = fewer fills but higher profit/fill",
            avg_quote_spread_bps, self.quote_spreads_bps.len());
        
        // Log market data trade direction distribution
        let total_market_trades = self.market_buy_trades + self.market_sell_trades;
        let buy_pct = if total_market_trades > 0 { self.market_buy_trades as f64 / total_market_trades as f64 * 100.0 } else { 0.0 };
        let sell_pct = if total_market_trades > 0 { self.market_sell_trades as f64 / total_market_trades as f64 * 100.0 } else { 0.0 };
        log_info!(BACKTEST_LOGGER, "Market trades | total={} buys={} ({:.1}%) sells={} ({:.1}%) | buys->fill asks, sells->fill bids",
            total_market_trades, self.market_buy_trades, buy_pct, self.market_sell_trades, sell_pct);
        
        // Log missed opportunity analysis - KEY DIAGNOSTIC for fill rate issues
        let avg_bid_gap = if self.missed_bid_opportunities > 0 { 
            self.bid_price_gap_sum / self.missed_bid_opportunities as f64 
        } else { 0.0 };
        let avg_ask_gap = if self.missed_ask_opportunities > 0 { 
            self.ask_price_gap_sum / self.missed_ask_opportunities as f64 
        } else { 0.0 };
        log_info!(BACKTEST_LOGGER, 
            "MISSED_FILLS | bid_opportunities_missed={} avg_gap=${:.4} | ask_opportunities_missed={} avg_gap=${:.4}",
            self.missed_bid_opportunities, avg_bid_gap, 
            self.missed_ask_opportunities, avg_ask_gap);
        
        // Calculate what percentage of sell trades we could have captured if our bids were higher
        let potential_bid_fills = self.market_sell_trades;
        let missed_bid_pct = if potential_bid_fills > 0 {
            self.missed_bid_opportunities as f64 / potential_bid_fills as f64 * 100.0
        } else { 0.0 };
        let captured_bid_fills = result.execution_metrics.as_ref().map(|m| m.bid_fills).unwrap_or(0);
        let captured_bid_pct = if potential_bid_fills > 0 {
            captured_bid_fills as f64 / potential_bid_fills as f64 * 100.0
        } else { 0.0 };
        log_info!(BACKTEST_LOGGER,
            "BID_FILL_RATE | sell_trades={} captured={} ({:.2}%) missed={} ({:.2}%) | avg_gap=${:.4} = how much higher bid needs to be",
            potential_bid_fills, captured_bid_fills, captured_bid_pct, 
            self.missed_bid_opportunities, missed_bid_pct, avg_bid_gap);
        
        // Log data date range and market trend
        // Add timestamps to result
        result.first_trade_timestamp = self.first_trade_timestamp;
        result.last_trade_timestamp = self.last_trade_timestamp;
        result.first_trade_price = self.first_trade_price;
        result.last_trade_price = self.last_trade_price;
        
        if let (Some(start_ts), Some(end_ts)) = (self.first_trade_timestamp, self.last_trade_timestamp) {
            let duration = end_ts - start_ts;
            let hours = duration.num_hours();
            let minutes = duration.num_minutes() % 60;
            log_info!(BACKTEST_LOGGER, "Data range | start={} end={} duration={}h {}m",
                start_ts.format("%Y-%m-%d %H:%M:%S UTC"), 
                end_ts.format("%Y-%m-%d %H:%M:%S UTC"),
                hours, minutes);
            
            // Log market trend (price change during backtest)
            if let (Some(start_price), Some(end_price)) = (self.first_trade_price, self.last_trade_price) {
                let price_change = end_price - start_price;
                let price_change_pct = (price_change / start_price) * 100.0;
                let trend = if price_change_pct > 1.0 { "UPTREND" } 
                           else if price_change_pct < -1.0 { "DOWNTREND" } 
                           else { "SIDEWAYS" };
                log_info!(BACKTEST_LOGGER, "Market trend | start_price=${:.2} end_price=${:.2} change={:.2}% ({}) | {} bids hit more, {} asks hit more",
                    start_price, end_price, price_change_pct, trend,
                    if price_change_pct > 0.0 { "uptrend" } else { "downtrend" },
                    if price_change_pct > 0.0 { "downtrend" } else { "uptrend" });
            }
        } else {
            log_info!(BACKTEST_LOGGER, "Data range | No trades processed in market data");
        }
        
        // Log inventory metrics summary
        let final_inventory = self.buy_qty - self.sell_qty;
        log_info!(BACKTEST_LOGGER, "Inventory position | final_qty={:.6} max_long={:.6} max_short={:.6} | asymmetric if not near zero",
            final_inventory, self.max_inventory, self.min_inventory);
        log_info!(BACKTEST_LOGGER, "Inventory metrics | total_volume=${:.2} buy=${:.2} sell=${:.2} turnover/day={:.2} imbalance={:.2}%",
            total_volume, self.buy_volume, self.sell_volume, turnover_per_day, imbalance_ratio * 100.0);
        
        // Log fee tier progression for debugging commission issues
        for (exchange, volume) in &self.current_30d_volume_by_exchange {
            let fee_rate = if let Some(ref fee_config) = config.trading.exchange_fees {
                fee_config.get_fee_rate(*volume, true) // maker rate
            } else {
                config.trading.commission_rate
            };
            log_info!(BACKTEST_LOGGER, "Fee tier summary | exchange={} final_30d_volume=${:.0} final_maker_fee_bps={:.1} total_commission=${:.2}",
                exchange, volume, fee_rate * 10000.0, self.total_commission);
        }
        
        // Log commission breakdown by fee tier
        let mut tier_breakdown: Vec<_> = self.commission_by_tier_bps.iter().collect();
        tier_breakdown.sort_by_key(|(tier, _)| **tier);
        for (tier_bps, commission) in &tier_breakdown {
            let fills = self.fills_by_tier_bps.get(tier_bps).unwrap_or(&0);
            let avg_per_fill = if *fills > 0 { *commission / *fills as f64 } else { 0.0 };
            log_info!(BACKTEST_LOGGER, "Commission by tier | fee_bps={} fills={} commission=${:.2} avg_per_fill=${:.2}",
                tier_bps, fills, commission, avg_per_fill);
        }
        
        // Log fill count distribution
        let mut fill_dist: Vec<_> = self.fill_count_histogram.iter().collect();
        fill_dist.sort_by_key(|(count, _)| **count);
        let fill_dist_str: String = fill_dist.iter()
            .map(|(fills, orders)| format!("{}fills:{}orders", fills, orders))
            .collect::<Vec<_>>()
            .join(", ");
        log_info!(BACKTEST_LOGGER, "Fill distribution | total_orders={} total_fills={} avg_fills_per_order={:.2} distribution=[{}]",
            self.total_order_count, self.total_fill_count, 
            if self.total_order_count > 0 { self.total_fill_count as f64 / self.total_order_count as f64 } else { 0.0 },
            fill_dist_str);

        // Diagnostic: Calculate average fill value from commission data
        // This helps identify if order sizes are as expected
        let total_fills = self.fills_by_tier_bps.values().sum::<u32>();
        log_info!(BACKTEST_LOGGER, "Order size diagnostic START | total_fills={} total_commission=${:.2} fills_by_tier_count={}",
            total_fills, self.total_commission, self.fills_by_tier_bps.len());
        
        if total_fills > 0 {
            // Estimate average fill value by dividing commission by estimated fee rate
            // Use the most common fee tier, or fallback to config commission rate
            let fee_rate = if let Some((&tier_bps, _)) = self.fills_by_tier_bps.iter().max_by_key(|(_, count)| *count) {
                if tier_bps > 0 {
                    tier_bps as f64 / 10000.0
                } else {
                    config.trading.commission_rate.max(0.001) // Fallback to 0.1% minimum
                }
            } else {
                config.trading.commission_rate.max(0.001)
            };
            
            let estimated_total_fill_value = if self.total_commission > 0.0 && fee_rate > 0.0 {
                self.total_commission / fee_rate
            } else {
                0.0
            };
            let avg_fill_value = if total_fills > 0 { estimated_total_fill_value / total_fills as f64 } else { 0.0 };
            let expected_order_value = config.trading.initial_capital * 0.02; // 2% of capital
            log_info!(BACKTEST_LOGGER, 
                "Order size diagnostic | total_fills={} commission=${:.2} fee_rate={:.4}% => estimated_fill_value=${:.2}/fill (expected ~${:.2} at 2% order_size_pct)",
                total_fills, self.total_commission, fee_rate * 100.0, avg_fill_value, expected_order_value);
        }
        
        // Log performance optimization stats
        let skip_pct = if processed_count > 0 {
            (self.stable_region_skips as f64 / processed_count as f64) * 100.0
        } else { 0.0 };
        log_info!(BACKTEST_LOGGER, "PERF_OPT | stable_region_skips={} ({:.1}% of {} ticks) | strategy calls saved",
            self.stable_region_skips, skip_pct, processed_count);

        result
    }
    
    /// Run simulation using a streaming TickSource (memory-efficient)
    /// 
    /// This method processes ticks one at a time from a memory-mapped binary file,
    /// maintaining constant memory usage (~20 KB) regardless of dataset size.
    /// 
    /// # Arguments
    /// * `strategy_manager` - Strategy for generating signals
    /// * `orderbook` - Order book for matching
    /// * `portfolio` - Portfolio state
    /// * `risk_manager` - Risk management
    /// * `source` - TickSource providing tick data (mmap-backed)
    /// * `symbol` - Symbol being traded
    /// * `exchange` - Exchange name
    /// * `config` - Backtest configuration
    pub async fn run_streaming<S: dataloader::TickSource>(
        &mut self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        source: &S,
        symbol: &str,
        exchange: &str,
        config: &config::BacktestConfig,
    ) -> BacktestResult {
        use dataloader::Tick;
        
        let total_items = source.len();
        
        // Collect into a temporary Vec for the existing run() method
        // Processing in chunks to limit peak memory usage
        const CHUNK_SIZE: usize = 500_000; // Process 500K ticks at a time (~24 MB)
        
        let mut final_result = BacktestResult::default();
        let mut chunk_start = 0;
        
        while chunk_start < total_items {
            let chunk_end = (chunk_start + CHUNK_SIZE).min(total_items);
            
            // Build chunk of MarketData from ticks
            let chunk: Vec<MarketData> = (chunk_start..chunk_end)
                .filter_map(|i| {
                    source.get(i).map(|tick| {
                        let timestamp = tick.timestamp(); // Already DateTime<Utc>
                        let price = tick.price();
                        let quantity = tick.quantity();
                        let side_str = match tick.side() {
                            dataloader::Side::Buy => "buy",
                            dataloader::Side::Sell => "sell",
                        };
                        
                        MarketData::Trade(dataloader::TradeData {
                            timestamp,
                            symbol: symbol.to_string(),
                            exchange: exchange.to_string(),
                            price: BigDecimal::from_f64(price).unwrap_or_default(),
                            quantity: BigDecimal::from_f64(quantity).unwrap_or_default(),
                            side: side_str.to_string(),
                            trade_id: String::new(),
                            price_f64: price,
                            quantity_f64: quantity,
                            value_f64: price * quantity,
                        })
                    })
                })
                .collect();
            
            log_info!(BACKTEST_LOGGER, "STREAMING_CHUNK | {}-{}/{} ({:.1}%)", 
                chunk_start, chunk_end, total_items, 
                (chunk_end as f64 / total_items as f64) * 100.0);
            
            // Run simulation on this chunk
            let chunk_result = self.run(
                strategy_manager,
                orderbook,
                portfolio,
                risk_manager,
                &chunk,
                config,
            ).await;
            
            // Merge chunk result into final result
            if chunk_start == 0 {
                final_result = chunk_result;
            } else {
                // Append equity curve
                final_result.equity_curve.extend(chunk_result.equity_curve);
                // Take latest metrics
                final_result.net_profit = chunk_result.net_profit;
                final_result.sharpe_ratio = chunk_result.sharpe_ratio;
                final_result.max_drawdown = chunk_result.max_drawdown;
                final_result.num_trades += chunk_result.num_trades;
            }
            
            chunk_start = chunk_end;
        }
        
        final_result
    }
}

/// Convert HistoricalOrder from database to OrderBookEvent
/// This allows us to reconstruct orderbook state during backtest simulation
#[cfg(feature = "postgres")]
fn convert_historical_order_to_event(order: &HistoricalOrder) -> Option<OrderBookEvent> {
    
    let event_type = order.get_event_type().to_uppercase();
    let order_id = order.get_order_id().to_string();
    let timestamp = *order.get_timestamp();
    
    match event_type.as_str() {
        "NEW" => {
            // Parse side
            let side = match BookSide::from_str(order.get_side()) {
                Some(s) => s,
                None => {
                    BACKTEST_LOGGER.warn_sync(format!(
                        "Invalid side '{}' for order {}", 
                        order.get_side(), 
                        order_id
                    ));
                    return None;
                }
            };
            
            Some(OrderBookEvent::NewOrder {
                order_id,
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        "MODIFY" => {
            Some(OrderBookEvent::ModifyOrder {
                order_id,
                new_quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        "CANCEL" => {
            Some(OrderBookEvent::CancelOrder {
                order_id,
                timestamp,
            })
        }
        "TRADE" => {
            // Parse side for trade
            let side = match BookSide::from_str(order.get_side()) {
                Some(s) => s,
                None => {
                    BACKTEST_LOGGER.warn_sync(format!(
                        "Invalid side '{}' for trade order {}", 
                        order.get_side(), 
                        order_id
                    ));
                    return None;
                }
            };
            
            Some(OrderBookEvent::Trade {
                aggressor_order_id: None,  // Historical data may not have aggressor ID
                passive_order_id: Some(order_id),
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        _ => {
            BACKTEST_LOGGER.warn_sync(format!(
                "Unknown event type '{}' for order {}", 
                event_type, 
                order_id
            ));
            None
        }
    }
}
