//! Realistic Order Book Simulation for Final Evaluation
//!
//! This module provides a high-fidelity simulation that maintains full order book
//! state and tracks queue positions for realistic fill modeling. It's designed
//! for final evaluation after GA optimization, where accuracy matters more than speed.
//!
//! # Key Features
//!
//! 1. **Full Order Book State**: Tracks bids/asks at all price levels
//! 2. **Queue Position Tracking**: Models FIFO queue priority for limit orders
//! 3. **Realistic Fill Modeling**: Only fills when sufficient volume crosses our price
//! 4. **Event-Driven Processing**: Handles new/modify/cancel/trade events properly
//!
//! # Usage
//! ```ignore
//! let simulator = RealisticSimulator::new(initial_capital, fee_rate);
//! let result = simulator.run(&events, &params);
//! ```

use std::collections::{BTreeMap, VecDeque};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use bigdecimal::BigDecimal;

/// Event types from historical data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    New,
    Modify,
    Cancel,
    Trade,
}

impl EventType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "NEW" | "ADD" => Some(EventType::New),
            "MODIFY" | "UPDATE" => Some(EventType::Modify),
            "CANCEL" | "DELETE" => Some(EventType::Cancel),
            "TRADE" | "FILL" | "EXECUTION" => Some(EventType::Trade),
            _ => None,
        }
    }
}

/// Side of the order
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BUY" | "BID" | "B" => Some(Side::Buy),
            "SELL" | "ASK" | "S" | "OFFER" => Some(Side::Sell),
            _ => None,
        }
    }
}

/// A compact order book event for realistic simulation
#[derive(Debug, Clone)]
pub struct OrderBookEvent {
    pub timestamp_ms: i64,
    pub event_type: EventType,
    pub order_id: String,
    pub side: Side,
    pub price: f64,
    pub quantity: f64,
}

// ============================================================================
// MARKET DATA CONVERSION
// ============================================================================

#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;
#[cfg(feature = "postgres")]
use dataloader::MarketData;
#[cfg(feature = "postgres")]
use bigdecimal::ToPrimitive;

/// Convert MarketData slice (containing HistoricalOrder) to OrderBookEvent vec
/// This is the bridge between the data loader and the realistic simulation engine
#[cfg(feature = "postgres")]
pub fn market_data_to_order_events(data: &[MarketData]) -> Vec<OrderBookEvent> {
    data.iter()
        .filter_map(|md| match md {
            MarketData::Order(order) => Some(order_to_event(order)),
            MarketData::Trade(trade) => {
                // Convert trades to Trade events for price discovery
                let side = Side::from_str(&trade.side).unwrap_or(Side::Buy);
                Some(OrderBookEvent {
                    timestamp_ms: trade.timestamp.timestamp_millis(),
                    event_type: EventType::Trade,
                    order_id: trade.trade_id.clone(),
                    side,
                    price: trade.price.to_f64().unwrap_or(0.0),
                    quantity: trade.quantity.to_f64().unwrap_or(0.0),
                })
            }
            _ => None, // Skip Candles and Snapshots
        })
        .collect()
}

/// Convert a HistoricalOrder to an OrderBookEvent
#[cfg(feature = "postgres")]
pub fn order_to_event(order: &HistoricalOrder) -> OrderBookEvent {
    let event_type = EventType::from_str(&order.event_type).unwrap_or(EventType::New);
    let side = Side::from_str(&order.side).unwrap_or(Side::Buy);
    
    OrderBookEvent {
        timestamp_ms: order.timestamp.timestamp_millis(),
        event_type,
        order_id: order.order_id.clone(),
        side,
        price: order.price_level.to_f64().unwrap_or(0.0),
        quantity: order.quantity.to_f64().unwrap_or(0.0),
    }
}

/// Parameters for Avellaneda-Stoikov strategy in realistic simulation
#[derive(Debug, Clone)]
pub struct RealisticASParams {
    pub risk_aversion: f64,
    pub inventory_target: f64,
    pub order_size_pct: f64,
    pub volatility_cap: f64,
    pub min_spread_bps: f64,
    pub time_horizon_minutes: f64,
    
    // === Initial Inventory ===
    /// Initial inventory as percentage of capital (0.5 = 50% in base asset)
    /// This is converted to base asset quantity using the first available price
    pub initial_inventory_pct: f64,
    
    // === Fee Configuration (Maker/Taker) ===
    /// Fee rate for passive (maker) orders - typically negative rebate or lower fee
    pub maker_fee_rate: f64,
    /// Fee rate for aggressive (taker) orders - typically higher
    pub taker_fee_rate: f64,
    
    // === Inventory Risk Limits ===
    /// Maximum inventory as multiple of order size (e.g., 5.0 = 5x order size)
    pub max_inventory_multiple: f64,
    
    // === Latency Modeling ===
    /// Mean latency in milliseconds for order execution
    pub latency_mean_ms: f64,
    /// Standard deviation of latency (for jitter simulation)
    pub latency_std_ms: f64,
    
    // === Partial Fill Modeling ===
    /// Enable partial fills (vs all-or-nothing)
    pub enable_partial_fills: bool,
    /// Minimum fill size as fraction of order (0.1 = 10% minimum)
    pub min_fill_fraction: f64,
    
    // === Rate Limiting ===
    /// Maximum orders per second (exchange rate limit)
    pub max_orders_per_second: f64,
    /// Cooldown after rate limit hit (ms)
    pub rate_limit_cooldown_ms: i64,
    
    // === Funding Rate (Perpetuals) ===
    /// Funding rate per 8-hour interval (e.g., 0.0001 = 0.01%)
    pub funding_rate_8h: f64,
    /// Funding interval in milliseconds (default 8 hours)
    pub funding_interval_ms: i64,
    
    // === Adverse Selection Protection ===
    /// Sensitivity to toxic order flow: 0.0 = ignore, 1.0 = proportional widening, 2.0 = aggressive
    /// When detected, spreads are widened by this multiplier of the toxicity signal
    pub adverse_selection_sensitivity: f64,
}

impl Default for RealisticASParams {
    fn default() -> Self {
        // Use Kraken's configuration as default - all values from config
        let exchange_config = config::ExchangeFeeConfig::kraken();
        
        Self {
            risk_aversion: 0.5,
            inventory_target: 0.0,
            order_size_pct: 0.01,
            volatility_cap: 0.5,
            // min_spread_bps must be > 2x maker_fee to be profitable
            // Kraken default maker fee is 25 bps, so we need at least 50 bps spread
            // We use 60 bps as default to provide margin for adverse selection
            min_spread_bps: 60.0,
            time_horizon_minutes: 5.0,
            
            // Initial inventory: 50% by default (balanced)
            initial_inventory_pct: 0.5,
            
            // All operational parameters from exchange configuration
            maker_fee_rate: exchange_config.maker_fee,
            taker_fee_rate: exchange_config.taker_fee,
            
            // Inventory limits (TODO: add to exchange config)
            max_inventory_multiple: 5.0,
            
            // Latency from exchange configuration
            latency_mean_ms: exchange_config.latency_mean_ms,
            latency_std_ms: exchange_config.latency_std_ms,
            
            // Partial fills enabled
            enable_partial_fills: true,
            min_fill_fraction: 0.1,
            
            // Rate limiting from exchange configuration
            max_orders_per_second: exchange_config.max_orders_per_second,
            rate_limit_cooldown_ms: exchange_config.rate_limit_cooldown_ms,
            
            // Funding rate from exchange configuration
            funding_rate_8h: exchange_config.funding_rate_8h,
            funding_interval_ms: exchange_config.funding_interval_ms,
            
            // Adverse selection: moderate sensitivity by default
            adverse_selection_sensitivity: 1.0,
        }
    }
}

/// Tracks rolling 30-day trading volume for dynamic fee tier calculation
#[derive(Debug, Clone)]
pub struct VolumeTracker {
    /// Volume entries: (timestamp_ms, volume_usd)
    entries: VecDeque<(i64, f64)>,
    /// Cached total for performance
    total_volume: f64,
    /// 30 days in milliseconds
    window_ms: i64,
}

impl VolumeTracker {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total_volume: 0.0,
            window_ms: 30 * 24 * 60 * 60 * 1000, // 30 days in ms
        }
    }
    
    /// Create with starting volume (simulates existing trading history)
    pub fn with_starting_volume(starting_volume: f64) -> Self {
        let mut tracker = Self::new();
        // Add starting volume as a single entry at time 0
        if starting_volume > 0.0 {
            tracker.entries.push_back((0, starting_volume));
            tracker.total_volume = starting_volume;
        }
        tracker
    }
    
    /// Add a trade and get updated 30-day volume
    pub fn add_trade(&mut self, timestamp_ms: i64, volume_usd: f64) -> f64 {
        // Remove entries older than 30 days
        let cutoff = timestamp_ms - self.window_ms;
        while let Some(&(ts, vol)) = self.entries.front() {
            if ts < cutoff {
                self.entries.pop_front();
                self.total_volume -= vol;
            } else {
                break;
            }
        }
        
        // Add new entry
        self.entries.push_back((timestamp_ms, volume_usd));
        self.total_volume += volume_usd;
        
        self.total_volume.max(0.0) // Ensure non-negative
    }
    
    /// Get current 30-day volume
    pub fn get_volume(&self) -> f64 {
        self.total_volume.max(0.0)
    }
}

impl Default for VolumeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Our strategy's pending order
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PendingOrder {
    order_id: String,
    side: Side,
    price: f64,
    quantity: f64,
    timestamp_ms: i64,
    quantity_ahead: f64,  // Quantity in queue ahead of us
}

/// Price level in the order book
#[derive(Debug, Clone, Default)]
struct PriceLevel {
    total_quantity: f64,
    orders: VecDeque<(String, f64)>,  // (order_id, quantity)
}

/// Simplified order book for tracking market state
#[derive(Debug, Clone, Default)]
struct SimpleOrderBook {
    bids: BTreeMap<OrderedFloat<f64>, PriceLevel>,
    asks: BTreeMap<OrderedFloat<f64>, PriceLevel>,
}

impl SimpleOrderBook {
    fn new() -> Self {
        Self::default()
    }
    
    /// Clear all orders from the book (used on snapshot boundary detection)
    fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }
    
    fn add_order(&mut self, side: Side, price: f64, quantity: f64, order_id: &str) {
        // First, remove any existing order with the same ID (handles order amends/replaces)
        // Kraken reuses order_ids when orders are amended to a new price
        self.cancel_order(order_id);
        
        let price_key = OrderedFloat(price);
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        
        let level = levels.entry(price_key).or_insert_with(PriceLevel::default);
        level.total_quantity += quantity;
        level.orders.push_back((order_id.to_string(), quantity));
    }
    
    fn cancel_order(&mut self, order_id: &str) -> bool {
        // Search bids first
        let mut found_price: Option<OrderedFloat<f64>> = None;
        let mut found_side_is_bids = false;
        
        for (&price, level) in self.bids.iter_mut() {
            if let Some(pos) = level.orders.iter().position(|(id, _)| id == order_id) {
                let (_, qty) = level.orders.remove(pos).unwrap();
                level.total_quantity -= qty;
                if level.orders.is_empty() {
                    found_price = Some(price);
                    found_side_is_bids = true;
                }
                // Found and removed - clean up empty level if needed and return
                if let Some(price) = found_price {
                    if found_side_is_bids {
                        self.bids.remove(&price);
                    }
                }
                return true;
            }
        }
        
        // Search asks
        for (&price, level) in self.asks.iter_mut() {
            if let Some(pos) = level.orders.iter().position(|(id, _)| id == order_id) {
                let (_, qty) = level.orders.remove(pos).unwrap();
                level.total_quantity -= qty;
                if level.orders.is_empty() {
                    found_price = Some(price);
                }
                // Found and removed - clean up empty level if needed and return
                if let Some(price) = found_price {
                    self.asks.remove(&price);
                }
                return true;
            }
        }
        
        false  // Order not found
    }
    
    fn modify_order(&mut self, order_id: &str, new_quantity: f64) {
        // Search both sides
        for levels in [&mut self.bids, &mut self.asks] {
            for (_, level) in levels.iter_mut() {
                if let Some(pos) = level.orders.iter().position(|(id, _)| id == order_id) {
                    let old_qty = level.orders[pos].1;
                    level.orders[pos].1 = new_quantity;
                    level.total_quantity = level.total_quantity - old_qty + new_quantity;
                    return;
                }
            }
        }
    }
    
    fn execute_trade(&mut self, side: Side, price: f64, quantity: f64) -> f64 {
        // Trade on one side removes liquidity from the other side
        let price_key = OrderedFloat(price);
        let levels = match side {
            Side::Buy => &mut self.asks,   // Buy trades consume asks
            Side::Sell => &mut self.bids,  // Sell trades consume bids
        };
        
        if let Some(level) = levels.get_mut(&price_key) {
            let mut remaining = quantity;
            while remaining > 0.0 && !level.orders.is_empty() {
                let (_, front_qty) = &mut level.orders[0];
                if *front_qty <= remaining {
                    remaining -= *front_qty;
                    level.total_quantity -= *front_qty;
                    level.orders.pop_front();
                } else {
                    *front_qty -= remaining;
                    level.total_quantity -= remaining;
                    remaining = 0.0;
                }
            }
            return quantity - remaining;  // Volume consumed at this level
        }
        0.0
    }
    
    fn best_bid(&self) -> Option<f64> {
        // Find highest bid with non-empty orders
        self.bids.iter().rev()
            .find(|(_, level)| !level.orders.is_empty() && level.total_quantity > 0.0)
            .map(|(price, _)| price.0)
    }
    
    fn best_ask(&self) -> Option<f64> {
        // Find lowest ask with non-empty orders
        self.asks.iter()
            .find(|(_, level)| !level.orders.is_empty() && level.total_quantity > 0.0)
            .map(|(price, _)| price.0)
    }
    
    fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }
    
    fn quantity_at_price(&self, side: Side, price: f64) -> f64 {
        let price_key = OrderedFloat(price);
        let levels = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        levels.get(&price_key).map(|l| l.total_quantity).unwrap_or(0.0)
    }
    
    #[allow(dead_code)]
    fn quantity_ahead_in_queue(&self, side: Side, price: f64, our_order_id: &str) -> f64 {
        let price_key = OrderedFloat(price);
        let levels = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        
        if let Some(level) = levels.get(&price_key) {
            let mut ahead = 0.0;
            for (order_id, qty) in &level.orders {
                if order_id == our_order_id {
                    return ahead;
                }
                ahead += qty;
            }
        }
        f64::MAX  // Order not found
    }
    
    /// Sanitize the order book by removing stale orders that are unreasonably far from the reference price.
    /// This handles the case where L3 data has old limit orders from before our data window that never got cancelled.
    /// 
    /// Returns the number of price levels removed.
    fn sanitize(&mut self, reference_price: f64, max_deviation_pct: f64) -> usize {
        let min_valid_price = reference_price * (1.0 - max_deviation_pct);
        let max_valid_price = reference_price * (1.0 + max_deviation_pct);
        
        let mut removed = 0;
        
        // Remove bids that are too far from reference (e.g., ancient low bids)
        let stale_bid_prices: Vec<_> = self.bids.keys()
            .filter(|p| p.0 < min_valid_price || p.0 > max_valid_price)
            .cloned()
            .collect();
        for price in stale_bid_prices {
            self.bids.remove(&price);
            removed += 1;
        }
        
        // Remove asks that are too far from reference (e.g., ancient high asks)
        let stale_ask_prices: Vec<_> = self.asks.keys()
            .filter(|p| p.0 < min_valid_price || p.0 > max_valid_price)
            .cloned()
            .collect();
        for price in stale_ask_prices {
            self.asks.remove(&price);
            removed += 1;
        }
        
        removed
    }
    
    /// Check if the book would be inverted after adding this order
    fn would_cause_inversion(&self, side: Side, price: f64) -> bool {
        match side {
            Side::Buy => {
                // A buy at this price would cause inversion if it's >= best ask
                if let Some(best_ask) = self.best_ask() {
                    price >= best_ask
                } else {
                    false
                }
            }
            Side::Sell => {
                // A sell at this price would cause inversion if it's <= best bid
                if let Some(best_bid) = self.best_bid() {
                    price <= best_bid
                } else {
                    false
                }
            }
        }
    }
}

/// Result from realistic simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealisticSimResult {
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub num_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_volume: f64,
    pub total_fees: f64,
    pub avg_fill_rate: f64,  // % of our orders that got filled
    pub avg_queue_position: f64,  // Average queue position when filled
    pub equity_curve: Vec<f64>,
    /// Individual trade P&L values for Monte Carlo simulation and statistical significance
    pub trade_returns: Vec<f64>,
    
    // === Enhanced Metrics ===
    /// Number of partial fills (vs full fills)
    pub partial_fill_count: usize,
    /// Rate limit rejections during simulation
    pub rate_limit_rejections: usize,
    /// Total funding costs paid (perpetuals)
    pub total_funding_paid: f64,
    /// Maker fees paid
    pub maker_fees: f64,
    /// Taker fees paid (if any aggressive orders)
    pub taker_fees: f64,
    
    // === Volume-Tiered Fee Tracking ===
    /// Starting 30-day volume (from config)
    pub starting_30d_volume: f64,
    /// Final 30-day rolling volume
    pub final_30d_volume: f64,
    /// Starting fee rate (maker)
    pub starting_fee_rate: f64,
    /// Final fee rate (maker) after volume accumulation
    pub final_fee_rate: f64,
    /// Number of fee tier upgrades during simulation
    pub fee_tier_upgrades: usize,
}

impl From<RealisticSimResult> for crate::types::BacktestResult {
    fn from(result: RealisticSimResult) -> Self {
        crate::types::BacktestResult {
            total_pnl: BigDecimal::try_from(result.total_pnl).unwrap_or_default(),
            max_drawdown: BigDecimal::try_from(result.max_drawdown).unwrap_or_default(),
            sharpe_ratio: Some(result.sharpe_ratio),
            num_trades: result.num_trades,
            profit_factor: Some(result.profit_factor),
            win_rate: Some(result.win_rate),
            equity_curve: result.equity_curve,
            closed_trades: result.num_trades,
            // Transaction costs from realistic simulation
            transaction_costs: Some(crate::types::TransactionCostAnalysis {
                total_commission: result.total_fees,
                total_slippage: 0.0,  // Realistic sim doesn't model slippage separately
                total_market_impact: 0.0,
                total_transaction_costs: result.total_fees,
                costs_as_percentage_of_pnl: if result.total_pnl.abs() > 0.01 {
                    (result.total_fees / result.total_pnl.abs()) * 100.0
                } else {
                    0.0
                },
                avg_cost_per_trade: if result.num_trades > 0 {
                    result.total_fees / result.num_trades as f64
                } else {
                    0.0
                },
                commission_by_exchange: std::collections::HashMap::new(),
            }),
            // Trade returns for Monte Carlo
            trade_returns: result.trade_returns,
            ..Default::default()
        }
    }
}

/// Realistic order book simulator with dynamic fee tiers
pub struct RealisticSimulator {
    initial_capital: f64,
    fee_config: config::ExchangeFeeConfig,
}

impl RealisticSimulator {
    pub fn new(initial_capital: f64, _fee_rate: f64) -> Self {
        // Use default Kraken config for backward compatibility
        Self {
            initial_capital,
            fee_config: config::ExchangeFeeConfig::kraken(),
        }
    }
    
    /// Create simulator with specific exchange configuration
    pub fn with_exchange_config(initial_capital: f64, fee_config: config::ExchangeFeeConfig) -> Self {
        Self {
            initial_capital,
            fee_config,
        }
    }
    
    /// Run simulation over order book events with dynamic volume-based fees
    pub fn run(&self, events: &[OrderBookEvent], params: &RealisticASParams) -> RealisticSimResult {
        use rand::prelude::*;
        use rand_distr::Normal;
        
        let mut rng = rand::thread_rng();
        let latency_dist = Normal::new(params.latency_mean_ms, params.latency_std_ms).unwrap_or(Normal::new(10.0, 1.0).unwrap());
        
        // Initialize volume tracker with starting 30d volume from config
        let mut volume_tracker = VolumeTracker::with_starting_volume(self.fee_config.starting_30d_volume);
        let starting_30d_volume = self.fee_config.starting_30d_volume;
        let starting_fee_rate = self.fee_config.get_fee_rate(starting_30d_volume, true);
        let mut current_fee_rate = starting_fee_rate;
        let mut fee_tier_upgrades = 0usize;
        
        let mut book = SimpleOrderBook::new();
        
        // Initialize with initial_inventory_pct
        // If > 0, we need to find the first TRADE price and allocate inventory
        // Capital is reduced by the value of initial inventory
        // IMPORTANT: Use first TRADE event price, not any order price, for accurate mark-to-market
        let initial_price = events.iter()
            .find_map(|e| if e.event_type == EventType::Trade && e.price > 0.0 { Some(e.price) } else { None })
            .unwrap_or_else(|| {
                // Fallback: use first event with any price
                events.iter()
                    .find_map(|e| if e.price > 0.0 { Some(e.price) } else { None })
                    .unwrap_or(1.0)
            });
        
        let initial_inventory_value = self.initial_capital * params.initial_inventory_pct;
        let initial_inventory_qty = initial_inventory_value / initial_price;
        
        let mut capital: f64 = self.initial_capital - initial_inventory_value;
        let mut inventory: f64 = initial_inventory_qty;
        let mut total_fees: f64 = 0.0;
        let mut total_volume: f64 = 0.0;
        
        // DEBUG: Log capital immediately after initialization
        eprintln!("💵 [CAPITAL_INIT] initial_capital={:.2}, initial_inventory_value={:.2}, capital={:.2}, inventory={:.4}",
            self.initial_capital, initial_inventory_value, capital, inventory);
        eprintln!("💵 [CAPITAL_INIT] Expected equity = capital + inventory * initial_price = {:.2} + {:.4} * {:.4} = {:.2}",
            capital, inventory, initial_price, capital + inventory * initial_price);
        
        // Initialize last_price to initial_price so first equity calculation is correct
        // This ensures equity starts at initial_capital (no immediate mark-to-market loss)
        let mut last_price: Option<f64> = Some(initial_price);
        
        // Log initial setup
        if params.initial_inventory_pct > 0.0 {
            eprintln!("📊 [SIM] Initial inventory: {:.4} units @ ${:.4} = ${:.2} ({}% of capital)",
                initial_inventory_qty, initial_price, initial_inventory_value, 
                (params.initial_inventory_pct * 100.0) as i32);
        }
        
        // Track our pending orders
        let mut our_bid: Option<PendingOrder> = None;
        let mut our_ask: Option<PendingOrder> = None;
        let mut next_order_id = 0u64;
        
        // Trading metrics
        let mut trades: Vec<(f64, f64)> = Vec::new();  // (pnl, timestamp)
        let mut equity_curve = Vec::with_capacity(events.len() / 100);
        let mut returns: Vec<f64> = Vec::new();
        
        // Fill statistics
        let mut orders_placed = 0usize;
        let mut orders_filled = 0usize;
        let mut partial_fills = 0usize;
        let mut queue_positions_at_fill: Vec<f64> = Vec::new();
        
        // Debug counters (local to this simulation run, reset per call)
        let mut trade_debug_count = 0usize;
        let mut quote_debug_count = 0usize;
        let mut orders_this_second = 0usize;
        let mut last_rate_limit_window: i64 = 0;
        let mut rate_limited_until: i64 = 0;
        let mut rate_limit_rejections = 0usize;
        
        // Debug: track skipped quotes
        let mut skipped_no_price = 0usize;
        let mut quote_refresh_attempts = 0usize;
        
        // Debug: track our quote prices vs trade prices
        let mut our_bid_prices: Vec<f64> = Vec::new();
        let mut our_ask_prices: Vec<f64> = Vec::new();
        let mut trade_prices_debug: Vec<f64> = Vec::new();
        
        // Debug: print first 20 NEW events (buys and sells separately) to understand book construction
        if events.len() > 100 {
            let mut buy_count = 0;
            let mut sell_count = 0;
            eprintln!("📊 [SIM_DEBUG] First NEW events by side:");
            eprintln!("  === BUY (bid) orders ===");
            for event in events.iter() {
                if event.event_type == EventType::New && event.side == Side::Buy && buy_count < 5 {
                    eprintln!("  BUY  price={:.6} qty={:.4} id={}", event.price, event.quantity, &event.order_id);
                    buy_count += 1;
                }
                if buy_count >= 5 { break; }
            }
            eprintln!("  === SELL (ask) orders ===");
            for event in events.iter() {
                if event.event_type == EventType::New && event.side == Side::Sell && sell_count < 5 {
                    eprintln!("  SELL price={:.6} qty={:.4} id={}", event.price, event.quantity, &event.order_id);
                    sell_count += 1;
                }
                if sell_count >= 5 { break; }
            }
            
            // Also find min/max prices per side to detect inversion
            let mut min_buy_price = f64::MAX;
            let mut max_buy_price = f64::MIN;
            let mut min_sell_price = f64::MAX;
            let mut max_sell_price = f64::MIN;
            for event in events.iter() {
                if event.event_type == EventType::New {
                    match event.side {
                        Side::Buy => {
                            min_buy_price = min_buy_price.min(event.price);
                            max_buy_price = max_buy_price.max(event.price);
                        }
                        Side::Sell => {
                            min_sell_price = min_sell_price.min(event.price);
                            max_sell_price = max_sell_price.max(event.price);
                        }
                    }
                }
            }
            eprintln!("📊 [SIM_DEBUG] Price ranges from all NEW events:");
            eprintln!("  BUY  (bids): min={:.6}, max={:.6}", min_buy_price, max_buy_price);
            eprintln!("  SELL (asks): min={:.6}, max={:.6}", min_sell_price, max_sell_price);
            if max_buy_price > min_sell_price {
                eprintln!("⚠️  [SIM_DEBUG] DATA INVERSION DETECTED: max_buy ({:.6}) > min_sell ({:.6})", 
                    max_buy_price, min_sell_price);
                eprintln!("     This means BUY and SELL sides are swapped in the source data!");
            }
        }
        
        // NOTE: We no longer auto-swap sides. The Kraken L3 data has correct side labels.
        // The overlap in price ranges (buy prices overlapping sell prices) happens because
        // the data spans months - orders placed when market was at $1.30 overlap with orders
        // placed when market moved to $1.70. This is temporal, not a labeling error.
        // Instead, we filter out stale orders in the event loop based on trade prices.
        let sides_inverted = false;
        
        // Funding rate tracking (only applies to perpetuals, not spot)
        let mut last_funding_time: i64 = 0;
        let mut total_funding_paid = 0.0;
        
        // Volatility tracking (simplified EWMA)
        let mut volatility: f64 = 0.01;  // Start with 1%
        // Note: last_price is initialized earlier to initial_price for proper mark-to-market
        const EWMA_ALPHA: f64 = 0.94;
        
        // Time tracking for order refresh
        let mut last_quote_time: i64 = 0;
        const QUOTE_REFRESH_MS: i64 = 1000;  // Refresh quotes every second
        
        // Snapshot boundary detection - clear book when there's a gap in data
        // A gap indicates DataEngine restarted and received a new snapshot
        // Use 5 minutes (300s) - normal market quiet periods can have gaps of 30-60s
        let mut last_event_timestamp: i64 = 0;
        let mut snapshot_count: usize = 0;
        const SNAPSHOT_GAP_THRESHOLD_MS: i64 = 300_000;  // 5 minutes = new snapshot
        
        let mut peak_equity: f64 = capital;
        let mut max_drawdown: f64 = 0.0;
        let mut event_count: usize = 0;
        let mut cancel_attempts: usize = 0;
        let mut cancel_successes: usize = 0;
        let mut first_cancel_failures: Vec<String> = Vec::new();
        
        for event in events {
            event_count += 1;
            
            // === SNAPSHOT BOUNDARY DETECTION ===
            // If there's a gap > 5 seconds, a new snapshot started
            // Clear the book to avoid stale order accumulation
            if last_event_timestamp > 0 {
                let gap_ms = event.timestamp_ms - last_event_timestamp;
                if gap_ms > SNAPSHOT_GAP_THRESHOLD_MS {
                    snapshot_count += 1;
                    let old_bids = book.bids.len();
                    let old_asks = book.asks.len();
                    book.clear();
                    // Also cancel our pending orders - they're from the old snapshot context
                    our_bid = None;
                    our_ask = None;
                    eprintln!("📸 [SNAPSHOT] Detected gap of {:.1}s - cleared book (had {} bids, {} asks). Snapshot #{}",
                        gap_ms as f64 / 1000.0, old_bids, old_asks, snapshot_count);
                }
            }
            last_event_timestamp = event.timestamp_ms;
            
            // Debug: print book state after first 100 events
            if event_count == 100 && events.len() > 100 {
                eprintln!("📊 [SIM_DEBUG] Book state after 100 events:");
                eprintln!("  - bids: {} levels, asks: {} levels", book.bids.len(), book.asks.len());
                eprintln!("  - best_bid: {:?}, best_ask: {:?}", book.best_bid(), book.best_ask());
                if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                    if bid > ask {
                        eprintln!("  ❌ INVERTED BOOK DETECTED! bid ({:.4}) > ask ({:.4})", bid, ask);
                    }
                }
            }
            
            // === Funding Rate Settlement ===
            if params.funding_rate_8h != 0.0 && inventory.abs() > 1e-10 {
                let funding_intervals_passed = (event.timestamp_ms - last_funding_time) / params.funding_interval_ms;
                if funding_intervals_passed > 0 {
                    // Pay/receive funding based on position direction
                    // Long pays when rate is positive, short receives
                    let mark_price = book.mid_price().unwrap_or_else(|| last_price.unwrap_or(1.0));
                    let position_value = inventory.abs() * mark_price;
                    let funding_payment = position_value * params.funding_rate_8h * funding_intervals_passed as f64;
                    
                    if inventory > 0.0 {
                        // Long position pays funding when rate is positive
                        capital -= funding_payment;
                        total_funding_paid += funding_payment;
                    } else {
                        // Short position receives funding when rate is positive
                        capital += funding_payment;
                        total_funding_paid -= funding_payment;
                    }
                    
                    last_funding_time = event.timestamp_ms - (event.timestamp_ms % params.funding_interval_ms);
                }
            }
            
            // === Rate Limit Window Reset ===
            let current_second = event.timestamp_ms / 1000;
            if current_second != last_rate_limit_window / 1000 {
                orders_this_second = 0;
                last_rate_limit_window = event.timestamp_ms;
            }
            
            // Update book state based on event type
            // If sides are inverted in the data, swap them
            let effective_side = if sides_inverted {
                match event.side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                }
            } else {
                event.side
            };
            
            match event.event_type {
                EventType::New => {
                    // Skip orders that would cause or worsen book inversion
                    // These are likely stale limit orders from before our data window
                    if let Some(ref_price) = last_price {
                        let deviation = (event.price - ref_price).abs() / ref_price;
                        // Skip orders more than 20% from last trade price - they're likely stale
                        if deviation > 0.20 {
                            continue;
                        }
                    }
                    
                    // Also skip if this specific order would cause inversion
                    if book.would_cause_inversion(effective_side, event.price) {
                        // This order crossed the spread - likely a stale limit order
                        continue;
                    }
                    
                    book.add_order(effective_side, event.price, event.quantity, &event.order_id);
                    
                    // Debug: detect first book inversion
                    if event_count < 1000 {
                        if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                            if bid > ask {
                                eprintln!("❌ [SIM_DEBUG] Book inverted at event {}!", event_count);
                                eprintln!("  Event: {:?} {:?} price={:.4} qty={:.4}", 
                                    event.event_type, event.side, event.price, event.quantity);
                                eprintln!("  Result: best_bid={:.4} > best_ask={:.4}", bid, ask);
                            }
                        }
                    }
                }
                EventType::Modify => {
                    book.modify_order(&event.order_id, event.quantity);
                }
                EventType::Cancel => {
                    cancel_attempts += 1;
                    if book.cancel_order(&event.order_id) {
                        cancel_successes += 1;
                    } else if first_cancel_failures.len() < 5 {
                        first_cancel_failures.push(format!(
                            "event#{}: id={} side={:?} price={:.4}", 
                            event_count, &event.order_id, event.side, event.price
                        ));
                    }
                }
                EventType::Trade => {
                    // DEBUG: Use local counter (not static) so it resets per simulation
                    trade_debug_count += 1;
                    
                    // DEBUG: Show what we're checking against for first 20 trades
                    if trade_debug_count <= 20 {
                        eprintln!("🔍 [TRADE_DEBUG] Trade #{}: price={:.4}, side={:?}, eff_side={:?}",
                            trade_debug_count, event.price, event.side, effective_side);
                        eprintln!("    our_bid: {:?}", our_bid.as_ref().map(|o| (o.price, o.quantity_ahead)));
                        eprintln!("    our_ask: {:?}", our_ask.as_ref().map(|o| (o.price, o.quantity_ahead)));
                        eprintln!("    orders_placed_so_far: {}, last_price: {:?}", orders_placed, last_price);
                    }
                    
                    // Sanitize book on first trade to establish reference price
                    // Remove orders that are unreasonably far from trade price
                    if last_price.is_none() && event.price > 0.0 {
                        let removed = book.sanitize(event.price, 0.10); // 10% max deviation
                        if removed > 0 {
                            eprintln!("🧹 [SIM_DEBUG] Sanitized {} stale price levels on first trade at {:.4}", removed, event.price);
                        }
                    }
                    
                    // Execute trade in book (use effective_side for book operations)
                    let _consumed = book.execute_trade(effective_side, event.price, event.quantity);
                    
                    // Check if our orders got filled
                    // A buy aggressor hits asks; a sell aggressor hits bids
                    let check_our_order = match effective_side {
                        Side::Buy => &mut our_ask,  // Buy aggressor may fill our ask
                        Side::Sell => &mut our_bid,  // Sell aggressor may fill our bid
                    };
                    
                    // DEBUG: Log trade-vs-quote comparison for first 20 trades where we have an order
                    if trade_debug_count <= 20 {
                        if let Some(ref our_order) = check_our_order {
                            let book_mid = book.mid_price().unwrap_or(0.0);
                            eprintln!("🔍 [FILL_DEBUG] Trade #{}: price={:.4}, our_{:?}={:.4}, book_mid={:.4}, diff={:.4}bps",
                                trade_debug_count, event.price, our_order.side, our_order.price, book_mid,
                                ((event.price - our_order.price).abs() / our_order.price) * 10000.0);
                        }
                    }
                    
                    if let Some(ref mut our_order) = check_our_order {
                        // For trade-only data (no order book events), we can't track queue position.
                        // Instead, check if the trade price crosses our quote:
                        // - For our bid: sell aggressor trades at price <= our bid (we buy)
                        // - For our ask: buy aggressor trades at price >= our ask (we sell)
                        let price_matches = (our_order.price - event.price).abs() < 1e-10;
                        let trade_crosses_our_quote = match our_order.side {
                            Side::Buy => event.price <= our_order.price,   // Seller willing to sell at/below our bid
                            Side::Sell => event.price >= our_order.price,  // Buyer willing to buy at/above our ask
                        };
                        
                        // For trade-only data (empty book), use probabilistic fills based on proximity
                        // If trade is within our spread of the mid, we have a chance to get filled
                        // This models the reality that we'd be competitive if we were actually in the book
                        let trade_only_mode = book.bids.is_empty() && book.asks.is_empty();
                        let probabilistic_fill = if trade_only_mode && !price_matches && !trade_crosses_our_quote {
                            // Calculate how close the trade is to our quote as a percentage
                            let price_diff = (event.price - our_order.price).abs();
                            let price_tolerance = our_order.price * 0.002; // 0.2% tolerance (20 bps)
                            
                            if price_diff <= price_tolerance {
                                // The closer the trade is to our price, the higher our fill probability
                                // At our exact price: 80% fill chance, linear decay to 0% at tolerance edge
                                let fill_prob = 0.8 * (1.0 - price_diff / price_tolerance);
                                rng.gen::<f64>() < fill_prob
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        
                        // DEBUG: Log why fills aren't happening for first few potential fills
                        if trade_debug_count <= 20 {
                            let price_diff_bps = ((event.price - our_order.price).abs() / our_order.price) * 10000.0;
                            if trade_crosses_our_quote || price_matches {
                                eprintln!("🔍 [FILL_DEBUG] CROSSES: trade={:.4} vs our_{:?}={:.4} (diff={:.0}bps), qty_ahead={:.2}",
                                    event.price, our_order.side, our_order.price, price_diff_bps, our_order.quantity_ahead);
                            } else {
                                eprintln!("🔍 [FILL_DEBUG] NO_CROSS: trade={:.4} vs our_{:?}={:.4} (diff={:.0}bps)",
                                    event.price, our_order.side, our_order.price, price_diff_bps);
                            }
                        }
                        
                        if price_matches || trade_crosses_our_quote || probabilistic_fill {
                            // Trade at or through our price level - check queue position
                            our_order.quantity_ahead -= event.quantity;
                            
                            // === Partial Fill Modeling ===
                            let should_fill = our_order.quantity_ahead <= 0.0;
                            
                            // DEBUG: Log fill decision for first 20 trades
                            if trade_debug_count <= 20 {
                                eprintln!("🔍 [FILL_DEBUG] QUEUE_CHECK: qty_ahead_after={:.2}, should_fill={}, trade_qty={:.4}",
                                    our_order.quantity_ahead, should_fill, event.quantity);
                            }
                            
                            let fill_qty = if should_fill {
                                if params.enable_partial_fills && event.quantity < our_order.quantity {
                                    // Partial fill: we get filled for the remaining trade volume
                                    let available = event.quantity + our_order.quantity_ahead.min(0.0).abs();
                                    let partial = available.min(our_order.quantity);
                                    if partial >= our_order.quantity * params.min_fill_fraction {
                                        partial_fills += 1;
                                        if trade_debug_count <= 20 {
                                            eprintln!("🔍 [FILL_DEBUG] PARTIAL_FILL: qty={:.4}", partial);
                                        }
                                        partial
                                    } else {
                                        if trade_debug_count <= 20 {
                                            eprintln!("🔍 [FILL_DEBUG] FULL_FILL (partial too small): qty={:.4}", our_order.quantity);
                                        }
                                        our_order.quantity // Full fill
                                    }
                                } else {
                                    if trade_debug_count <= 20 {
                                        eprintln!("🔍 [FILL_DEBUG] FULL_FILL: qty={:.4}", our_order.quantity);
                                    }
                                    our_order.quantity // Full fill
                                }
                            } else {
                                0.0 // No fill yet
                            };
                            
                            if fill_qty > 0.0 {
                                let fill_price = our_order.price;
                                let trade_volume_usd = fill_qty * fill_price;
                                
                                // Update 30-day rolling volume and get dynamic fee rate
                                let current_30d_volume = volume_tracker.add_trade(event.timestamp_ms, trade_volume_usd);
                                let dynamic_maker_fee = self.fee_config.get_fee_rate(current_30d_volume, true);
                                let fee = trade_volume_usd * dynamic_maker_fee;
                                
                                // Track fee tier upgrades (when fee rate decreases)
                                if dynamic_maker_fee < current_fee_rate {
                                    fee_tier_upgrades += 1;
                                    current_fee_rate = dynamic_maker_fee;
                                }
                                
                                let capital_before = capital;
                                match our_order.side {
                                    Side::Buy => {
                                        // Bought: pay price, receive inventory
                                        capital -= fill_qty * fill_price + fee;
                                        inventory += fill_qty;
                                    }
                                    Side::Sell => {
                                        // Sold: receive price, reduce inventory
                                        capital += fill_qty * fill_price - fee;
                                        inventory -= fill_qty;
                                    }
                                }
                                
                                // DEBUG: Log first 5 capital changes
                                if orders_filled < 5 {
                                    eprintln!("💵 [CAPITAL_CHANGE] Fill #{}: {:?} {:.4} @ {:.4}, capital: {:.2} -> {:.2}, inventory: {:.4}",
                                        orders_filled + 1, our_order.side, fill_qty, fill_price, capital_before, capital, inventory);
                                }
                                
                                total_fees += fee;
                                total_volume += trade_volume_usd;
                                orders_filled += 1;
                                queue_positions_at_fill.push(-our_order.quantity_ahead.max(0.0));
                                
                                // Record trade
                                let mark_price = book.mid_price().unwrap_or(fill_price);
                                let pnl = match our_order.side {
                                    Side::Buy => (mark_price - fill_price) * fill_qty - fee,
                                    Side::Sell => (fill_price - mark_price) * fill_qty - fee,
                                };
                                trades.push((pnl, event.timestamp_ms as f64));
                                
                                *check_our_order = None;
                            }
                        }
                    }
                    
                    // Update volatility estimate
                    if let Some(last) = last_price {
                        let ret: f64 = (event.price / last).ln();
                        volatility = (EWMA_ALPHA * volatility.powi(2) + (1.0 - EWMA_ALPHA) * ret.powi(2)).sqrt();
                    }
                    last_price = Some(event.price);
                    trade_prices_debug.push(event.price);  // Debug tracking
                }
            }
            
            // === Latency Jitter Calculation ===
            let current_latency = latency_dist.sample(&mut rng).max(1.0) as i64;
            
            // Periodically update our quotes (accounting for latency with jitter)
            if event.timestamp_ms - last_quote_time >= QUOTE_REFRESH_MS + current_latency {
                // === Rate Limit Check ===
                if event.timestamp_ms < rate_limited_until {
                    // Still in cooldown, skip this quote update
                    continue;
                }
                
                if orders_this_second as f64 >= params.max_orders_per_second {
                    // Hit rate limit, enter cooldown
                    rate_limited_until = event.timestamp_ms + params.rate_limit_cooldown_ms;
                    rate_limit_rejections += 1;
                    continue;
                }
                
                last_quote_time = event.timestamp_ms;
                quote_refresh_attempts += 1;
                
                // Get current market state
                // CRITICAL: Trade prices are the ground truth of where the market IS.
                // The order book can drift from trade prices when:
                // 1. L3 data spans long time periods (months)
                // 2. Stale limit orders accumulate at old price levels
                // 3. Synthetic data generates orders that don't match trades
                // 
                // For market making simulation, we ALWAYS want to quote where trades happen.
                // The book mid is useful for queue position modeling, but NOT for pricing.
                let mid_price = match (last_price, book.mid_price()) {
                    (Some(trade_price), _) => trade_price,  // ALWAYS prefer trade price
                    (None, Some(book_mid)) => book_mid,     // No trades yet, use book
                    (None, None) => {
                        skipped_no_price += 1;
                        continue;  // No price reference at all, skip
                    }
                };
                
                // Apply volatility cap
                let effective_vol = volatility.min(params.volatility_cap);
                // Note: We previously had a check to skip quoting if effective_vol < 0.15
                // but this was causing 0 trades in low-volatility periods. Market makers
                // should quote in all volatility environments, using spread adjustment instead.
                
                // Calculate max inventory limit
                // Use initial_capital for order sizing to maintain consistent position sizes
                // regardless of current P&L. This simulates a fixed notional trading strategy.
                let order_size = self.initial_capital * params.order_size_pct;
                let max_capital_position = self.initial_capital / mid_price;  // Max if all capital was in asset
                let max_inventory = max_capital_position * params.max_inventory_multiple;
                
                // Check if we're at inventory limits  
                // For buying: can't exceed max_inventory
                // For selling: we need positive inventory, but don't restrict below 0 (allow shorts up to max)
                let can_buy = inventory < max_inventory;
                let can_sell = inventory > -max_inventory;
                
                // Avellaneda-Stoikov spread calculation
                let gamma = params.risk_aversion;
                let sigma = effective_vol;
                let t_horizon = params.time_horizon_minutes / 1440.0;  // Convert to days
                let q = inventory;
                
                // Reservation price: mid - gamma * sigma^2 * T * q
                let reservation_price = mid_price - gamma * sigma.powi(2) * t_horizon * q * mid_price;
                
                // Optimal spread: gamma * sigma^2 * T + 2/gamma * ln(1 + gamma/k)
                // NOTE: The original A-S formula produces a dimensionless spread.
                // We need to multiply by mid_price to get dollar-denominated spread.
                // Also, the ln term can dominate - we clamp it to be reasonable.
                let k = 1.5;  // Market maker intensity
                let variance_term = gamma * sigma.powi(2) * t_horizon;
                let ln_term = (2.0 / gamma) * (1.0 + gamma / k).ln();
                // The ln_term can be huge (>1.0) which makes no sense for spread
                // Clamp it to a reasonable maximum (e.g., 1% = 0.01)
                let clamped_ln_term = ln_term.min(0.01);
                let optimal_spread = (variance_term + clamped_ln_term) * mid_price;
                
                // Apply minimum spread: must be at least 2x the current maker fee to be profitable
                // This ensures we capture spread > fees on a round trip
                // Use the dynamic fee rate (current_fee_rate) which adjusts with volume tiers
                let fee_based_min_spread_bps = current_fee_rate * 10000.0 * 2.5;  // 2.5x fee in bps
                let effective_min_spread_bps = params.min_spread_bps.max(fee_based_min_spread_bps);
                let min_spread = mid_price * effective_min_spread_bps / 10000.0;
                let half_spread = (optimal_spread / 2.0).max(min_spread / 2.0);
                
                // Calculate quote prices
                // CRITICAL INSIGHT: The book's best bid/ask can be stale and far from actual trades.
                // What matters for getting fills is being competitive with WHERE TRADES ACTUALLY HAPPEN.
                // 
                // Strategy: Quote at the minimum profitable spread around mid price.
                // The spread must exceed fees to be profitable.
                let (bid_price, ask_price) = if last_price.is_some() {
                    // Quote at half_spread around mid (which is the trade price)
                    // half_spread is already the larger of AS optimal or fee-based minimum
                    (mid_price - half_spread, mid_price + half_spread)
                } else if let (Some(best_bid), Some(best_ask)) = (book.best_bid(), book.best_ask()) {
                    // No trades yet - use book inside (may be less accurate)
                    (best_bid, best_ask)
                } else {
                    // No reference - use AS calculated spread
                    (reservation_price - half_spread, reservation_price + half_spread)
                };
                
                // DEBUG: Log first 5 quote placements (using local counter)
                quote_debug_count += 1;
                if quote_debug_count <= 5 {
                    eprintln!("📍 [QUOTE_DEBUG] #{}: mid={:.4} (book_mid={:?}, last_trade={:?}), bid={:.4}, ask={:.4}",
                        quote_debug_count, mid_price, book.mid_price(), last_price, bid_price, ask_price);
                }
                
                // Cancel old orders (counts against rate limit)
                if our_bid.is_some() {
                    let old_bid = our_bid.take().unwrap();
                    let _ = book.cancel_order(&old_bid.order_id);
                    orders_this_second += 1;
                }
                if our_ask.is_some() {
                    let old_ask = our_ask.take().unwrap();
                    let _ = book.cancel_order(&old_ask.order_id);
                    orders_this_second += 1;
                }
                
                // Place new bid (only if we can buy - not at max inventory)
                if can_buy && (orders_this_second as f64) < params.max_orders_per_second {
                let bid_order_id = format!("our_bid_{}", next_order_id);
                next_order_id += 1;
                let quantity_at_bid = book.quantity_at_price(Side::Buy, bid_price);
                book.add_order(Side::Buy, bid_price, order_size / bid_price, &bid_order_id);
                our_bid = Some(PendingOrder {
                    order_id: bid_order_id,
                    side: Side::Buy,
                    price: bid_price,
                    quantity: order_size / bid_price,
                    timestamp_ms: event.timestamp_ms,
                    quantity_ahead: quantity_at_bid,
                });
                orders_placed += 1;
                orders_this_second += 1;
                our_bid_prices.push(bid_price);  // Debug tracking
                } // End can_buy check
                
                // Place new ask (only if we can sell - not at min inventory)
                if can_sell && (orders_this_second as f64) < params.max_orders_per_second {
                let ask_order_id = format!("our_ask_{}", next_order_id);
                next_order_id += 1;
                let quantity_at_ask = book.quantity_at_price(Side::Sell, ask_price);
                book.add_order(Side::Sell, ask_price, order_size / ask_price, &ask_order_id);
                our_ask = Some(PendingOrder {
                    order_id: ask_order_id,
                    side: Side::Sell,
                    price: ask_price,
                    quantity: order_size / ask_price,
                    timestamp_ms: event.timestamp_ms,
                    quantity_ahead: quantity_at_ask,
                });
                our_ask_prices.push(ask_price);  // Debug tracking
                orders_placed += 1;
                orders_this_second += 1;
                } // End can_sell check
                
                // Track equity
                let mark_price = mid_price;
                let equity = capital + inventory * mark_price;
                
                // DEBUG: Log first equity calculation to trace the -$20k issue
                if equity_curve.is_empty() {
                    eprintln!("💰 [EQUITY_DEBUG] FIRST equity calculation:");
                    eprintln!("    capital = {:.2}", capital);
                    eprintln!("    inventory = {:.4}", inventory);
                    eprintln!("    mark_price = {:.4}", mark_price);
                    eprintln!("    equity = capital + inventory * mark_price = {:.2}", equity);
                    eprintln!("    initial_capital = {:.2}", self.initial_capital);
                }
                
                equity_curve.push(equity);
                
                // Track drawdown
                if equity > peak_equity {
                    peak_equity = equity;
                }
                let dd = (peak_equity - equity) / peak_equity;
                if dd > max_drawdown {
                    max_drawdown = dd;
                }
                
                // Track returns for Sharpe
                if equity_curve.len() > 1 {
                    let prev = equity_curve[equity_curve.len() - 2];
                    if prev > 0.0 {
                        returns.push((equity - prev) / prev);
                    }
                }
            }
        }
        
        // Final mark-to-market
        let final_mark = last_price.unwrap_or(1.0);
        let final_equity = capital + inventory * final_mark;
        let total_pnl = final_equity - self.initial_capital;
        
        // Calculate Sharpe ratio (annualized)
        let sharpe_ratio: f64 = if returns.len() > 10 {
            let mean_return: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
            let variance: f64 = returns.iter().map(|r| (r - mean_return).powi(2)).sum::<f64>() / returns.len() as f64;
            let std_dev: f64 = variance.sqrt();
            if std_dev > 1e-10 {
                // Assume 1 second samples, ~31.5M seconds/year
                let annualization: f64 = (365.25_f64 * 24.0 * 3600.0 / 1.0).sqrt();
                (mean_return / std_dev) * annualization
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // Calculate win rate and profit factor
        let wins: Vec<f64> = trades.iter().filter(|(pnl, _)| *pnl > 0.0).map(|(pnl, _)| *pnl).collect();
        let losses: Vec<f64> = trades.iter().filter(|(pnl, _)| *pnl < 0.0).map(|(pnl, _)| pnl.abs()).collect();
        
        let win_rate = if !trades.is_empty() {
            wins.len() as f64 / trades.len() as f64
        } else {
            0.0
        };
        
        let profit_factor = {
            let total_wins: f64 = wins.iter().sum();
            let total_losses: f64 = losses.iter().sum();
            if total_losses > 0.0 {
                (total_wins / total_losses).min(100.0) // Cap at reasonable max
            } else if total_wins > 0.0 {
                10.0 // All wins, no losses - use a high but reasonable value
            } else {
                1.0 // No trades - neutral
            }
        };
        
        // Fill statistics
        let avg_fill_rate = if orders_placed > 0 {
            orders_filled as f64 / orders_placed as f64
        } else {
            0.0
        };
        
        let avg_queue_position = if !queue_positions_at_fill.is_empty() {
            queue_positions_at_fill.iter().sum::<f64>() / queue_positions_at_fill.len() as f64
        } else {
            0.0
        };
        
        // Extract individual trade P&L values for Monte Carlo
        let trade_returns: Vec<f64> = trades.iter().map(|(pnl, _)| *pnl).collect();
        
        // Debug logging for 0 trades investigation
        if trades.is_empty() && events.len() > 1000 {
            eprintln!("⚠️ [SIM_DEBUG] 0 trades with {} events!", events.len());
            eprintln!("  - snapshot_boundaries_detected: {}", snapshot_count);
            eprintln!("  - quote_refresh_attempts: {}", quote_refresh_attempts);
            eprintln!("  - skipped_no_price: {}", skipped_no_price);
            eprintln!("  - orders_placed: {}", orders_placed);
            eprintln!("  - orders_filled: {}", orders_filled);
            eprintln!("  - rate_limit_rejections: {}", rate_limit_rejections);
            eprintln!("  - cancel_attempts: {}, cancel_successes: {} ({:.1}%)", 
                cancel_attempts, cancel_successes, 
                if cancel_attempts > 0 { 100.0 * cancel_successes as f64 / cancel_attempts as f64 } else { 0.0 });
            if !first_cancel_failures.is_empty() {
                eprintln!("  - first cancel failures:");
                for fail in &first_cancel_failures {
                    eprintln!("      {}", fail);
                }
            }
            eprintln!("  - final book state: {} bids, {} asks", book.bids.len(), book.asks.len());
            eprintln!("  - best_bid: {:?}, best_ask: {:?}", book.best_bid(), book.best_ask());
            eprintln!("  - last_price: {:?}", last_price);
            eprintln!("  - final volatility: {:.6}", volatility);
            
            // Analyze trade prices vs our quote positions
            let trade_events: Vec<_> = events.iter()
                .filter(|e| matches!(e.event_type, EventType::Trade))
                .collect();
            if !trade_events.is_empty() {
                let trade_prices: Vec<f64> = trade_events.iter().map(|e| e.price).collect();
                let min_trade = trade_prices.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_trade = trade_prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let avg_trade = trade_prices.iter().sum::<f64>() / trade_prices.len() as f64;
                eprintln!("  - trade prices: min={:.4}, avg={:.4}, max={:.4}, count={}", 
                    min_trade, avg_trade, max_trade, trade_prices.len());
            }
            
            // Compare our quotes to trade prices
            if !our_bid_prices.is_empty() && !our_ask_prices.is_empty() && !trade_prices_debug.is_empty() {
                let min_bid = our_bid_prices.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_bid = our_bid_prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let avg_bid = our_bid_prices.iter().sum::<f64>() / our_bid_prices.len() as f64;
                let min_ask = our_ask_prices.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_ask = our_ask_prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let avg_ask = our_ask_prices.iter().sum::<f64>() / our_ask_prices.len() as f64;
                let avg_trade = trade_prices_debug.iter().sum::<f64>() / trade_prices_debug.len() as f64;
                
                eprintln!("  - OUR BIDS:  min={:.4}, avg={:.4}, max={:.4}", min_bid, avg_bid, max_bid);
                eprintln!("  - OUR ASKS:  min={:.4}, avg={:.4}, max={:.4}", min_ask, avg_ask, max_ask);
                eprintln!("  - TRADES:    avg={:.4}", avg_trade);
                eprintln!("  - Gap: avg_bid to avg_trade = {:.2}%, avg_ask to avg_trade = {:.2}%",
                    (avg_trade - avg_bid) / avg_trade * 100.0,
                    (avg_ask - avg_trade) / avg_trade * 100.0);
            }
            
            if sides_inverted {
                eprintln!("  - ⚠️ SIDES WERE AUTO-SWAPPED due to inversion detection");
            }
            
            // Count event types
            let new_count = events.iter().filter(|e| matches!(e.event_type, EventType::New)).count();
            let cancel_count = events.iter().filter(|e| matches!(e.event_type, EventType::Cancel)).count();
            let modify_count = events.iter().filter(|e| matches!(e.event_type, EventType::Modify)).count();
            let trade_count = events.iter().filter(|e| matches!(e.event_type, EventType::Trade)).count();
            eprintln!("  - event types: new={}, cancel={}, modify={}, trade={}", 
                new_count, cancel_count, modify_count, trade_count);
        }
        
        // ============================================================================
        // PROFITABILITY ANALYSIS
        // ============================================================================
        // Log detailed analysis to understand why strategy might be unprofitable
        if trades.len() >= 5 {
            let avg_trade_pnl = if !trades.is_empty() {
                trades.iter().map(|(pnl, _)| *pnl).sum::<f64>() / trades.len() as f64
            } else { 0.0 };
            
            let avg_trade_volume = if !trades.is_empty() {
                total_volume / trades.len() as f64
            } else { 0.0 };
            
            let avg_fee_per_trade = if !trades.is_empty() {
                total_fees / trades.len() as f64
            } else { 0.0 };
            
            let fee_as_pct_of_volume = if total_volume > 0.0 {
                total_fees / total_volume * 100.0
            } else { 0.0 };
            
            let gross_pnl = total_pnl + total_fees;  // P&L before fees
            let fee_drag = if gross_pnl.abs() > 0.01 {
                (total_fees / gross_pnl.abs()) * 100.0
            } else { 0.0 };
            
            eprintln!("\n📊 [PROFITABILITY ANALYSIS] {} trades on ${:.0} capital", trades.len(), self.initial_capital);
            eprintln!("  💰 P&L Breakdown:");
            eprintln!("      Gross P&L (before fees): ${:.2}", gross_pnl);
            eprintln!("      Total Fees:              ${:.2}", total_fees);
            eprintln!("      Net P&L:                 ${:.2}", total_pnl);
            eprintln!("      Fee Drag:                {:.1}% of gross P&L", fee_drag);
            eprintln!("  📈 Per-Trade Metrics:");
            eprintln!("      Avg Trade P&L:           ${:.4}", avg_trade_pnl);
            eprintln!("      Avg Trade Volume:        ${:.2}", avg_trade_volume);
            eprintln!("      Avg Fee/Trade:           ${:.4}", avg_fee_per_trade);
            eprintln!("      Fee Rate:                {:.3}% (effective)", fee_as_pct_of_volume);
            eprintln!("  📉 Risk Metrics:");
            eprintln!("      Win Rate:                {:.1}%", win_rate * 100.0);
            eprintln!("      Profit Factor:           {:.2}", profit_factor);
            eprintln!("      Max Drawdown:            {:.2}%", max_drawdown * 100.0);
            eprintln!("      Sharpe Ratio:            {:.4}", sharpe_ratio);
            eprintln!("  🏦 Fee Tier Progress:");
            eprintln!("      Starting Fee:            {:.3}%", starting_fee_rate * 100.0);
            eprintln!("      Final Fee:               {:.3}%", current_fee_rate * 100.0);
            eprintln!("      Volume Traded:           ${:.0}", total_volume);
            eprintln!("      Tier Upgrades:           {}", fee_tier_upgrades);
            
            // Diagnose the issue
            eprintln!("  🔍 Diagnosis:");
            if fee_as_pct_of_volume > 0.20 {
                eprintln!("      ⚠️  HIGH FEES: {:.3}% effective fee rate is eating profits", fee_as_pct_of_volume);
                eprintln!("         Consider: wider min_spread_bps or simulate with established 30d volume");
            }
            if win_rate < 0.45 {
                eprintln!("      ⚠️  LOW WIN RATE: {:.1}% suggests adverse selection", win_rate * 100.0);
                eprintln!("         Consider: increase adverse_selection_sensitivity, widen spreads");
            }
            if avg_trade_pnl < 0.0 && avg_fee_per_trade > avg_trade_pnl.abs() {
                eprintln!("      ⚠️  FEES > GROSS PROFIT: Can't overcome fee structure");
                eprintln!("         Consider: increase min_spread_bps to > {:.0} bps", fee_as_pct_of_volume * 100.0 * 2.0);
            }
            if max_drawdown > 0.10 {
                eprintln!("      ⚠️  HIGH DRAWDOWN: {:.1}% suggests inventory accumulation", max_drawdown * 100.0);
                eprintln!("         Consider: reduce max_inventory_multiple, increase risk_aversion");
            }
        }
        
        RealisticSimResult {
            total_pnl,
            sharpe_ratio,
            max_drawdown,
            num_trades: trades.len(),
            win_rate,
            profit_factor,
            total_volume,
            total_fees,
            avg_fill_rate,
            avg_queue_position,
            equity_curve,
            trade_returns,
            partial_fill_count: partial_fills,
            rate_limit_rejections,
            total_funding_paid,
            maker_fees: total_fees, // All fees are maker in this simulation
            taker_fees: 0.0, // No aggressive orders in market making
            starting_30d_volume,
            final_30d_volume: volume_tracker.get_volume(),
            starting_fee_rate,
            final_fee_rate: current_fee_rate,
            fee_tier_upgrades,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_order_book() {
        let mut book = SimpleOrderBook::new();
        
        // Add some orders
        book.add_order(Side::Buy, 100.0, 10.0, "bid1");
        book.add_order(Side::Buy, 100.0, 5.0, "bid2");
        book.add_order(Side::Sell, 101.0, 8.0, "ask1");
        
        assert_eq!(book.best_bid(), Some(100.0));
        assert_eq!(book.best_ask(), Some(101.0));
        assert_eq!(book.quantity_at_price(Side::Buy, 100.0), 15.0);
        
        // Cancel an order
        book.cancel_order("bid1");
        assert_eq!(book.quantity_at_price(Side::Buy, 100.0), 5.0);
        
        // Execute a trade
        book.execute_trade(Side::Buy, 101.0, 3.0);  // Buy aggressor hits ask
        assert_eq!(book.quantity_at_price(Side::Sell, 101.0), 5.0);
    }
    
    #[test]
    fn test_queue_position() {
        let mut book = SimpleOrderBook::new();
        
        book.add_order(Side::Buy, 100.0, 10.0, "order1");
        book.add_order(Side::Buy, 100.0, 20.0, "order2");
        book.add_order(Side::Buy, 100.0, 15.0, "our_order");
        book.add_order(Side::Buy, 100.0, 5.0, "order3");
        
        let ahead = book.quantity_ahead_in_queue(Side::Buy, 100.0, "our_order");
        assert_eq!(ahead, 30.0);  // 10 + 20 ahead of us
    }
}

// ============================================================================
// Helper functions for converting between data formats
// ============================================================================

impl OrderBookEvent {
    /// Create a trade event from SimulationTick data
    /// 
    /// This is a convenience function for when we only have trade data
    /// (no order book events). The simulation will be less accurate
    /// but still usable.
    pub fn from_trade(
        timestamp_ms: i64,
        price: f64,
        quantity: f64,
        is_sell: bool,
        trade_idx: usize,
    ) -> Self {
        Self {
            timestamp_ms,
            event_type: EventType::Trade,
            order_id: format!("trade_{}", trade_idx),
            side: if is_sell { Side::Sell } else { Side::Buy },
            price,
            quantity,
        }
    }
}

/// Convert a slice of ticks to OrderBookEvents
/// 
/// Uses the tick's event_type() to create proper New/Modify/Cancel/Trade events.
/// This enables realistic order book simulation when data includes event types.
/// Convert trade-only tick data to OrderBookEvents
/// 
/// IMPORTANT: This function is used for TRADE-ONLY data fallback when L3 order book
/// data is not available. All events are forced to EventType::Trade because the source
/// data represents executed trades, not order book mutations.
/// 
/// For L3 order book data (new/modify/cancel/trade events), use the From<OrderBookEventTick>
/// implementation instead.
pub fn ticks_to_trade_events<T: dataloader::Tick>(ticks: &[T]) -> Vec<OrderBookEvent> {
    ticks.iter().enumerate().map(|(idx, tick)| {
        // FORCE all events to Trade type - this data represents executed trades,
        // not order book mutations. The event_type field in the database may be
        // incorrectly set to "new" for historical trade data.
        let side = if tick.is_sell() { Side::Sell } else { Side::Buy };
        
        OrderBookEvent {
            timestamp_ms: tick.timestamp_ms(),
            event_type: EventType::Trade,  // Always Trade for trade-only data
            order_id: format!("trade_{}", idx),
            side,
            price: tick.price(),
            quantity: tick.quantity(),
        }
    }).collect()
}

/// Convert from data_prep OrderBookEventTick format
impl From<&data_prep::OrderBookEventTick> for OrderBookEvent {
    fn from(tick: &data_prep::OrderBookEventTick) -> Self {
        let event_type = match tick.event_type.0 {
            0 => EventType::New,
            1 => EventType::Modify,
            2 => EventType::Cancel,
            _ => EventType::Trade,
        };
        let side = if tick.side.is_sell() { Side::Sell } else { Side::Buy };
        
        Self {
            timestamp_ms: tick.timestamp_ms,
            event_type,
            order_id: format!("ob_{}", tick.order_id_hash),
            side,
            price: tick.price,
            quantity: tick.quantity,
        }
    }
}
