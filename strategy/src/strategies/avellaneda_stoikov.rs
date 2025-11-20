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
    pub risk_aversion: f64,         // γ: risk aversion parameter
    pub order_size: f64,            // Fixed order size (in base asset)
    pub window_ms: u64,             // Rolling window for volatility estimation (milliseconds)
    pub min_quote_lifetime_ms: u64, // Minimum quote lifetime (milliseconds)
    pub fee_rate: f64,              // Exchange taker/maker fee (fraction)
    pub latency_buffer: f64,        // Price buffer for latency/slippage
    pub inventory_target: f64,      // Target inventory (0 = flat)
    pub volatility_cap: f64,        // Max allowed volatility (pause quoting above this)
    pub time_horizon_minutes: f64,  // t: time horizon for optimization (in minutes)
    pub market_depth_k: f64,        // k: market depth parameter (higher = tighter spread)
}

impl Default for AvellanedaStoikovParams {
    fn default() -> Self {
        Self {
            risk_aversion: 0.01,
            order_size: 0.01,
            window_ms: 60_000, // 60 seconds
            min_quote_lifetime_ms: 500,
            fee_rate: 0.0005,
            latency_buffer: 0.5,
            inventory_target: 0.0,
            volatility_cap: 0.10, // 10% annualized
            time_horizon_minutes: 1.0, // 1 minute default
            market_depth_k: 1.5, // Default market depth parameter
        }
    }
}

impl AvellanedaStoikovParams {
    /// Validate parameter bounds
    pub fn validate(&self) -> Result<(), String> {
        if self.risk_aversion <= 0.0 || self.risk_aversion > 1.0 {
            return Err(format!("risk_aversion must be in (0, 1], got {}", self.risk_aversion));
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
            dataloader::MarketData::Order(_) => {
                return Err(StrategyError::MarketDataError("No mid price available".to_string()));
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
        let gamma = self.params.risk_aversion;
        let sigma = ann_vol;
        let q = self.inventory;
        let q_star = self.params.inventory_target;
        let s = mid.to_f64().unwrap_or(0.0);
        
        // Use configurable time horizon (converted to years)
        let t = self.params.time_horizon_minutes / 365.0 / 24.0 / 60.0;
        
        // Use configurable market depth parameter
        let k = self.params.market_depth_k;
        
        let fee = self.params.fee_rate;
        let latency = self.params.latency_buffer;

        // Reservation price (skew)
        let r = s - (q - q_star) * gamma * sigma * sigma * t;
        
        // Optimal spread
        let spread = gamma * sigma * sigma * t + (2.0 / gamma) * (1.0 + fee + latency) / k;
        let bid = r - spread / 2.0;
        let ask = r + spread / 2.0;

        // 5. Only re-quote if price changes by threshold or min quote lifetime elapsed
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
    self.last_quote_time = Some(now);
    self.last_bid = Some(BigDecimal::from_f64(bid).unwrap());
    self.last_ask = Some(BigDecimal::from_f64(ask).unwrap());

        // 6. Generate bid and ask signals
        let mut signals = Vec::new();
        signals.push(Signal::new(
            format!("as_bid_{}", now.timestamp_millis()),
            SignalType::Buy,
            SignalStrength::Medium,
            SignalReason::Custom("AvellanedaStoikov".to_string(), "Optimal bid quote".to_string()),
        ).with_price(BigDecimal::from_f64(bid).unwrap())
         .with_quantity(BigDecimal::from_f64(self.params.order_size).unwrap()));
        signals.push(Signal::new(
            format!("as_ask_{}", now.timestamp_millis()),
            SignalType::Sell,
            SignalStrength::Medium,
            SignalReason::Custom("AvellanedaStoikov".to_string(), "Optimal ask quote".to_string()),
        ).with_price(BigDecimal::from_f64(ask).unwrap())
         .with_quantity(BigDecimal::from_f64(self.params.order_size).unwrap()));
        Ok(signals)
    }

    async fn update_state(&mut self, market_data: &dataloader::MarketData, context: &StrategyContext) -> StrategyResult<()> {
    let _ = market_data; // silence unused variable warning
    // Update inventory from portfolio state
    self.inventory = context.portfolio_state.net_position;
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
        }
    }
}
