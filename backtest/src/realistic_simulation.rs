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

/// Parameters for Avellaneda-Stoikov strategy in realistic simulation
#[derive(Debug, Clone)]
pub struct RealisticASParams {
    pub risk_aversion: f64,
    pub inventory_target: f64,
    pub order_size_pct: f64,
    pub volatility_cap: f64,
    pub min_spread_bps: f64,
    pub time_horizon_minutes: f64,
    pub fee_rate: f64,
}

/// Our strategy's pending order
#[derive(Debug, Clone)]
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
    
    fn add_order(&mut self, side: Side, price: f64, quantity: f64, order_id: &str) {
        let price_key = OrderedFloat(price);
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        
        let level = levels.entry(price_key).or_insert_with(PriceLevel::default);
        level.total_quantity += quantity;
        level.orders.push_back((order_id.to_string(), quantity));
    }
    
    fn cancel_order(&mut self, order_id: &str) {
        // Search both sides
        for levels in [&mut self.bids, &mut self.asks] {
            for (_, level) in levels.iter_mut() {
                if let Some(pos) = level.orders.iter().position(|(id, _)| id == order_id) {
                    let (_, qty) = level.orders.remove(pos).unwrap();
                    level.total_quantity -= qty;
                    return;
                }
            }
        }
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
        self.bids.keys().next_back().map(|k| k.0)
    }
    
    fn best_ask(&self) -> Option<f64> {
        self.asks.keys().next().map(|k| k.0)
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
            ..Default::default()
        }
    }
}

/// Realistic order book simulator
pub struct RealisticSimulator {
    initial_capital: f64,
    fee_rate: f64,
}

impl RealisticSimulator {
    pub fn new(initial_capital: f64, fee_rate: f64) -> Self {
        Self {
            initial_capital,
            fee_rate,
        }
    }
    
    /// Run simulation over order book events
    pub fn run(&self, events: &[OrderBookEvent], params: &RealisticASParams) -> RealisticSimResult {
        let mut book = SimpleOrderBook::new();
        let mut capital = self.initial_capital;
        let mut inventory = 0.0;
        let mut total_fees = 0.0;
        let mut total_volume = 0.0;
        
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
        let mut queue_positions_at_fill: Vec<f64> = Vec::new();
        
        // Volatility tracking (simplified EWMA)
        let mut volatility: f64 = 0.01;  // Start with 1%
        let mut last_price: Option<f64> = None;
        const EWMA_ALPHA: f64 = 0.94;
        
        // Time tracking for order refresh
        let mut last_quote_time: i64 = 0;
        const QUOTE_REFRESH_MS: i64 = 1000;  // Refresh quotes every second
        
        let mut peak_equity: f64 = capital;
        let mut max_drawdown: f64 = 0.0;
        
        for event in events {
            // Update book state based on event type
            match event.event_type {
                EventType::New => {
                    book.add_order(event.side, event.price, event.quantity, &event.order_id);
                }
                EventType::Modify => {
                    book.modify_order(&event.order_id, event.quantity);
                }
                EventType::Cancel => {
                    book.cancel_order(&event.order_id);
                }
                EventType::Trade => {
                    // Execute trade in book
                    let consumed = book.execute_trade(event.side, event.price, event.quantity);
                    
                    // Check if our orders got filled
                    // A buy aggressor hits asks; a sell aggressor hits bids
                    let check_our_order = match event.side {
                        Side::Buy => &mut our_ask,  // Buy aggressor may fill our ask
                        Side::Sell => &mut our_bid,  // Sell aggressor may fill our bid
                    };
                    
                    if let Some(ref mut our_order) = check_our_order {
                        if (our_order.price - event.price).abs() < 1e-10 {
                            // Trade at our price level - check queue position
                            our_order.quantity_ahead -= event.quantity;
                            
                            if our_order.quantity_ahead <= 0.0 {
                                // We got filled!
                                let fill_qty = our_order.quantity;
                                let fill_price = our_order.price;
                                let fee = fill_qty * fill_price * self.fee_rate;
                                
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
                                
                                total_fees += fee;
                                total_volume += fill_qty * fill_price;
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
                }
            }
            
            // Periodically update our quotes
            if event.timestamp_ms - last_quote_time >= QUOTE_REFRESH_MS {
                last_quote_time = event.timestamp_ms;
                
                // Get current market state
                let mid_price = match book.mid_price() {
                    Some(p) => p,
                    None => continue,
                };
                
                // Apply volatility cap
                let effective_vol = volatility.min(params.volatility_cap);
                if effective_vol < 0.15 {
                    // Skip quoting in low volatility environments
                    continue;
                }
                
                // Avellaneda-Stoikov spread calculation
                let gamma = params.risk_aversion;
                let sigma = effective_vol;
                let t_horizon = params.time_horizon_minutes / 1440.0;  // Convert to days
                let q = inventory;
                
                // Reservation price: mid - gamma * sigma^2 * T * q
                let reservation_price = mid_price - gamma * sigma.powi(2) * t_horizon * q;
                
                // Optimal spread: gamma * sigma^2 * T + 2/gamma * ln(1 + gamma/k)
                let k = 1.5;  // Market maker intensity
                let optimal_spread = gamma * sigma.powi(2) * t_horizon + (2.0 / gamma) * (1.0 + gamma / k).ln();
                
                // Apply minimum spread
                let min_spread = mid_price * params.min_spread_bps / 10000.0;
                let half_spread = (optimal_spread / 2.0).max(min_spread / 2.0);
                
                // Calculate quote prices
                let bid_price = reservation_price - half_spread;
                let ask_price = reservation_price + half_spread;
                let order_size = capital * params.order_size_pct;
                
                // Cancel old orders and place new ones
                if our_bid.is_some() {
                    let old_bid = our_bid.take().unwrap();
                    book.cancel_order(&old_bid.order_id);
                }
                if our_ask.is_some() {
                    let old_ask = our_ask.take().unwrap();
                    book.cancel_order(&old_ask.order_id);
                }
                
                // Place new bid
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
                
                // Place new ask
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
                orders_placed += 1;
                
                // Track equity
                let mark_price = mid_price;
                let equity = capital + inventory * mark_price;
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
                total_wins / total_losses
            } else if total_wins > 0.0 {
                f64::MAX
            } else {
                0.0
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

/// Convert a slice of SimulationTicks to OrderBookEvents
/// 
/// This creates trade events only - suitable for degraded-mode realistic
/// simulation where we don't have full order book data.
pub fn ticks_to_trade_events<T: dataloader::Tick>(ticks: &[T]) -> Vec<OrderBookEvent> {
    ticks.iter().enumerate().map(|(idx, tick)| {
        OrderBookEvent::from_trade(
            tick.timestamp_ms(),
            tick.price(),
            tick.quantity(),
            tick.is_sell(),
            idx,
        )
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
