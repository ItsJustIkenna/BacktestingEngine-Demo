//! Avellaneda-Stoikov Market Making Strategy (Crypto Adaptation)
//!
//! Implements the optimal market making model from Avellaneda & Stoikov (2008),
//! adapted for high-frequency crypto markets with millisecond data granularity.
use num_traits::{ToPrimitive, FromPrimitive};

use crate::traits::*;
use crate::signal::*;
// use crate::indicators::*;
// use portfoliomanager::PortfolioState;
use riskmanager::RiskLimits;
use config::ParameterValue;
// use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc, Duration};
use bigdecimal::BigDecimal;
use std::collections::HashMap;

/// Parameters for the Avellaneda-Stoikov strategy
#[derive(Debug, Clone)]
pub struct AvellanedaStoikovParams {
    pub risk_aversion: f64,         // γ: BASE risk aversion parameter (will be adjusted dynamically)
    pub order_size: f64,            // Fixed order size (in base asset) - DEPRECATED, use order_size_pct
    pub order_size_pct: f64,        // Order size as percentage of capital (e.g., 0.02 = 2%)
    pub initial_capital: f64,       // Initial trading capital (used with order_size_pct)
    pub initial_inventory_pct: f64, // Percentage of capital to hold as base asset inventory (0.5 = 50%)
    pub window_ms: u64,             // Rolling window for volatility estimation (milliseconds)
    pub min_quote_lifetime_ms: u64, // Minimum quote lifetime (milliseconds)
    pub fee_rate: f64,              // Exchange taker/maker fee (fraction)
    pub latency_buffer: f64,        // Price buffer for latency/slippage
    pub inventory_target: f64,      // BASE target inventory (will be adjusted dynamically)
    pub volatility_cap: f64,        // Max allowed volatility (pause quoting above this)
    pub time_horizon_minutes: f64,  // t: time horizon for optimization (in minutes)
    pub market_depth_k: f64,        // k: market depth parameter (higher = tighter spread)
    pub min_spread_bps: f64,        // Minimum spread in basis points (to cover costs)
    pub max_trades_per_minute: f64, // Rate limit on trading frequency
    pub take_profit_pct: f64,       // Take profit percentage (close position when profit exceeds this)
    pub stop_loss_pct: f64,         // Stop loss percentage (close position when loss exceeds this)
    
    // Phase 2: Dynamic adjustment parameters (optimized by genetic algorithm)
    pub volatility_adjustment_factor: f64, // How much to scale γ with volatility (0.5 = 50% increase per 1.0 vol ratio)
    pub inventory_penalty_factor: f64,     // How much to increase γ with inventory (0.3 = 30% per unit deviation)
    pub trend_sensitivity: f64,            // How much to lean inventory target with trend (-0.5 to 0.5)
    pub flow_sensitivity: f64,             // How much to fade order flow imbalance (-0.2 to 0.2)
}

impl Default for AvellanedaStoikovParams {
    fn default() -> Self {
        Self {
            risk_aversion: 0.1,             // Increased from 0.01 - wider spreads to cover costs
            order_size: 0.001,              // DEPRECATED: Small order size fallback
            order_size_pct: 0.02,           // 2% of capital per trade (default)
            initial_capital: 100000.0,      // Default $100K capital
            initial_inventory_pct: 0.5,     // 50% of capital as base asset inventory (for market making)
            window_ms: 60_000, // 60 seconds
            min_quote_lifetime_ms: 500,     // 500ms quote lifetime for responsive market making
            fee_rate: 0.0003,               // 3bps maker fee (typical for high-volume)
            latency_buffer: 0.3,            // Small buffer for slippage
            inventory_target: 0.0,
            volatility_cap: 0.10, // 10% annualized
            time_horizon_minutes: 1.0,      // Short horizon for tighter spreads
            market_depth_k: 1.5,            // Higher k = tighter spreads, more fills
            min_spread_bps: 15.0,           // Minimum 15bps spread (covers fees + small buffer)
            max_trades_per_minute: 30.0,    // Max 30 trades per minute - allows active trading
            take_profit_pct: 0.005,         // 0.5% take profit (50bps)
            stop_loss_pct: 0.003,           // 0.3% stop loss (30bps) - tighter than TP for market making
            
            // Phase 2: Dynamic adjustment defaults
            volatility_adjustment_factor: 0.8,  // Increased from 0.5 - more responsive to vol
            inventory_penalty_factor: 0.5,      // Increased from 0.3 - stronger inventory control
            trend_sensitivity: 0.3,             // Increased from 0.2 - more trend following
            flow_sensitivity: -0.15,            // Increased from -0.1 - stronger fade
        }
    }
}

impl AvellanedaStoikovParams {
    /// Validate parameter bounds
    pub fn validate(&self) -> Result<(), String> {
        if self.risk_aversion <= 0.0 || self.risk_aversion > 10.0 {
            return Err(format!("risk_aversion must be in (0, 10], got {}", self.risk_aversion));
        }
        if self.order_size <= 0.0 {
            return Err(format!("order_size must be positive, got {}", self.order_size));
        }
        if self.window_ms < 1_000 {
            return Err(format!("window_ms must be >= 1000ms, got {}", self.window_ms));
        }
        if self.min_quote_lifetime_ms == 0 {
            return Err(format!("min_quote_lifetime_ms must be positive, got {}", self.min_quote_lifetime_ms));
        }
        if self.fee_rate < 0.0 || self.fee_rate > 0.1 {
            return Err(format!("fee_rate must be in [0, 0.1], got {}", self.fee_rate));
        }
        if self.latency_buffer < 0.0 {
            return Err(format!("latency_buffer must be non-negative, got {}", self.latency_buffer));
        }
        if self.volatility_cap <= 0.0 || self.volatility_cap > 10.0 {
            return Err(format!("volatility_cap must be in (0, 10], got {}", self.volatility_cap));
        }
        if self.time_horizon_minutes <= 0.0 || self.time_horizon_minutes > 60.0 {
            return Err(format!("time_horizon_minutes must be in (0, 60], got {}", self.time_horizon_minutes));
        }
        if self.market_depth_k <= 0.0 || self.market_depth_k > 10.0 {
            return Err(format!("market_depth_k must be in (0, 10], got {}", self.market_depth_k));
        }
        Ok(())
    }
}

/// Avellaneda-Stoikov Market Making Strategy
#[derive(Debug)]
pub struct AvellanedaStoikovStrategy {
    params: AvellanedaStoikovParams,
    last_bid: Option<BigDecimal>,
    last_ask: Option<BigDecimal>,
    last_quote_time: Option<DateTime<Utc>>,
    price_history: Vec<(DateTime<Utc>, BigDecimal)>,
    inventory: f64,
    paused: bool,
    // EWMA state for volatility estimation
    ewma_mean: Option<f64>,
    ewma_variance: Option<f64>,
    
    // Phase 2: Market state tracking for dynamic adjustments
    baseline_volatility: Option<f64>,       // Historical average volatility for comparison
    recent_fills: Vec<(DateTime<Utc>, f64)>, // Recent fill prices for trend estimation
    buy_volume: f64,                         // Recent buy volume
    sell_volume: f64,                        // Recent sell volume
    max_inventory: f64,                      // Maximum allowed inventory for penalty calculation
    
    // Trade rate limiting
    trades_this_minute: u32,                 // Count of trades in current minute
    current_minute_start: Option<DateTime<Utc>>, // Start of current minute window
    
    // Position exit tracking
    avg_entry_price: Option<f64>,            // Average entry price for current position
}

impl AvellanedaStoikovStrategy {
    /// Create a new Avellaneda-Stoikov strategy with given parameters
    pub fn new(params: AvellanedaStoikovParams) -> Self {
        // Validate parameters
        if let Err(e) = params.validate() {
            panic!("Invalid Avellaneda-Stoikov parameters: {}", e);
        }
        
        Self {
            params,
            last_bid: None,
            last_ask: None,
            last_quote_time: None,
            price_history: Vec::new(),
            inventory: 0.0,
            paused: false,
            ewma_mean: None,
            ewma_variance: None,
            
            // Phase 2: Initialize market state tracking
            baseline_volatility: None,
            recent_fills: Vec::new(),
            buy_volume: 0.0,
            sell_volume: 0.0,
            max_inventory: 10.0, // Default max inventory (will be calibrated)
            
            // Trade rate limiting
            trades_this_minute: 0,
            current_minute_start: None,
            
            // Position exit tracking
            avg_entry_price: None,
        }
    }
    
    /// Check if we can trade (rate limiting)
    fn can_trade(&mut self, now: DateTime<Utc>) -> bool {
        // Reset counter if we're in a new minute
        if let Some(minute_start) = self.current_minute_start {
            if now - minute_start >= Duration::seconds(60) {
                self.current_minute_start = Some(now);
                self.trades_this_minute = 0;
            }
        } else {
            self.current_minute_start = Some(now);
            self.trades_this_minute = 0;
        }
        
        // Check if under rate limit
        (self.trades_this_minute as f64) < self.params.max_trades_per_minute
    }
    
    /// Record a trade for rate limiting
    fn record_trade(&mut self) {
        self.trades_this_minute += 1;
    }
    
    /// Calculate order size in asset units based on percentage of capital
    /// This ensures each trade is a consistent percentage of available capital
    /// order_size = (capital * order_size_pct) / current_price
    fn calculate_order_size(&self, current_price: f64, available_balance: f64) -> f64 {
        if current_price <= 0.0 {
            return self.params.order_size; // Fallback to fixed size
        }
        
        // Use available balance from portfolio if > 0, otherwise use initial_capital
        let capital = if available_balance > 0.0 {
            available_balance
        } else {
            self.params.initial_capital
        };
        
        // Calculate order value as percentage of capital, then convert to asset units
        let order_value_usd = capital * self.params.order_size_pct;
        let order_size_units = order_value_usd / current_price;
        
        // Ensure order size is at least some minimum (avoid dust orders)
        let min_order_size = 0.0001;
        order_size_units.max(min_order_size)
    }

    /// Check if we should exit current position (take profit or stop loss)
    fn check_exit_conditions(&mut self, current_price: f64, now: DateTime<Utc>) -> Vec<Signal> {
        let mut signals = Vec::new();
        
        // Only check exits if we have a position and an entry price
        if self.inventory.abs() < 1e-8 || self.avg_entry_price.is_none() {
            return signals;
        }
        
        let entry_price = self.avg_entry_price.unwrap();
        let position_size = self.inventory.abs();
        
        // Calculate P&L percentage
        let pnl_pct = if self.inventory > 0.0 {
            // Long position: profit if price goes up
            (current_price - entry_price) / entry_price
        } else {
            // Short position: profit if price goes down
            (entry_price - current_price) / entry_price
        };
        
        // Check take profit
        if pnl_pct >= self.params.take_profit_pct {
            let signal_type = if self.inventory > 0.0 {
                SignalType::TakeProfit
            } else {
                SignalType::TakeProfit
            };
            
            signals.push(Signal::new(
                format!("as_tp_{}", now.timestamp_millis()),
                signal_type,
                SignalStrength::Strong,
                SignalReason::RiskManagement(format!("Take profit at {:.2}% gain", pnl_pct * 100.0)),
            ).with_price(BigDecimal::from_f64(current_price).unwrap())
             .with_quantity(BigDecimal::from_f64(position_size).unwrap()));
            
            // Reset entry price after exit
            self.avg_entry_price = None;
            return signals;
        }
        
        // Check stop loss
        if pnl_pct <= -self.params.stop_loss_pct {
            let signal_type = SignalType::StopLoss;
            
            signals.push(Signal::new(
                format!("as_sl_{}", now.timestamp_millis()),
                signal_type,
                SignalStrength::Strong,
                SignalReason::RiskManagement(format!("Stop loss at {:.2}% loss", pnl_pct.abs() * 100.0)),
            ).with_price(BigDecimal::from_f64(current_price).unwrap())
             .with_quantity(BigDecimal::from_f64(position_size).unwrap()));
            
            // Reset entry price after exit
            self.avg_entry_price = None;
            return signals;
        }
        
        signals
    }
    
    /// Phase 2: Calculate dynamic risk aversion (gamma) based on market conditions
    fn calculate_dynamic_gamma(&mut self, current_volatility: f64) -> f64 {
        let base_gamma = self.params.risk_aversion;
        
        // 1. Volatility adjustment: scale gamma with volatility ratio
        let vol_factor = if let Some(baseline) = self.baseline_volatility {
            if baseline > 0.0 {
                (current_volatility / baseline).min(3.0) // Cap at 3x baseline
            } else {
                1.0
            }
        } else {
            // Initialize baseline on first call
            self.baseline_volatility = Some(current_volatility);
            1.0
        };
        
        // 2. Inventory penalty: increase gamma when inventory deviates from target
        let inventory_deviation = (self.inventory - self.params.inventory_target).abs();
        let inventory_penalty = if self.max_inventory > 0.0 {
            (inventory_deviation / self.max_inventory).min(1.0).powi(2)
        } else {
            0.0
        };
        
        // 3. Combine adjustments
        let dynamic_gamma = base_gamma * (
            1.0 
            + vol_factor * self.params.volatility_adjustment_factor
            + inventory_penalty * self.params.inventory_penalty_factor
        );
        
        // Clamp to reasonable bounds
        dynamic_gamma.max(0.001).min(1.0)
    }
    
    /// Phase 2: Calculate dynamic inventory target based on market signals
    fn calculate_dynamic_target(&mut self) -> f64 {
        let base_target = self.params.inventory_target;
        
        // 1. Trend component: estimate from recent price action
        let trend = self.estimate_trend();
        let trend_bias = trend * self.params.trend_sensitivity;
        
        // 2. Order flow imbalance: fade the flow (contrarian)
        let total_volume = self.buy_volume + self.sell_volume;
        let flow_imbalance = if total_volume > 0.0 {
            (self.buy_volume - self.sell_volume) / total_volume
        } else {
            0.0
        };
        let flow_bias = flow_imbalance * self.params.flow_sensitivity;
        
        // 3. Combine adjustments
        let dynamic_target = base_target + trend_bias + flow_bias;
        
        // Clamp to reasonable inventory limits
        dynamic_target.clamp(-1.0, 1.0)
    }
    
    /// Estimate trend from recent price action (simple linear regression on log prices)
    fn estimate_trend(&self) -> f64 {
        if self.recent_fills.len() < 5 {
            return 0.0; // Not enough data
        }
        
        // Use last 20 fills or all available
        let window = self.recent_fills.len().min(20);
        let recent = &self.recent_fills[self.recent_fills.len() - window..];
        
        // Simple slope calculation
        let n = recent.len() as f64;
        let x_mean = (n - 1.0) / 2.0;
        let y_mean = recent.iter().map(|(_, p)| p.ln()).sum::<f64>() / n;
        
        let numerator: f64 = recent.iter().enumerate()
            .map(|(i, (_, p))| {
                let x = i as f64;
                let y = p.ln();
                (x - x_mean) * (y - y_mean)
            })
            .sum();
            
        let denominator: f64 = recent.iter().enumerate()
            .map(|(i, _)| {
                let x = i as f64;
                (x - x_mean).powi(2)
            })
            .sum();
        
        if denominator > 0.0 {
            let slope = numerator / denominator;
            // Normalize to approximately [-1, 1] range
            slope.tanh()
        } else {
            0.0
        }
    }
    
    /// Update volume tracking (called from update_state)
    fn update_volume_tracking(&mut self, market_data: &dataloader::MarketData) {
        // Decay old volume over time (exponential decay)
        let decay_factor = 0.99;
        self.buy_volume *= decay_factor;
        self.sell_volume *= decay_factor;
        
        // Add new volume based on market data type
        match market_data {
            dataloader::MarketData::Trade(trade) => {
                let volume = trade.quantity.to_f64().unwrap_or(0.0);
                if trade.side == "buy" {
                    self.buy_volume += volume;
                } else {
                    self.sell_volume += volume;
                }
                
                // Track fill price for trend estimation
                let price = trade.price.to_f64().unwrap_or(0.0);
                self.recent_fills.push((trade.timestamp, price));
                
                // Keep only recent fills (last 100)
                if self.recent_fills.len() > 100 {
                    self.recent_fills.remove(0);
                }
            },
            _ => {},
        }
    }
}

impl Default for AvellanedaStoikovStrategy {
    fn default() -> Self {
        Self::new(AvellanedaStoikovParams::default())
    }
}

#[async_trait]
impl Strategy for AvellanedaStoikovStrategy {
    fn name(&self) -> &str {
        "AvellanedaStoikovMarketMaker"
    }

    fn description(&self) -> &str {
        "Optimal market making strategy (Avellaneda-Stoikov, crypto adaptation)"
    }

    async fn initialize(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        // Allow user to override default parameters
        if let Some(ParameterValue::Float(gamma)) = parameters.get("risk_aversion") {
            self.params.risk_aversion = *gamma;
        }
        if let Some(ParameterValue::Float(size)) = parameters.get("order_size") {
            self.params.order_size = *size;
        }
        // Order size as percentage of capital (preferred over fixed order_size)
        if let Some(ParameterValue::Float(pct)) = parameters.get("order_size_pct") {
            self.params.order_size_pct = *pct;
        }
        // Initial capital for percentage-based order sizing
        if let Some(ParameterValue::Float(capital)) = parameters.get("initial_capital") {
            self.params.initial_capital = *capital;
        }
        if let Some(ParameterValue::Float(window_ms)) = parameters.get("window_ms") {
            self.params.window_ms = *window_ms as u64;
        }
        if let Some(ParameterValue::Float(lifetime)) = parameters.get("min_quote_lifetime_ms") {
            self.params.min_quote_lifetime_ms = *lifetime as u64;
        }
        if let Some(ParameterValue::Float(fee)) = parameters.get("fee_rate") {
            self.params.fee_rate = *fee;
        }
        if let Some(ParameterValue::Float(buffer)) = parameters.get("latency_buffer") {
            self.params.latency_buffer = *buffer;
        }
        if let Some(ParameterValue::Float(target)) = parameters.get("inventory_target") {
            self.params.inventory_target = *target;
        }
        if let Some(ParameterValue::Float(vol_cap)) = parameters.get("volatility_cap") {
            self.params.volatility_cap = *vol_cap;
        }
        Ok(())
    }

    async fn generate_signals(
        &mut self,
        market_data: &dataloader::MarketData,
        context: &StrategyContext,
    ) -> StrategyResult<Vec<Signal>> {
        // 1. Update price history for volatility estimation
        let now = context.timestamp;
        // Compute mid price from market data (assume Snapshot variant)
        let mid = match market_data {
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Snapshot(snapshot) => {
                // For a single snapshot, use price_level directly
                snapshot.price_level.clone()
            }
            #[cfg(feature = "postgres")]
            dataloader::MarketData::Order(order) => {
                // For order-level data, calculate mid price from context spread if available
                // Otherwise use the last known price from history, or the order price as fallback
                if let Some(_spread) = &context.spread {
                    // If we have spread, use the last known price + half spread as approximation
                    if !context.price_history.is_empty() {
                        let last_price = &context.price_history.last().unwrap().price;
                        // Assume last_price is roughly the mid, so mid ≈ last_price
                        last_price.clone()
                    } else {
                        // Use order price as fallback
                        order.price_level.clone()
                    }
                } else if !context.price_history.is_empty() {
                    context.price_history.last().unwrap().price.clone()
                } else {
                    order.price_level.clone()
                }
            }
            dataloader::MarketData::Candle(candle) => {
                // Use close price as mid price approximation
                candle.close.clone()
            }
            dataloader::MarketData::Trade(trade) => {
                // Use trade price as mid price approximation
                trade.price.clone()
            }
        };
        self.price_history.push((now, mid.clone()));
        // Remove old prices outside window
        let window_start = now - Duration::milliseconds(self.params.window_ms as i64);
        self.price_history.retain(|(ts, _)| *ts >= window_start);

        // 2. Estimate volatility using EWMA (Exponentially Weighted Moving Average)
        // More responsive to recent changes than simple rolling standard deviation
        let lambda = 0.94; // RiskMetrics standard decay factor
        
        let mid_price = mid.to_f64().unwrap_or(0.0);
        
        // Calculate return from last price
        let log_return = if self.price_history.len() >= 2 {
            let prev_price = self.price_history[self.price_history.len() - 2].1.to_f64().unwrap_or(mid_price);
            if prev_price > 0.0 {
                (mid_price / prev_price).ln()
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // Update EWMA mean and variance
        match (self.ewma_mean, self.ewma_variance) {
            (Some(prev_mean), Some(prev_var)) => {
                // EWMA update formulas
                let new_mean = lambda * prev_mean + (1.0 - lambda) * log_return;
                let squared_deviation = (log_return - prev_mean).powi(2);
                let new_var = lambda * prev_var + (1.0 - lambda) * squared_deviation;
                
                self.ewma_mean = Some(new_mean);
                self.ewma_variance = Some(new_var);
            }
            _ => {
                // Initialize EWMA with first return
                self.ewma_mean = Some(log_return);
                self.ewma_variance = Some(0.0);
            }
        }
        
        // Calculate annualized volatility from EWMA variance
        let ewma_stdev = self.ewma_variance.unwrap_or(0.0).sqrt();
        
        // Annualize volatility (252 trading days, assuming ~6.5 hour trading sessions for crypto)
        // For millisecond data: sqrt(252 * 24 * 60 * 60 * 1000) for continuous markets
        let annualization_factor = (252.0 * 24.0 * 60.0 * 60.0 * 1000.0 / self.params.window_ms as f64).sqrt();
        let ann_vol = ewma_stdev * annualization_factor;

        // 3. Pause quoting if volatility is too high
        if ann_vol > self.params.volatility_cap {
            self.paused = true;
            return Ok(vec![]);
        } else {
            self.paused = false;
        }

        // 4. Calculate optimal spread and skew (Avellaneda-Stoikov formulas)
        // Phase 2: Use dynamic parameters instead of static
        let gamma = self.calculate_dynamic_gamma(ann_vol);
        let q_star = self.calculate_dynamic_target();
        
        let sigma = ann_vol;
        let q = self.inventory;
        let s = mid.to_f64().unwrap_or(0.0);
        
        // Use configurable time horizon (converted to years)
        let t = self.params.time_horizon_minutes / 365.0 / 24.0 / 60.0;
        
        // Use configurable market depth parameter
        let k = self.params.market_depth_k;
        
        let fee = self.params.fee_rate;
        let latency = self.params.latency_buffer;

        // Reservation price (skew)
        let r = s - (q - q_star) * gamma * sigma * sigma * t;
        
        // Optimal spread from A-S formula
        let as_spread = gamma * sigma * sigma * t + (2.0 / gamma) * (1.0 + fee + latency) / k;
        
        // Enforce minimum spread to cover transaction costs
        // min_spread_bps is in basis points, convert to price
        let min_spread = s * self.params.min_spread_bps / 10000.0;
        let spread = as_spread.max(min_spread);
        
        let bid = r - spread / 2.0;
        let ask = r + spread / 2.0;

        // 5. Check for exit signals (take profit / stop loss)
        let exit_signals = self.check_exit_conditions(mid_price, now);
        if !exit_signals.is_empty() {
            return Ok(exit_signals);
        }

        // 6. Check trade rate limit
        if !self.can_trade(now) {
            return Ok(vec![]);
        }

        // 7. Only re-quote if price changes by threshold or min quote lifetime elapsed
        let min_lifetime = Duration::milliseconds(self.params.min_quote_lifetime_ms as i64);
        let should_quote = match self.last_quote_time {
            Some(last) => now - last >= min_lifetime
                || self.last_bid.as_ref().map_or(true, |b| (b.to_f64().unwrap_or(0.0) - bid).abs() > 1e-8)
                || self.last_ask.as_ref().map_or(true, |a| (a.to_f64().unwrap_or(0.0) - ask).abs() > 1e-8),
            None => true,
        };
        if !should_quote {
            return Ok(vec![]);
        }
        
        // Record the quote for rate limiting
        self.record_trade();
        
    self.last_quote_time = Some(now);
    self.last_bid = Some(BigDecimal::from_f64(bid).unwrap());
    self.last_ask = Some(BigDecimal::from_f64(ask).unwrap());

        // 8. Calculate dynamic order size based on percentage of capital
        // Use mid price (average of bid/ask) for order size calculation
        let mid_price = (bid + ask) / 2.0;
        let available_balance = context.portfolio_state.balance.to_f64().unwrap_or(0.0);
        let order_size = self.calculate_order_size(mid_price, available_balance);
        
        // 9. Generate bid and ask signals with percentage-based sizing
        let mut signals = Vec::new();
        signals.push(Signal::new(
            format!("as_bid_{}", now.timestamp_millis()),
            SignalType::Buy,
            SignalStrength::Medium,
            SignalReason::Custom("AvellanedaStoikov".to_string(), "Optimal bid quote".to_string()),
        ).with_price(BigDecimal::from_f64(bid).unwrap())
         .with_quantity(BigDecimal::from_f64(order_size).unwrap()));
        signals.push(Signal::new(
            format!("as_ask_{}", now.timestamp_millis()),
            SignalType::Sell,
            SignalStrength::Medium,
            SignalReason::Custom("AvellanedaStoikov".to_string(), "Optimal ask quote".to_string()),
        ).with_price(BigDecimal::from_f64(ask).unwrap())
         .with_quantity(BigDecimal::from_f64(order_size).unwrap()));
        Ok(signals)
    }

    async fn update_state(&mut self, market_data: &dataloader::MarketData, context: &StrategyContext) -> StrategyResult<()> {
        let old_inventory = self.inventory;
        
        // Update inventory from portfolio state
        self.inventory = context.portfolio_state.net_position;
        
        // Track entry price when position is opened or increased
        if old_inventory.abs() < 1e-8 && self.inventory.abs() > 1e-8 {
            // New position opened - get current price as entry
            let current_price = match market_data {
                dataloader::MarketData::Trade(t) => t.price.to_f64().unwrap_or(0.0),
                dataloader::MarketData::Candle(c) => c.close.to_f64().unwrap_or(0.0),
                #[cfg(feature = "postgres")]
                dataloader::MarketData::Snapshot(s) => s.price_level.to_f64().unwrap_or(0.0),
                #[cfg(feature = "postgres")]
                dataloader::MarketData::Order(o) => o.price_level.to_f64().unwrap_or(0.0),
            };
            self.avg_entry_price = Some(current_price);
        } else if self.inventory.abs() < 1e-8 {
            // Position closed
            self.avg_entry_price = None;
        }
        
        // Phase 2: Update volume tracking for dynamic adjustments
        self.update_volume_tracking(market_data);
        
        Ok(())
    }

    fn get_parameters(&self) -> HashMap<String, ParameterValue> {
        let mut params = HashMap::new();
    params.insert("risk_aversion".to_string(), ParameterValue::Float(self.params.risk_aversion));
    params.insert("order_size".to_string(), ParameterValue::Float(self.params.order_size));
    params.insert("window_ms".to_string(), ParameterValue::Float(self.params.window_ms as f64));
    params.insert("min_quote_lifetime_ms".to_string(), ParameterValue::Float(self.params.min_quote_lifetime_ms as f64));
    params.insert("fee_rate".to_string(), ParameterValue::Float(self.params.fee_rate));
    params.insert("latency_buffer".to_string(), ParameterValue::Float(self.params.latency_buffer));
    params.insert("inventory_target".to_string(), ParameterValue::Float(self.params.inventory_target));
    params.insert("volatility_cap".to_string(), ParameterValue::Float(self.params.volatility_cap));
        params
    }

    async fn update_parameters(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        self.initialize(parameters).await
    }

    fn get_risk_limits(&self) -> RiskLimits {
        RiskLimits {
            max_position: Some(10.0),
            max_order_size: Some(self.params.order_size),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(self.params.volatility_cap),
            // New risk controls to prevent massive losses
            max_drawdown: Some(0.15),      // Halt trading at 15% drawdown
            stop_loss_pct: Some(0.05),     // 5% stop-loss per position
            max_daily_loss: Some(0.10),    // 10% max daily loss
            ..Default::default()
        }
    }
}
