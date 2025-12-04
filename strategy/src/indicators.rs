//! Technical analysis indicators for trading strategies
//!
//! This module provides a comprehensive set of technical indicators that can be used
//! by trading strategies for market analysis and signal generation.

use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use statrs::statistics::Statistics;

/// Error types for indicator calculations
#[derive(Debug, thiserror::Error)]
pub enum IndicatorError {
    #[error("Insufficient data: need at least {required} points, got {available}")]
    InsufficientData { required: usize, available: usize },
    
    #[error("Invalid parameter: {parameter} = {value}")]
    InvalidParameter { parameter: String, value: String },
    
    #[error("Division by zero in indicator calculation")]
    DivisionByZero,
    
    #[error("Invalid price data")]
    InvalidPriceData,
    
    #[error("Calculation error: {0}")]
    CalculationError(String),
}

/// Result type for indicator calculations
pub type IndicatorResult<T> = Result<T, IndicatorError>;

/// Price data point for indicators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub timestamp: DateTime<Utc>,
    pub open: BigDecimal,
    pub high: BigDecimal,
    pub low: BigDecimal,
    pub close: BigDecimal,
    pub volume: BigDecimal,
}

/// Simple indicator value with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorValue {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// Moving Average types
#[derive(Debug, Clone, PartialEq)]
pub enum MovingAverageType {
    Simple,
    Exponential,
    Weighted,
    Hull,
}

/// Simple Moving Average (SMA) indicator
#[derive(Debug)]
pub struct SimpleMovingAverage {
    period: usize,
    values: VecDeque<f64>,
}

impl SimpleMovingAverage {
    /// Create a new SMA indicator
    pub fn new(period: usize) -> IndicatorResult<Self> {
        if period == 0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "period".to_string(),
                value: period.to_string(),
            });
        }

        Ok(Self {
            period,
            values: VecDeque::new(),
        })
    }

    /// Add a new price and calculate the SMA
    pub fn next(&mut self, price: f64) -> Option<f64> {
        self.values.push_back(price);

        if self.values.len() > self.period {
            self.values.pop_front();
        }

        if self.values.len() == self.period {
            Some(self.values.iter().sum::<f64>() / self.period as f64)
        } else {
            None
        }
    }

    /// Get current value without adding new data
    pub fn current(&self) -> Option<f64> {
        if self.values.len() == self.period {
            Some(self.values.iter().sum::<f64>() / self.period as f64)
        } else {
            None
        }
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.values.clear();
    }
}

/// Exponential Moving Average (EMA) indicator
#[derive(Debug)]
pub struct ExponentialMovingAverage {
    #[allow(dead_code)]
    period: usize,
    multiplier: f64,
    current_value: Option<f64>,
    initialized: bool,
}

impl ExponentialMovingAverage {
    /// Create a new EMA indicator
    pub fn new(period: usize) -> IndicatorResult<Self> {
        if period == 0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "period".to_string(),
                value: period.to_string(),
            });
        }

        let multiplier = 2.0 / (period as f64 + 1.0);

        Ok(Self {
            period,
            multiplier,
            current_value: None,
            initialized: false,
        })
    }

    /// Add a new price and calculate the EMA
    pub fn next(&mut self, price: f64) -> Option<f64> {
        if !self.initialized {
            self.current_value = Some(price);
            self.initialized = true;
        } else {
            let current = self.current_value.unwrap();
            self.current_value = Some(price * self.multiplier + current * (1.0 - self.multiplier));
        }

        self.current_value
    }

    /// Get current value
    pub fn current(&self) -> Option<f64> {
        self.current_value
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.current_value = None;
        self.initialized = false;
    }
}

/// Relative Strength Index (RSI) indicator
#[derive(Debug)]
pub struct RelativeStrengthIndex {
    period: usize,
    gains: VecDeque<f64>,
    losses: VecDeque<f64>,
    last_price: Option<f64>,
}

impl RelativeStrengthIndex {
    /// Create a new RSI indicator
    pub fn new(period: usize) -> IndicatorResult<Self> {
        if period == 0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "period".to_string(),
                value: period.to_string(),
            });
        }

        Ok(Self {
            period,
            gains: VecDeque::new(),
            losses: VecDeque::new(),
            last_price: None,
        })
    }

    /// Add a new price and calculate RSI
    pub fn next(&mut self, price: f64) -> Option<f64> {
        if let Some(last_price) = self.last_price {
            let change = price - last_price;
            let gain = if change > 0.0 { change } else { 0.0 };
            let loss = if change < 0.0 { -change } else { 0.0 };

            self.gains.push_back(gain);
            self.losses.push_back(loss);

            if self.gains.len() > self.period {
                self.gains.pop_front();
                self.losses.pop_front();
            }

            if self.gains.len() == self.period {
                let avg_gain = self.gains.iter().sum::<f64>() / self.period as f64;
                let avg_loss = self.losses.iter().sum::<f64>() / self.period as f64;

                if avg_loss == 0.0 {
                    Some(100.0)
                } else {
                    let rs = avg_gain / avg_loss;
                    Some(100.0 - (100.0 / (1.0 + rs)))
                }
            } else {
                None
            }
        } else {
            self.last_price = Some(price);
            None
        }
    }

    /// Get current RSI value
    pub fn current(&self) -> Option<f64> {
        if self.gains.len() == self.period {
            let avg_gain = self.gains.iter().sum::<f64>() / self.period as f64;
            let avg_loss = self.losses.iter().sum::<f64>() / self.period as f64;

            if avg_loss == 0.0 {
                Some(100.0)
            } else {
                let rs = avg_gain / avg_loss;
                Some(100.0 - (100.0 / (1.0 + rs)))
            }
        } else {
            None
        }
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.last_price = None;
    }
}

/// Moving Average Convergence Divergence (MACD) indicator
#[derive(Debug)]
pub struct MACD {
    fast_ema: ExponentialMovingAverage,
    slow_ema: ExponentialMovingAverage,
    signal_ema: ExponentialMovingAverage,
    macd_history: VecDeque<f64>,
}

/// MACD result containing all three lines
#[derive(Debug, Clone)]
pub struct MACDResult {
    pub macd: f64,
    pub signal: Option<f64>,
    pub histogram: Option<f64>,
}

impl MACD {
    /// Create a new MACD indicator with standard parameters (12, 26, 9)
    pub fn new() -> IndicatorResult<Self> {
        Self::with_params(12, 26, 9)
    }

    /// Create MACD with custom parameters
    pub fn with_params(fast_period: usize, slow_period: usize, signal_period: usize) -> IndicatorResult<Self> {
        Ok(Self {
            fast_ema: ExponentialMovingAverage::new(fast_period)?,
            slow_ema: ExponentialMovingAverage::new(slow_period)?,
            signal_ema: ExponentialMovingAverage::new(signal_period)?,
            macd_history: VecDeque::new(),
        })
    }

    /// Add a new price and calculate MACD
    pub fn next(&mut self, price: f64) -> Option<MACDResult> {
        let fast_ema = self.fast_ema.next(price)?;
        let slow_ema = self.slow_ema.next(price)?;

        let macd = fast_ema - slow_ema;
        let signal = self.signal_ema.next(macd);
        let histogram = signal.map(|s| macd - s);

        Some(MACDResult {
            macd,
            signal,
            histogram,
        })
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.fast_ema.reset();
        self.slow_ema.reset();
        self.signal_ema.reset();
        self.macd_history.clear();
    }
}

/// Bollinger Bands indicator
#[derive(Debug)]
pub struct BollingerBands {
    period: usize,
    std_dev_multiplier: f64,
    sma: SimpleMovingAverage,
    values: VecDeque<f64>,
}

/// Bollinger Bands result
#[derive(Debug, Clone)]
pub struct BollingerBandsResult {
    pub middle: f64,  // SMA
    pub upper: f64,   // Upper band
    pub lower: f64,   // Lower band
    pub bandwidth: f64,
}

impl BollingerBands {
    /// Create new Bollinger Bands with standard parameters (20, 2.0)
    pub fn new() -> IndicatorResult<Self> {
        Self::with_params(20, 2.0)
    }

    /// Create Bollinger Bands with custom parameters
    pub fn with_params(period: usize, std_dev_multiplier: f64) -> IndicatorResult<Self> {
        if std_dev_multiplier <= 0.0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "std_dev_multiplier".to_string(),
                value: std_dev_multiplier.to_string(),
            });
        }

        Ok(Self {
            period,
            std_dev_multiplier,
            sma: SimpleMovingAverage::new(period)?,
            values: VecDeque::new(),
        })
    }

    /// Add a new price and calculate Bollinger Bands
    pub fn next(&mut self, price: f64) -> Option<BollingerBandsResult> {
        self.values.push_back(price);
        if self.values.len() > self.period {
            self.values.pop_front();
        }

        let middle = self.sma.next(price)?;

        if self.values.len() == self.period {
            let values_vec: Vec<f64> = self.values.iter().cloned().collect();
            let std_dev = values_vec.std_dev();
            
            let band_width = self.std_dev_multiplier * std_dev;
            let upper = middle + band_width;
            let lower = middle - band_width;

            Some(BollingerBandsResult {
                middle,
                upper,
                lower,
                bandwidth: (upper - lower) / middle * 100.0,
            })
        } else {
            None
        }
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.sma.reset();
        self.values.clear();
    }
}

/// Stochastic Oscillator indicator
#[derive(Debug)]
pub struct StochasticOscillator {
    k_period: usize,
    d_period: usize,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    closes: VecDeque<f64>,
    k_values: VecDeque<f64>,
}

/// Stochastic result
#[derive(Debug, Clone)]
pub struct StochasticResult {
    pub k_percent: f64,
    pub d_percent: Option<f64>,
}

impl StochasticOscillator {
    /// Create new Stochastic with standard parameters (14, 3)
    pub fn new() -> IndicatorResult<Self> {
        Self::with_params(14, 3)
    }

    /// Create Stochastic with custom parameters
    pub fn with_params(k_period: usize, d_period: usize) -> IndicatorResult<Self> {
        if k_period == 0 || d_period == 0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "period".to_string(),
                value: format!("k_period: {}, d_period: {}", k_period, d_period),
            });
        }

        Ok(Self {
            k_period,
            d_period,
            highs: VecDeque::new(),
            lows: VecDeque::new(),
            closes: VecDeque::new(),
            k_values: VecDeque::new(),
        })
    }

    /// Add new OHLC data and calculate Stochastic
    pub fn next(&mut self, high: f64, low: f64, close: f64) -> Option<StochasticResult> {
        self.highs.push_back(high);
        self.lows.push_back(low);
        self.closes.push_back(close);

        if self.highs.len() > self.k_period {
            self.highs.pop_front();
            self.lows.pop_front();
            self.closes.pop_front();
        }

        if self.highs.len() == self.k_period {
            let highest_high = self.highs.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            let lowest_low = self.lows.iter().fold(f64::INFINITY, |a, &b| a.min(b));

            if highest_high == lowest_low {
                return Some(StochasticResult {
                    k_percent: 50.0,
                    d_percent: None,
                });
            }

            let k_percent = ((close - lowest_low) / (highest_high - lowest_low)) * 100.0;
            self.k_values.push_back(k_percent);

            if self.k_values.len() > self.d_period {
                self.k_values.pop_front();
            }

            let d_percent = if self.k_values.len() == self.d_period {
                Some(self.k_values.iter().sum::<f64>() / self.d_period as f64)
            } else {
                None
            };

            Some(StochasticResult {
                k_percent,
                d_percent,
            })
        } else {
            None
        }
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.closes.clear();
        self.k_values.clear();
    }
}

/// Average True Range (ATR) indicator for volatility measurement
#[derive(Debug)]
pub struct AverageTrueRange {
    period: usize,
    true_ranges: VecDeque<f64>,
    previous_close: Option<f64>,
}

impl AverageTrueRange {
    /// Create new ATR with default period (14)
    pub fn new() -> IndicatorResult<Self> {
        Self::with_period(14)
    }

    /// Create ATR with custom period
    pub fn with_period(period: usize) -> IndicatorResult<Self> {
        if period == 0 {
            return Err(IndicatorError::InvalidParameter {
                parameter: "period".to_string(),
                value: period.to_string(),
            });
        }

        Ok(Self {
            period,
            true_ranges: VecDeque::new(),
            previous_close: None,
        })
    }

    /// Add new OHLC data and calculate ATR
    pub fn next(&mut self, high: f64, low: f64, close: f64) -> Option<f64> {
        let true_range = if let Some(prev_close) = self.previous_close {
            let hl = high - low;
            let hc = (high - prev_close).abs();
            let lc = (low - prev_close).abs();
            hl.max(hc).max(lc)
        } else {
            high - low
        };

        self.previous_close = Some(close);
        self.true_ranges.push_back(true_range);

        if self.true_ranges.len() > self.period {
            self.true_ranges.pop_front();
        }

        if self.true_ranges.len() == self.period {
            Some(self.true_ranges.iter().sum::<f64>() / self.period as f64)
        } else {
            None
        }
    }

    /// Reset the indicator
    pub fn reset(&mut self) {
        self.true_ranges.clear();
        self.previous_close = None;
    }
}

/// Utility functions for indicator calculations
pub mod utils {
    use super::*;

    /// Calculate standard deviation for a slice of values
    pub fn standard_deviation(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / values.len() as f64;
        
        variance.sqrt()
    }

    /// Convert BigDecimal to f64 for calculations
    pub fn bigdecimal_to_f64(value: &BigDecimal) -> Result<f64, IndicatorError> {
        value.to_string().parse::<f64>()
            .map_err(|_| IndicatorError::InvalidPriceData)
    }

    /// Calculate simple moving average from a vector
    pub fn simple_moving_average(values: &[f64], period: usize) -> IndicatorResult<Vec<f64>> {
        if values.len() < period || period == 0 {
            return Err(IndicatorError::InsufficientData {
                required: period,
                available: values.len(),
            });
        }

        let mut sma_values = Vec::new();
        for i in (period - 1)..values.len() {
            let start_idx = i + 1 - period; // Safe because i >= period - 1
            let sum: f64 = values[start_idx..=i].iter().sum();
            sma_values.push(sum / period as f64);
        }

        Ok(sma_values)
    }

    /// Calculate exponential moving average from a vector
    pub fn exponential_moving_average(values: &[f64], period: usize) -> IndicatorResult<Vec<f64>> {
        if values.is_empty() {
            return Ok(Vec::new());
        }

        let multiplier = 2.0 / (period as f64 + 1.0);
        let mut ema_values = Vec::new();
        let mut ema = values[0]; // Start with first value

        ema_values.push(ema);

        for &value in values.iter().skip(1) {
            ema = value * multiplier + ema * (1.0 - multiplier);
            ema_values.push(ema);
        }

        Ok(ema_values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_moving_average() {
        let mut sma = SimpleMovingAverage::new(3).unwrap();

        assert!(sma.next(1.0).is_none());
        assert!(sma.next(2.0).is_none());
        assert_eq!(sma.next(3.0), Some(2.0));
        assert_eq!(sma.next(4.0), Some(3.0));
        assert_eq!(sma.next(5.0), Some(4.0));
    }

    #[test]
    fn test_exponential_moving_average() {
        let mut ema = ExponentialMovingAverage::new(3).unwrap();

        let first = ema.next(1.0).unwrap();
        assert_eq!(first, 1.0);

        let second = ema.next(2.0).unwrap();
        assert!(second > 1.0 && second < 2.0);
    }

    #[test]
    fn test_rsi() {
        let mut rsi = RelativeStrengthIndex::new(14).unwrap();

        // Add 14 values with increasing trend
        for i in 1..=14 {
            rsi.next(i as f64);
        }

        // Add a decreasing value
        if let Some(rsi_value) = rsi.next(13.0) {
            assert!(rsi_value <= 100.0 && rsi_value >= 0.0);
        }
    }

    #[test]
    fn test_bollinger_bands() {
        let mut bb = BollingerBands::new().unwrap();

        // Need 20 values for standard Bollinger Bands
        for i in 1..=20 {
            if let Some(result) = bb.next(i as f64) {
                assert!(result.upper > result.middle);
                assert!(result.lower < result.middle);
                assert!(result.bandwidth > 0.0);
            }
        }
    }

    #[test]
    fn test_stochastic() {
        let mut stoch = StochasticOscillator::new().unwrap();

        // Add 14 periods of data
        for i in 1..=14 {
            let high = i as f64 + 1.0;
            let low = i as f64 - 1.0;
            let close = i as f64;

            if let Some(result) = stoch.next(high, low, close) {
                assert!(result.k_percent >= 0.0 && result.k_percent <= 100.0);
                if let Some(d_percent) = result.d_percent {
                    assert!(d_percent >= 0.0 && d_percent <= 100.0);
                }
            }
        }
    }

    #[test]
    fn test_atr() {
        let mut atr = AverageTrueRange::new().unwrap();

        for i in 1..=14 {
            let high = i as f64 + 2.0;
            let low = i as f64 - 1.0;
            let close = i as f64;

            if let Some(atr_value) = atr.next(high, low, close) {
                assert!(atr_value > 0.0);
            }
        }
    }

    #[test]
    fn test_utils_standard_deviation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let std_dev = utils::standard_deviation(&values);
        assert!(std_dev > 0.0);
    }

    #[test]
    fn test_utils_simple_moving_average() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sma = utils::simple_moving_average(&values, 3).unwrap();
        assert_eq!(sma, vec![2.0, 3.0, 4.0]);
    }
}
