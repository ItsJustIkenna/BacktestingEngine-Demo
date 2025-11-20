//! RSI Mean Reversion Strategy
//!
//! A strategy that uses the Relative Strength Index (RSI) to identify overbought
//! and oversold conditions for mean reversion trading.

use crate::traits::*;
use crate::signal::Signal;
use dataloader::MarketData;
use config::ParameterValue;
use riskmanager::RiskLimits;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use std::collections::{HashMap, VecDeque};

/// Parameters for the RSI Mean Reversion strategy
#[derive(Debug, Clone)]
pub struct RSIMeanReversionParams {
    pub rsi_period: usize,           // Period for RSI calculation
    pub oversold_threshold: f64,     // RSI level considered oversold (buy signal)
    pub overbought_threshold: f64,   // RSI level considered overbought (sell signal)
    pub position_size: f64,          // Position size as fraction of portfolio
    pub min_hold_period: u64,        // Minimum holding period in minutes
    pub profit_target: f64,          // Profit target percentage
    pub stop_loss: f64,              // Stop loss percentage
    pub volatility_filter: bool,     // Enable volatility filtering
    pub volume_filter: bool,         // Enable volume filtering
    pub min_volume_ratio: f64,       // Minimum volume ratio vs average
}

impl Default for RSIMeanReversionParams {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            oversold_threshold: 30.0,
            overbought_threshold: 70.0,
            position_size: 0.1,
            min_hold_period: 60, // 1 hour
            profit_target: 2.0,  // 2%
            stop_loss: 1.0,      // 1%
            volatility_filter: true,
            volume_filter: false,
            min_volume_ratio: 1.2,
        }
    }
}

/// RSI calculation helper
#[derive(Debug)]
#[allow(dead_code)]
struct RSICalculator {
    period: usize,
    gains: VecDeque<f64>,
    losses: VecDeque<f64>,
    avg_gain: f64,
    avg_loss: f64,
}

impl RSICalculator {
    fn new(period: usize) -> Self {
        Self {
            period,
            gains: VecDeque::new(),
            losses: VecDeque::new(),
            avg_gain: 0.0,
            avg_loss: 0.0,
        }
    }

    #[allow(dead_code)]
    fn update(&mut self, price_change: f64) -> Option<f64> {
        let gain = if price_change > 0.0 { price_change } else { 0.0 };
        let loss = if price_change < 0.0 { -price_change } else { 0.0 };

        self.gains.push_back(gain);
        self.losses.push_back(loss);

        if self.gains.len() > self.period {
            self.gains.pop_front();
            self.losses.pop_front();
        }

        if self.gains.len() < self.period {
            return None;
        }

        // Calculate average gain and loss
        let sum_gains: f64 = self.gains.iter().sum();
        let sum_losses: f64 = self.losses.iter().sum();
        
        self.avg_gain = sum_gains / self.period as f64;
        self.avg_loss = sum_losses / self.period as f64;

        // Calculate RSI
        if self.avg_loss == 0.0 {
            return Some(100.0);
        }

        let rs = self.avg_gain / self.avg_loss;
        let rsi = 100.0 - (100.0 / (1.0 + rs));
        
        Some(rsi)
    }
}

/// RSI Mean Reversion Strategy
#[derive(Debug)]
#[allow(dead_code)]
pub struct RSIMeanReversionStrategy {
    params: RSIMeanReversionParams,
    rsi_calculator: RSICalculator,
    price_history: VecDeque<(DateTime<Utc>, BigDecimal)>,
    volume_history: VecDeque<f64>,
    current_position: f64,
    position_entry_time: Option<DateTime<Utc>>,
    position_entry_price: Option<BigDecimal>,
    last_rsi: Option<f64>,
}

impl Default for RSIMeanReversionStrategy {
    fn default() -> Self {
        let params = RSIMeanReversionParams::default();
        Self {
            rsi_calculator: RSICalculator::new(params.rsi_period),
            params,
            price_history: VecDeque::new(),
            volume_history: VecDeque::new(),
            current_position: 0.0,
            position_entry_time: None,
            position_entry_price: None,
            last_rsi: None,
        }
    }
}

#[async_trait]
impl Strategy for RSIMeanReversionStrategy {
    fn name(&self) -> &str {
        "RSIMeanReversion"
    }

    fn description(&self) -> &str {
        "Mean reversion strategy using RSI to identify overbought/oversold conditions"
    }

    async fn initialize(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        // Allow user to override default parameters
        if let Some(ParameterValue::Integer(period)) = parameters.get("rsi_period") {
            self.params.rsi_period = *period as usize;
            self.rsi_calculator = RSICalculator::new(self.params.rsi_period);
        }
        if let Some(ParameterValue::Float(threshold)) = parameters.get("oversold_threshold") {
            self.params.oversold_threshold = *threshold;
        }
        if let Some(ParameterValue::Float(threshold)) = parameters.get("overbought_threshold") {
            self.params.overbought_threshold = *threshold;
        }
        if let Some(ParameterValue::Float(size)) = parameters.get("position_size") {
            self.params.position_size = *size;
        }
        if let Some(ParameterValue::Integer(period)) = parameters.get("min_hold_period") {
            self.params.min_hold_period = *period as u64;
        }
        if let Some(ParameterValue::Float(target)) = parameters.get("profit_target") {
            self.params.profit_target = *target;
        }
        if let Some(ParameterValue::Float(stop)) = parameters.get("stop_loss") {
            self.params.stop_loss = *stop;
        }
        if let Some(ParameterValue::Boolean(filter)) = parameters.get("volatility_filter") {
            self.params.volatility_filter = *filter;
        }
        if let Some(ParameterValue::Boolean(filter)) = parameters.get("volume_filter") {
            self.params.volume_filter = *filter;
        }
        if let Some(ParameterValue::Float(ratio)) = parameters.get("min_volume_ratio") {
            self.params.min_volume_ratio = *ratio;
        }

        Ok(())
    }

    async fn generate_signals(
        &mut self,
        _market_data: &MarketData,
        _context: &StrategyContext,
    ) -> StrategyResult<Vec<Signal>> {
        // This would contain the actual signal generation logic
        // For now, return empty signals
        Ok(vec![])
    }

    fn get_parameters(&self) -> HashMap<String, ParameterValue> {
        let mut params = HashMap::new();
        params.insert("rsi_period".to_string(), ParameterValue::Integer(self.params.rsi_period as i64));
        params.insert("oversold_threshold".to_string(), ParameterValue::Float(self.params.oversold_threshold));
        params.insert("overbought_threshold".to_string(), ParameterValue::Float(self.params.overbought_threshold));
        params.insert("position_size".to_string(), ParameterValue::Float(self.params.position_size));
        params.insert("profit_target".to_string(), ParameterValue::Float(self.params.profit_target));
        params.insert("stop_loss".to_string(), ParameterValue::Float(self.params.stop_loss));
        params.insert("min_hold_period".to_string(), ParameterValue::Integer(self.params.min_hold_period as i64));
        params.insert("volatility_filter".to_string(), ParameterValue::Boolean(self.params.volatility_filter));
        params.insert("volume_filter".to_string(), ParameterValue::Boolean(self.params.volume_filter));
        params.insert("min_volume_ratio".to_string(), ParameterValue::Float(self.params.min_volume_ratio));
        params
    }

    async fn update_parameters(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        self.initialize(parameters).await
    }

    fn get_risk_limits(&self) -> RiskLimits {
        RiskLimits {
            max_position: Some(self.params.position_size * 10.0), // 10x position size as max
            max_order_size: Some(self.params.position_size),
            max_inventory_skew: Some(2.0),
            max_volatility: Some(0.10),
        }
    }
}
