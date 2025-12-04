use crate::order_utils;
// Utility for converting Signal to OrderBookEvent moved to order_utils.rs

/// Core order book implementation
///
/// This module provides the main OrderBook structure that manages bid and ask sides
/// and handles all order book operations.

use crate::level::Level;
use crate::types::{
    BookSide, OrderBookEvent, OrderInfo, SnapshotLevel, LiquidityType, Fill,
    VolumeEntry, SlippageConfig, ExecutionLevel, MarketOrderExecution,
};
use crate::{DepthLevel, MarketDepth, OrderBookConfig};
use config::ExchangeFeeConfig;
use crate::logging_facade::ORDERBOOK_LOGGER;
use crate::{log_debug, log_warn};
use anyhow::Result;
use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};
use chrono::{DateTime, Utc, Duration};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Main order book structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    /// Symbol being traded
    pub symbol: String,
    /// Exchange name
    pub exchange: String,
    /// Bid side (buy orders) - ordered by price descending (highest first)
    pub bids: BTreeMap<OrderedFloat<f64>, Level>,
    /// Ask side (sell orders) - ordered by price ascending (lowest first)
    pub asks: BTreeMap<OrderedFloat<f64>, Level>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
    /// Configuration
    config: OrderBookConfig,
    /// Event sequence number
    pub sequence_number: u64,
    /// Fee configuration for this exchange
    pub fee_config: Option<ExchangeFeeConfig>,
    /// Rolling 30-day volume tracking
    pub volume_history: VecDeque<VolumeEntry>,
    /// Current 30-day volume in USD
    pub volume_30d: f64,
    /// Slippage configuration
    pub slippage_config: SlippageConfig,
}

impl OrderBook {

    /// Submit an order from a Signal (convenience for simulation)
    pub fn submit_order_from_signal(&mut self, signal: &signal::Signal, timestamp: DateTime<Utc>) -> anyhow::Result<()> {
        // Use the utility function from order_utils
        let event = order_utils::signal_to_order_event(signal, timestamp);
        self.apply_event(event)
    }

    /// Match all possible orders (comprehensive matching engine)
    /// Returns fills with proper maker/taker fee calculations
    pub fn match_all_orders(&mut self, timestamp: DateTime<Utc>) -> Vec<Fill> {
        let mut fills = Vec::new();
        
        while self.is_crossed() {
            // Get the best bid and ask price keys
            let best_bid_key = self.bids.keys().next_back().copied();
            let best_ask_key = self.asks.keys().next().copied();
            
            if let (Some(bid_key), Some(ask_key)) = (best_bid_key, best_ask_key) {
                // Get the matching price (use the ask price as the execution price)
                let execution_price = self.asks.get(&ask_key).unwrap().price.clone();
                
                // Calculate the maximum quantity that can be matched
                let bid_quantity = self.bids.get(&bid_key).unwrap().total_quantity.clone();
                let ask_quantity = self.asks.get(&ask_key).unwrap().total_quantity.clone();
                let match_quantity = bid_quantity.min(ask_quantity);
                
                if match_quantity <= BigDecimal::from(0) {
                    break;
                }
                
                // Execute trade on ask side (they are MAKERS - their limit orders were resting)
                let ask_executed_orders = {
                    let ask_level = self.asks.get_mut(&ask_key).unwrap();
                    ask_level.execute_trade(&match_quantity, timestamp).unwrap_or_default()
                };
                
                // Execute trade on bid side (they are MAKERS - their limit orders were resting)  
                let bid_executed_orders = {
                    let bid_level = self.bids.get_mut(&bid_key).unwrap();
                    bid_level.execute_trade(&match_quantity, timestamp).unwrap_or_default()
                };
                
                // Create fill records for bid orders (MAKERS - they provided liquidity)
                for bid_order in bid_executed_orders {
                    let fill_quantity = &bid_order.original_quantity - &bid_order.remaining_quantity;
                    let mut order_with_price = bid_order;
                    order_with_price.price = execution_price.clone();
                    
                    let fill = self.create_fill(&order_with_price, &fill_quantity, LiquidityType::Maker, timestamp);
                    
                    // Track volume for fee tier calculation
                    let _volume_usd = (&fill.price * &fill.quantity).to_f64().unwrap_or(0.0);
                    fills.push(fill);
                }
                
                // Create fill records for ask orders (MAKERS - they provided liquidity)
                for ask_order in ask_executed_orders {
                    let fill_quantity = &ask_order.original_quantity - &ask_order.remaining_quantity;
                    let mut order_with_price = ask_order;
                    order_with_price.price = execution_price.clone();
                    
                    let fill = self.create_fill(&order_with_price, &fill_quantity, LiquidityType::Maker, timestamp);
                    
                    // Track volume for fee tier calculation
                    let _volume_usd = (&fill.price * &fill.quantity).to_f64().unwrap_or(0.0);
                    fills.push(fill);
                }
                
                // Remove empty levels
                if self.bids.get(&bid_key).map_or(true, |level| level.is_empty()) {
                    self.bids.remove(&bid_key);
                }
                if self.asks.get(&ask_key).map_or(true, |level| level.is_empty()) {
                    self.asks.remove(&ask_key);
                }
                
                self.last_updated = timestamp;
            } else {
                break;
            }
        }
        
        // Track total volume for fee tier calculation
        let total_volume_usd: f64 = fills
            .iter()
            .map(|fill| (&fill.price * &fill.quantity).to_f64().unwrap_or(0.0))
            .sum();
            
        if total_volume_usd > 0.0 {
            self.add_volume(total_volume_usd, timestamp);
        }
        
        fills
    }
    /// Create a new empty order book
    pub fn new(symbol: String, exchange: String, config: OrderBookConfig) -> Self {
        Self {
            symbol,
            exchange,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_updated: Utc::now(),
            config,
            sequence_number: 0,
            fee_config: None,
            volume_history: VecDeque::new(),
            volume_30d: 0.0,
            slippage_config: SlippageConfig::default(),
        }
    }

    /// Create a new order book with fee configuration
    pub fn new_with_fees(
        symbol: String, 
        exchange: String, 
        config: OrderBookConfig,
        fee_config: ExchangeFeeConfig
    ) -> Self {
        Self {
            symbol,
            exchange,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_updated: Utc::now(),
            config,
            sequence_number: 0,
            fee_config: Some(fee_config),
            volume_history: VecDeque::new(),
            volume_30d: 0.0,
            slippage_config: SlippageConfig::default(),
        }
    }

    /// Create a new order book with full configuration
    pub fn new_with_config(
        symbol: String, 
        exchange: String, 
        config: OrderBookConfig,
        fee_config: Option<ExchangeFeeConfig>,
        slippage_config: SlippageConfig
    ) -> Self {
        Self {
            symbol,
            exchange,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_updated: Utc::now(),
            config,
            sequence_number: 0,
            fee_config,
            volume_history: VecDeque::new(),
            volume_30d: 0.0,
            slippage_config,
        }
    }

    /// Set fee configuration
    pub fn set_fee_config(&mut self, fee_config: ExchangeFeeConfig) {
        self.fee_config = Some(fee_config);
    }

    /// Set slippage configuration
    pub fn set_slippage_config(&mut self, slippage_config: SlippageConfig) {
        self.slippage_config = slippage_config;
    }

    /// Simulate a market order execution without consuming the order book
    /// This is useful for backtesting where we want to see the slippage impact without modifying the book
    pub fn simulate_market_order(
        &self, 
        side: BookSide, 
        quantity: BigDecimal
    ) -> MarketOrderExecution {
        let mut execution_levels = Vec::new();
        let mut remaining_quantity = quantity.clone();
        let mut total_cost = BigDecimal::from(0);
        let mut cumulative_quantity = BigDecimal::from(0);
        
        // Determine which side of the book to analyze
        let levels = match side {
            BookSide::Bid => &self.asks, // Buying would consume asks
            BookSide::Ask => &self.bids, // Selling would consume bids
        };
        
        // Get sorted keys
        let level_keys: Vec<_> = match side {
            BookSide::Bid => levels.keys().cloned().collect(), // Best ask first (lowest price)
            BookSide::Ask => levels.keys().rev().cloned().collect(), // Best bid first (highest price)
        };
        
        for level_key in level_keys {
            if remaining_quantity <= BigDecimal::from(0) {
                break;
            }
            
            if let Some(level) = levels.get(&level_key) {
                let available_quantity = level.total_quantity.clone();
                let execute_quantity = remaining_quantity.clone().min(available_quantity);
                let level_cost = &level.price * &execute_quantity;
                
                cumulative_quantity += &execute_quantity;
                execution_levels.push(ExecutionLevel {
                    price: level.price.clone(),
                    quantity: execute_quantity.clone(),
                    cumulative_quantity: cumulative_quantity.clone(),
                });
                
                total_cost += level_cost;
                remaining_quantity -= &execute_quantity;
            }
        }
        
        let executed_quantity = &quantity - &remaining_quantity;
        
        // Calculate VWAP
        let vwap = if executed_quantity > BigDecimal::from(0) {
            &total_cost / &executed_quantity
        } else {
            BigDecimal::from(0)
        };
        
        // Calculate slippage
        let slippage_bps = if let Some(first_level) = execution_levels.first() {
            let first_price = &first_level.price;
            let price_diff = match side {
                BookSide::Bid => (&vwap - first_price) / first_price,
                BookSide::Ask => (first_price - &vwap) / first_price,
            };
            price_diff.to_f64().unwrap_or(0.0) * 10000.0
        } else {
            0.0
        };
        
        MarketOrderExecution {
            requested_quantity: quantity,
            executed_quantity,
            vwap,
            execution_levels,
            total_cost,
            slippage_bps,
        }
    }

    /// Execute a market order against the order book, consuming actual liquidity
    /// This provides realistic slippage by walking through actual order book levels
    pub fn execute_market_order(
        &mut self, 
        side: BookSide, 
        quantity: BigDecimal, 
        timestamp: DateTime<Utc>
    ) -> Result<MarketOrderExecution> {
        let mut execution_levels = Vec::new();
        let mut remaining_quantity = quantity.clone();
        let mut total_cost = BigDecimal::from(0);
        let mut cumulative_quantity = BigDecimal::from(0);
        
        // Determine which side of the book to consume
        let levels = match side {
            BookSide::Bid => &mut self.asks, // Buying consumes asks
            BookSide::Ask => &mut self.bids, // Selling consumes bids
        };
        
        // Store keys to iterate over (to avoid borrowing issues)
        let level_keys: Vec<_> = match side {
            BookSide::Bid => levels.keys().cloned().collect(), // Best ask first (lowest price)
            BookSide::Ask => levels.keys().rev().cloned().collect(), // Best bid first (highest price)
        };
        
        for level_key in level_keys {
            if remaining_quantity <= BigDecimal::from(0) {
                break;
            }
            
            let level_price = {
                if let Some(level) = levels.get(&level_key) {
                    level.price.clone()
                } else {
                    continue;
                }
            };
            
            let available_quantity = {
                if let Some(level) = levels.get(&level_key) {
                    level.total_quantity.clone()
                } else {
                    continue;
                }
            };
            
            // Calculate how much we can execute at this level
            let execute_quantity = remaining_quantity.clone().min(available_quantity.clone());
            let level_cost = &level_price * &execute_quantity;
            
            // Execute the trade at this level
            if let Some(level) = levels.get_mut(&level_key) {
                let _executed_orders = level.execute_trade(&execute_quantity, timestamp)?;
                
                // Record this execution level
                cumulative_quantity += &execute_quantity;
                execution_levels.push(ExecutionLevel {
                    price: level_price.clone(),
                    quantity: execute_quantity.clone(),
                    cumulative_quantity: cumulative_quantity.clone(),
                });
                
                total_cost += level_cost;
                remaining_quantity -= &execute_quantity;
                
                // Remove empty levels
                if level.is_empty() {
                    levels.remove(&level_key);
                }
            }
        }
        
        let executed_quantity = &quantity - &remaining_quantity;
        
        // Calculate VWAP
        let vwap = if executed_quantity > BigDecimal::from(0) {
            &total_cost / &executed_quantity
        } else {
            BigDecimal::from(0)
        };
        
        // Calculate slippage (if we have execution levels)
        let slippage_bps = if let Some(first_level) = execution_levels.first() {
            let first_price = &first_level.price;
            let price_diff = match side {
                BookSide::Bid => (&vwap - first_price) / first_price, // Buying: higher VWAP = positive slippage
                BookSide::Ask => (first_price - &vwap) / first_price, // Selling: lower VWAP = positive slippage
            };
            price_diff.to_f64().unwrap_or(0.0) * 10000.0 // Convert to basis points
        } else {
            0.0
        };
        
        Ok(MarketOrderExecution {
            requested_quantity: quantity,
            executed_quantity,
            vwap,
            execution_levels,
            total_cost,
            slippage_bps,
        })
    }

    /// Add volume to tracking history and update 30-day volume
    fn add_volume(&mut self, volume_usd: f64, timestamp: DateTime<Utc>) {
        // Add new volume entry
        self.volume_history.push_back(VolumeEntry {
            timestamp,
            volume_usd,
        });

        // Remove entries older than 30 days
        let cutoff = timestamp - Duration::days(30);
        while let Some(entry) = self.volume_history.front() {
            if entry.timestamp >= cutoff {
                break;
            }
            self.volume_history.pop_front();
        }

        // Recalculate 30-day volume
        self.volume_30d = self.volume_history
            .iter()
            .map(|entry| entry.volume_usd)
            .sum();
    }

    /// Calculate slippage based on order size and market depth
    fn calculate_slippage(&self, side: BookSide, order_size_usd: f64) -> f64 {
        let levels = match side {
            BookSide::Bid => &self.asks, // Buying hits asks
            BookSide::Ask => &self.bids, // Selling hits bids
        };

        // Calculate available liquidity in the top N levels
        let mut total_liquidity = 0.0;
        let mut level_count = 0;

        for (price, level) in levels.iter() {
            if level_count >= self.slippage_config.depth_levels {
                break;
            }
            let price_f64 = price.into_inner();
            let quantity = level.total_quantity.to_f64().unwrap_or(0.0);
            total_liquidity += price_f64 * quantity;
            level_count += 1;
        }

        // Base slippage
        let mut slippage_bps = self.slippage_config.base_slippage_bps;

        // Add impact based on order size relative to available liquidity
        if total_liquidity > 0.0 {
            let impact_ratio = order_size_usd / total_liquidity;
            let impact_slippage = impact_ratio * self.slippage_config.impact_coefficient * 10000.0; // Convert to bps
            slippage_bps += impact_slippage;
        } else {
            // No liquidity - maximum slippage
            slippage_bps = self.slippage_config.max_slippage_bps;
        }

        // Apply bounds
        slippage_bps = slippage_bps.max(self.slippage_config.min_slippage_bps);
        slippage_bps = slippage_bps.min(self.slippage_config.max_slippage_bps);

        slippage_bps / 10000.0 // Convert back to decimal
    }

    /// Calculate execution price including slippage
    fn apply_slippage(&self, base_price: &BigDecimal, side: BookSide, slippage_rate: f64) -> BigDecimal {
        let price_f64 = base_price.to_f64().unwrap_or(0.0);
        let slipped_price = match side {
            BookSide::Bid => price_f64 * (1.0 + slippage_rate), // Buying - price goes up
            BookSide::Ask => price_f64 * (1.0 - slippage_rate), // Selling - price goes down
        };
        BigDecimal::from_f64(slipped_price).unwrap_or_else(|| base_price.clone())
    }

    /// Create a fill from market order execution result
    #[allow(dead_code)]
    fn create_fill_from_execution(
        &self, 
        order_id: String, 
        side: BookSide,
        execution: &MarketOrderExecution, 
        liquidity_type: LiquidityType, 
        timestamp: DateTime<Utc>
    ) -> Fill {
        // Calculate fees on the VWAP execution
        let final_trade_value = &execution.vwap * &execution.executed_quantity;
        
        let (fee_rate, fee_amount) = if let Some(ref fee_config) = self.fee_config {
            let is_maker = liquidity_type == LiquidityType::Maker;
            let rate = fee_config.get_fee_rate(self.volume_30d, is_maker);
            let fee = if rate >= 0.0 {
                &final_trade_value * BigDecimal::from_f64(rate).unwrap_or_default()
            } else {
                // Negative fee (rebate)
                &final_trade_value * BigDecimal::from_f64(-rate).unwrap_or_default() * BigDecimal::from(-1)
            };
            (rate, fee)
        } else {
            (0.0, BigDecimal::from(0))
        };

        // For realistic execution, the base price would be the first level price
        let base_price = execution.execution_levels
            .first()
            .map(|level| level.price.clone())
            .unwrap_or_else(|| execution.vwap.clone());

        Fill {
            order_id,
            side,
            price: execution.vwap.clone(),  // Use VWAP as execution price
            base_price: Some(base_price),   // First level price as base
            quantity: execution.executed_quantity.clone(),
            liquidity_type,
            fee_rate,
            fee_amount,
            slippage_bps: if execution.slippage_bps > 0.0 { Some(execution.slippage_bps) } else { None },
            timestamp,
        }
    }

    /// Create a fill with fee calculation and slippage
    fn create_fill(&self, order: &OrderInfo, quantity: &BigDecimal, liquidity_type: LiquidityType, timestamp: DateTime<Utc>) -> Fill {
        let base_price = &order.price;
        
        // Calculate slippage
        let trade_value_usd = (base_price * quantity).to_f64().unwrap_or(0.0);
        let slippage_rate = self.calculate_slippage(order.side, trade_value_usd);
        let execution_price = self.apply_slippage(base_price, order.side, slippage_rate);
        
        // Calculate final trade value with slippage
        let final_trade_value = &execution_price * quantity;
        
        // Calculate fees
        let (fee_rate, fee_amount) = if let Some(ref fee_config) = self.fee_config {
            let is_maker = liquidity_type == LiquidityType::Maker;
            let rate = fee_config.get_fee_rate(self.volume_30d, is_maker);
            let fee = if rate >= 0.0 {
                &final_trade_value * BigDecimal::from_f64(rate).unwrap_or_default()
            } else {
                // Negative fee (rebate)
                &final_trade_value * BigDecimal::from_f64(-rate).unwrap_or_default() * BigDecimal::from(-1)
            };
            (rate, fee)
        } else {
            (0.0, BigDecimal::from(0))
        };

        Fill {
            order_id: order.order_id.clone(),
            side: order.side,
            price: execution_price.clone(),  // Use slippage-adjusted price
            base_price: Some(base_price.clone()),  // Original price before slippage
            quantity: quantity.clone(),
            liquidity_type,
            fee_rate,
            fee_amount,
            slippage_bps: if slippage_rate > 0.0 { Some(slippage_rate * 10000.0) } else { None }, // Convert to basis points
            timestamp,
        }
    }

    /// Apply an order book event
    pub fn apply_event(&mut self, event: OrderBookEvent) -> Result<()> {
        match event {
            OrderBookEvent::NewOrder {
                order_id,
                side,
                price,
                quantity,
                timestamp,
            } => {
                self.add_order(order_id, side, price, quantity, timestamp)?;
            }
            OrderBookEvent::ModifyOrder {
                order_id,
                new_quantity,
                timestamp,
            } => {
                self.modify_order(&order_id, new_quantity, timestamp)?;
            }
            OrderBookEvent::CancelOrder { order_id, timestamp } => {
                self.cancel_order(&order_id, timestamp)?;
            }
            OrderBookEvent::Trade {
                side,
                price,
                quantity,
                timestamp,
                ..
            } => {
                self.execute_trade(side, price, quantity, timestamp)?;
            }
            OrderBookEvent::Snapshot {
                bids,
                asks,
                timestamp,
            } => {
                self.apply_snapshot(bids, asks, timestamp)?;
            }
        }
        
        self.sequence_number += 1;
        self.cleanup_empty_levels();
        Ok(())
    }

    /// Add a new order to the book
    pub fn add_order(
        &mut self,
        order_id: String,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        let price_key = OrderedFloat(price.to_string().parse::<f64>().map_err(|e| {
            anyhow::anyhow!("Failed to convert price to f64: {}", e)
        })?);

        let order = OrderInfo {
            order_id: order_id.clone(),
            side,
            original_quantity: quantity.clone(),
            remaining_quantity: quantity.clone(),
            price: price.clone(),
            timestamp,
            order_type: crate::types::OrderType::Limit,
            liquidity_type: None, // Will be set when the order is filled
        };

        let levels = match side {
            BookSide::Bid => &mut self.bids,
            BookSide::Ask => &mut self.asks,
        };

        // Get or create level
        let level = levels
            .entry(price_key)
            .or_insert_with(|| Level::new(price.clone(), side, self.config.track_individual_orders));

        level.add_order(order)?;
        self.last_updated = timestamp;

        log_debug!(ORDERBOOK_LOGGER, 
            "Added order {} to {} side at price {} with quantity {}",
            order_id,
            match side {
                BookSide::Bid => "bid",
                BookSide::Ask => "ask",
            },
            price,
            quantity
        );

        Ok(())
    }

    /// Modify an existing order
    pub fn modify_order(
        &mut self,
        order_id: &str,
        new_quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Search both sides for the order
        for (_, level) in self.bids.iter_mut().chain(self.asks.iter_mut()) {
            if level.orders.as_ref().map_or(false, |orders| orders.contains_key(order_id)) {
                level.modify_order(order_id, new_quantity, timestamp)?;
                self.last_updated = timestamp;
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Order {} not found for modification", order_id))
    }

    /// Cancel an order
    pub fn cancel_order(&mut self, order_id: &str, timestamp: DateTime<Utc>) -> Result<()> {
        // Search both sides for the order
        for (_, level) in self.bids.iter_mut().chain(self.asks.iter_mut()) {
            if level.orders.as_ref().map_or(false, |orders| orders.contains_key(order_id)) {
                level.remove_order(order_id, timestamp)?;
                self.last_updated = timestamp;
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("Order {} not found for cancellation", order_id))
    }

    /// Execute a trade against the book
    pub fn execute_trade(
        &mut self,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Result<Vec<OrderInfo>> {
        let price_key = OrderedFloat(price.to_string().parse::<f64>().map_err(|e| {
            anyhow::anyhow!("Failed to convert price to f64: {}", e)
        })?);

        let levels = match side {
            BookSide::Bid => &mut self.bids,
            BookSide::Ask => &mut self.asks,
        };

        if let Some(level) = levels.get_mut(&price_key) {
            let executed_orders = level.execute_trade(&quantity, timestamp)?;
            self.last_updated = timestamp;
            Ok(executed_orders)
        } else {
            log_warn!(ORDERBOOK_LOGGER, "Trade execution attempted at non-existent price level: {}", price);
            Ok(Vec::new())
        }
    }

    /// Execute a trade against the book and return fills
    /// This is used when processing incoming market trades that may fill our resting orders
    pub fn execute_trade_with_fills(
        &mut self,
        side: BookSide,
        price: BigDecimal,
        quantity: BigDecimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<Fill> {
        let mut fills = Vec::new();
        
        let price_f64 = match price.to_string().parse::<f64>() {
            Ok(p) => p,
            Err(e) => {
                log_warn!(ORDERBOOK_LOGGER, "Failed to convert price to f64: {}", e);
                return fills;
            }
        };
        let price_key = OrderedFloat(price_f64);

        // Collect executed orders first to avoid borrow issues
        let executed_orders: Vec<OrderInfo> = {
            let levels = match side {
                BookSide::Bid => &mut self.bids,
                BookSide::Ask => &mut self.asks,
            };

            if let Some(level) = levels.get_mut(&price_key) {
                let orders = level.execute_trade(&quantity, timestamp).unwrap_or_default();
                self.last_updated = timestamp;
                
                // Remove level if empty
                if level.is_empty() {
                    levels.remove(&price_key);
                }
                orders
            } else {
                Vec::new()
            }
        };

        // Now create fills from executed orders
        for order in executed_orders {
            let fill_quantity = &order.original_quantity - &order.remaining_quantity;
            if fill_quantity > BigDecimal::from(0) {
                let fill = self.create_fill(&order, &fill_quantity, LiquidityType::Maker, timestamp);
                fills.push(fill);
            }
        }

        // Track volume for fee tier calculation
        let total_volume_usd: f64 = fills
            .iter()
            .map(|fill| (&fill.price * &fill.quantity).to_f64().unwrap_or(0.0))
            .sum();
            
        if total_volume_usd > 0.0 {
            self.add_volume(total_volume_usd, timestamp);
        }

        fills
    }

    /// Apply a complete snapshot to rebuild the book
    pub fn apply_snapshot(
        &mut self,
        bids: Vec<SnapshotLevel>,
        asks: Vec<SnapshotLevel>,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        // Clear existing levels
        self.bids.clear();
        self.asks.clear();

        // Rebuild from snapshot
        for bid in bids {
            let price_key = OrderedFloat(bid.price.to_string().parse::<f64>().map_err(|e| {
                anyhow::anyhow!("Failed to convert bid price to f64: {}", e)
            })?);
            
            let mut level = Level::new(bid.price.clone(), BookSide::Bid, false);
            level.total_quantity = bid.quantity;
            level.order_count = bid.order_count;
            level.last_updated = timestamp;
            
            self.bids.insert(price_key, level);
        }

        for ask in asks {
            let price_key = OrderedFloat(ask.price.to_string().parse::<f64>().map_err(|e| {
                anyhow::anyhow!("Failed to convert ask price to f64: {}", e)
            })?);
            
            let mut level = Level::new(ask.price.clone(), BookSide::Ask, false);
            level.total_quantity = ask.quantity;
            level.order_count = ask.order_count;
            level.last_updated = timestamp;
            
            self.asks.insert(price_key, level);
        }

        self.last_updated = timestamp;
        Ok(())
    }

    /// Get market depth information
    pub fn get_market_depth(&self, max_levels: Option<usize>) -> MarketDepth {
        let limit = max_levels.unwrap_or(self.config.max_levels.unwrap_or(100));

        let bids: Vec<DepthLevel> = self
            .bids
            .iter()
            .rev() // Highest bids first
            .take(limit)
            .map(|(price, level)| DepthLevel {
                price: price.0,
                quantity: level.total_quantity.to_string().parse().unwrap_or(0.0),
                order_count: level.order_count,
            })
            .collect();

        let asks: Vec<DepthLevel> = self
            .asks
            .iter() // Lowest asks first
            .take(limit)
            .map(|(price, level)| DepthLevel {
                price: price.0,
                quantity: level.total_quantity.to_string().parse().unwrap_or(0.0),
                order_count: level.order_count,
            })
            .collect();

        let best_bid = bids.first().cloned();
        let best_ask = asks.first().cloned();
        let spread = match (&best_bid, &best_ask) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        };

        MarketDepth {
            timestamp: self.last_updated,
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
            bids,
            asks,
            best_bid,
            best_ask,
            spread,
        }
    }

    /// Get the best bid price
    pub fn best_bid(&self) -> Option<&BigDecimal> {
        self.bids.values().next_back().map(|level| &level.price)
    }

    /// Get the best ask price
    pub fn best_ask(&self) -> Option<&BigDecimal> {
        self.asks.values().next().map(|level| &level.price)
    }

    /// Get the current spread
    pub fn spread(&self) -> Option<BigDecimal> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Remove empty levels from the book
    fn cleanup_empty_levels(&mut self) {
        self.bids.retain(|_, level| !level.is_empty());
        self.asks.retain(|_, level| !level.is_empty());
    }

    /// Check if the order book is crossed (best bid >= best ask)
    pub fn is_crossed(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid >= ask,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BookSide;

    #[test]
    fn test_order_book_creation() {
        let config = OrderBookConfig::default();
        let book = OrderBook::new("BTCUSD".to_string(), "TEST".to_string(), config);
        
        assert_eq!(book.symbol, "BTCUSD");
        assert_eq!(book.exchange, "TEST");
        assert!(book.bids.is_empty());
        assert!(book.asks.is_empty());
    }

    #[tokio::test]
    async fn test_add_orders() {
        let config = OrderBookConfig::default();
        let mut book = OrderBook::new("BTCUSD".to_string(), "TEST".to_string(), config);
        
        // Add bid
        book.add_order(
            "bid1".to_string(),
            BookSide::Bid,
            BigDecimal::from(100),
            BigDecimal::from(10),
            Utc::now(),
        ).unwrap();
        
        // Add ask
        book.add_order(
            "ask1".to_string(),
            BookSide::Ask,
            BigDecimal::from(105),
            BigDecimal::from(15),
            Utc::now(),
        ).unwrap();
        
        assert_eq!(book.bids.len(), 1);
        assert_eq!(book.asks.len(), 1);
        assert_eq!(book.best_bid(), Some(&BigDecimal::from(100)));
        assert_eq!(book.best_ask(), Some(&BigDecimal::from(105)));
    }
    
    #[tokio::test]
    async fn test_realistic_slippage_modeling() {
        let mut order_book = OrderBook::new(
            "BTC-USD".to_string(),
            "test_exchange".to_string(),
            config::OrderBookConfig::default(),
        );
        
        let now = Utc::now();
        
        // Add some ask levels to the book (people selling)
        order_book.add_order("ask1".to_string(), BookSide::Ask, BigDecimal::from_f64(50000.0).unwrap(), BigDecimal::from_f64(1.0).unwrap(), now).unwrap();
        order_book.add_order("ask2".to_string(), BookSide::Ask, BigDecimal::from_f64(50010.0).unwrap(), BigDecimal::from_f64(2.0).unwrap(), now).unwrap();
        order_book.add_order("ask3".to_string(), BookSide::Ask, BigDecimal::from_f64(50020.0).unwrap(), BigDecimal::from_f64(3.0).unwrap(), now).unwrap();
        
        // Add some bid levels (people buying)
        order_book.add_order("bid1".to_string(), BookSide::Bid, BigDecimal::from_f64(49990.0).unwrap(), BigDecimal::from_f64(1.5).unwrap(), now).unwrap();
        order_book.add_order("bid2".to_string(), BookSide::Bid, BigDecimal::from_f64(49980.0).unwrap(), BigDecimal::from_f64(2.5).unwrap(), now).unwrap();
        
        // Simulate a large market buy order (should consume asks)
        let market_buy_result = order_book.simulate_market_order(
            BookSide::Bid,  // Buying
            BigDecimal::from_f64(4.0).unwrap()  // Buy 4 BTC
        );
        
        println!("Market Buy Order Results:");
        println!("Requested: {} BTC", market_buy_result.requested_quantity);
        println!("Executed: {} BTC", market_buy_result.executed_quantity);
        println!("VWAP: ${}", market_buy_result.vwap);
        println!("Total Cost: ${}", market_buy_result.total_cost);
        println!("Slippage: {} bps", market_buy_result.slippage_bps);
        
        for (i, level) in market_buy_result.execution_levels.iter().enumerate() {
            println!("Level {}: {} BTC @ ${} (cumulative: {})", 
                i + 1, level.quantity, level.price, level.cumulative_quantity);
        }
        
        // The order should have consumed:
        // - 1.0 BTC @ $50,000 = $50,000
        // - 2.0 BTC @ $50,010 = $100,020
        // - 1.0 BTC @ $50,020 = $50,020  (partial fill of level 3)
        // Total: 4.0 BTC for $200,040
        // VWAP: $50,010 (vs first level $50,000 = 0.02% = 2 bps slippage)
        
        assert_eq!(market_buy_result.executed_quantity, BigDecimal::from_f64(4.0).unwrap());
        assert!(market_buy_result.slippage_bps > 0.0); // Should have positive slippage
        assert_eq!(market_buy_result.execution_levels.len(), 3); // Should have hit 3 levels
        
        // Now test a market sell order
        let market_sell_result = order_book.simulate_market_order(
            BookSide::Ask,  // Selling
            BigDecimal::from_f64(3.0).unwrap()  // Sell 3 BTC
        );
        
        println!("\nMarket Sell Order Results:");
        println!("Requested: {} BTC", market_sell_result.requested_quantity);
        println!("Executed: {} BTC", market_sell_result.executed_quantity);  
        println!("VWAP: ${}", market_sell_result.vwap);
        println!("Total Proceeds: ${}", market_sell_result.total_cost);
        println!("Slippage: {} bps", market_sell_result.slippage_bps);
        
        // Should consume bids starting from highest price
        assert!(market_sell_result.slippage_bps > 0.0);
    }
}
