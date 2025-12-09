//! SimulationLoop: Drives the backtest event loop
//!
//! Steps through market data, triggers strategies, matches orders, and updates state.

use strategy::{StrategyManager, StrategyContext};
use strategy::traits::{MarketSession, SessionType};
use orderbook::OrderBook;
use orderbook::{OrderBookEvent, BookSide};
#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;
use portfoliomanager::PortfolioState;
use riskmanager::{RiskManager, RiskMetrics, RiskAction};
use dataloader::MarketData;
use crate::types::{BacktestEvent, BacktestResult, TransactionCostAnalysis, MarketImpactAnalysis, LiquidityWarning};
use crate::logging_facade::BACKTEST_LOGGER;
use metrics::{performance, risk, trade_stats};
use bigdecimal::{BigDecimal, ToPrimitive, FromPrimitive};
use rustc_hash::FxHashMap;

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
    active_order_ids: Vec<String>,
    // Track rolling market volume for realistic market impact calculation
    market_volume_window: Vec<(chrono::DateTime<chrono::Utc>, f64)>, // (timestamp, trade_volume_usd)
    rolling_avg_volume: f64, // Average volume over rolling window
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
            active_order_ids: Vec::with_capacity(10),
            market_volume_window: Vec::with_capacity(1000), // Track last ~1000 trades
            rolling_avg_volume: 0.0,
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
        market_data: Vec<MarketData>,
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

    for data in market_data {
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
                    
                    // Get exchange for fee tracking
                    let exchange = orderbook.exchange.clone();
                    
                    // Track market volume for realistic market impact calculation
                    let trade_volume_usd = trade.price.to_f64().unwrap_or(0.0) * trade.quantity.to_f64().unwrap_or(0.0);
                    self.update_market_volume(trade.timestamp, trade_volume_usd);
                    
                    // Convert TradeData side string to BookSide
                    // The trade side is the AGGRESSOR's side (the taker who initiated):
                    // - "buy" means a buyer aggressed -> fills our ASK orders
                    // - "sell" means a seller aggressed -> fills our BID orders
                    let aggressor_side = match trade.side.to_lowercase().as_str() {
                        "buy" | "b" => BookSide::Bid,   // Aggressor is buyer
                        "sell" | "s" => BookSide::Ask,  // Aggressor is seller
                        _ => continue, // Skip unknown side
                    };
                    
                    // Execute the trade against our resting orders and get fills
                    let trade_fills = orderbook.execute_trade_with_fills(
                        aggressor_side,
                        trade.price.clone(),
                        trade.quantity.clone(),
                        trade.timestamp,
                    );
                    
                    // Track fill count for market fills
                    let fill_count = trade_fills.len() as u32;
                    if fill_count > 0 {
                        self.total_order_count += 1;
                        self.total_fill_count += fill_count;
                        *self.fill_count_histogram.entry(fill_count).or_insert(0) += 1;
                    }
                    
                    // Track commission for market fills (our resting orders getting filled)
                    for fill in &trade_fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
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
                    
                    // Apply fills from incoming trades to portfolio
                    if !trade_fills.is_empty() {
                        if let Err(e) = portfolio.apply_fills(&trade_fills) {
                            log_error!(BACKTEST_LOGGER, "Error applying trade fills: {}", e);
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
                    
                    // Get exchange for fee tracking
                    let exchange = orderbook.exchange.clone();
                    
                    // Track market volume for realistic market impact calculation
                    let trade_volume_usd = trade.price.to_f64().unwrap_or(0.0) * trade.quantity.to_f64().unwrap_or(0.0);
                    self.update_market_volume(trade.timestamp, trade_volume_usd);
                    
                    // Convert TradeData side string to BookSide
                    // The trade side is the AGGRESSOR's side (the taker who initiated):
                    // - "buy" means a buyer aggressed -> fills our ASK orders
                    // - "sell" means a seller aggressed -> fills our BID orders
                    let aggressor_side = match trade.side.to_lowercase().as_str() {
                        "buy" | "b" => BookSide::Bid,   // Aggressor is buyer
                        "sell" | "s" => BookSide::Ask,  // Aggressor is seller
                        _ => continue, // Skip unknown side
                    };
                    
                    // Execute the trade against our resting orders and get fills
                    let trade_fills = orderbook.execute_trade_with_fills(
                        aggressor_side,
                        trade.price.clone(),
                        trade.quantity.clone(),
                        trade.timestamp,
                    );
                    
                    // Track fill count for market fills
                    let fill_count = trade_fills.len() as u32;
                    if fill_count > 0 {
                        self.total_order_count += 1;
                        self.total_fill_count += fill_count;
                        *self.fill_count_histogram.entry(fill_count).or_insert(0) += 1;
                    }
                    
                    // Track commission for market fills (our resting orders getting filled)
                    for fill in &trade_fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
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
                    
                    // Apply fills from incoming trades to portfolio
                    if !trade_fills.is_empty() {
                        if let Err(e) = portfolio.apply_fills(&trade_fills) {
                            log_error!(BACKTEST_LOGGER, "Error applying trade fills: {}", e);
                        }
                    }
                }
                MarketData::Candle(candle) => {
                    // Update current market price from candle close
                    current_market_price = Some(candle.close.clone());
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

            // 3. Run all enabled strategies, collect signals
            let signals = strategy_manager.process_market_data(&data, &context).await.unwrap_or_default();
            
            // Cancel previous active orders before placing new quotes
            // This prevents order accumulation that leads to incorrect fill volumes
            if !signals.is_empty() {
                for order_id in &self.active_order_ids {
                    let cancel_event = OrderBookEvent::CancelOrder { order_id: order_id.clone(), timestamp };
                    let _ = orderbook.apply_event(cancel_event);
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
            };
            
            // Check global risk limits before processing signals
            if let Err(reason) = risk_manager.check(&risk_metrics) {
                log_error!(BACKTEST_LOGGER, "Risk limit exceeded: {}", reason);
                trading_halted = true;
                halt_reason = Some(reason);
                continue;
            }
            
            for (_strategy_id, sigs) in signals {
                for signal in sigs {
                    // Extract order details for cost calculation
                    let order_price = signal.price.as_ref()
                        .and_then(|p| p.to_f64())
                        .unwrap_or(0.0);
                    let mut order_size = signal.quantity.as_ref()
                        .and_then(|q| q.to_f64())
                        .unwrap_or(0.0);
                    let exchange = orderbook.exchange.clone();
                    
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
                    
                    // Track this order ID for later cancellation
                    self.active_order_ids.push(signal.id.clone());
                    
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
            }
            
            // Collect TOTAL portfolio value (balance + unrealized P&L) at each step
            // This correctly reflects the portfolio's worth including open positions
            let total_portfolio_value = portfolio.get_total_value().to_f64().unwrap_or(0.0);
            equity_curve.push(total_portfolio_value);
        }

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
        result.num_trades = performance::number_of_trades(&trade_returns);
        result.gross_profit = Some(performance::gross_profit(&trade_returns));
        result.gross_loss = Some(performance::gross_loss(&trade_returns));
        result.net_profit = Some(performance::net_profit(&trade_returns));
        result.avg_trade_return = performance::average_trade_return(&trade_returns);
        result.median_trade_return = performance::median_trade_return(&trade_returns);
        
        // For Sharpe ratio, if we have very few closed trades but significant unrealized P&L,
        // use the equity curve returns instead for a more representative metric
        let use_equity_returns = trade_returns.len() < 10 && open_position_count > 0;
        let sharpe_returns = if use_equity_returns {
            // Calculate returns from equity curve for better Sharpe estimation
            equity_curve.windows(2)
                .filter_map(|w| if w[0] > 0.0 { Some((w[1] - w[0]) / w[0]) } else { None })
                .collect::<Vec<_>>()
        } else {
            trade_returns.clone()
        };
        result.sharpe_ratio = performance::sharpe_ratio(&sharpe_returns, config.trading.risk_free_rate);
        
        result.profit_factor = performance::profit_factor(
            result.gross_profit.unwrap_or(0.0),
            result.gross_loss.unwrap_or(0.0),
        );
        
        // Calculate win rate - if no closed trades, indicate this clearly with None
        result.win_rate = if closed_position_count > 0 {
            trade_stats::win_rate(&trade_returns)
        } else {
            // No closed trades - win rate is undefined, not 0%
            None
        };
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
        result.sortino_ratio = risk::sortino_ratio(&sharpe_returns, config.trading.risk_free_rate);
        
        // Calculate Calmar ratio (annualized return / max drawdown)
        // Uses percentage-based annualized return for proper ratio
        let max_dd = result.max_drawdown.to_f64().unwrap_or(0.0);
        if !sharpe_returns.is_empty() && max_dd > 0.0 {
            // Calculate annualized return from percentage returns
            // Mean return per period * periods per year (assuming ~252 trading days)
            let mean_return = sharpe_returns.iter().sum::<f64>() / sharpe_returns.len() as f64;
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

        result
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
