//! Momentum Strategy Simulation
//!
//! Unlike market making (which provides liquidity and suffers adverse selection),
//! momentum strategies CONSUME liquidity and profit from price trends.
//!
//! # Key Concepts
//!
//! 1. **Signal Detection**: Identify momentum, orderbook imbalance, and volume spikes
//! 2. **Position Entry**: Take directional positions when signals align
//! 3. **Risk Management**: Stop loss, take profit, trailing stops
//! 4. **Position Sizing**: Kelly criterion, volatility scaling
//!
//! # Why Momentum Works Better Than MM on CEXs
//!
//! - You're the "informed trader" using signals to predict direction
//! - Asymmetric payoffs: small losses, big wins (can profit with 40% win rate)
//! - No adverse selection: you choose when to trade, not when others pick you off

use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use genetic::MomentumChromosome;

use crate::realistic_simulation::{OrderBookEvent, EventType, Side, FillModelType};

// ============================================================================
// MOMENTUM STRATEGY PARAMETERS
// ============================================================================

/// Parameters for momentum strategy simulation
/// Derived from MomentumChromosome but with runtime state
#[derive(Debug, Clone)]
pub struct MomentumParams {
    // === Entry Signals ===
    pub momentum_threshold_pct: f64,
    pub momentum_lookback_ms: i64,
    pub volume_spike_threshold: f64,
    pub volume_lookback_ms: i64,
    pub imbalance_threshold: f64,
    pub min_confirmations: u32,
    pub require_breakout: bool,
    pub breakout_lookback_ms: i64,
    
    // === Position Sizing ===
    pub position_size_pct: f64,
    pub max_position_pct: f64,
    pub signal_strength_scaling: f64,
    pub kelly_fraction: f64,
    pub volatility_position_scaling: f64,
    
    // === Scale-in ===
    pub scale_in_enabled: bool,
    pub scale_in_profit_threshold_pct: f64,
    pub max_scale_ins: u32,
    pub scale_in_size_multiplier: f64,
    
    // === Exit Signals ===
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    pub trailing_stop_enabled: bool,
    pub trailing_stop_pct: f64,
    pub trailing_activation_pct: f64,
    pub momentum_exit_threshold_pct: f64,
    pub max_hold_time_ms: i64,
    pub min_hold_time_ms: i64,
    pub exit_on_imbalance_reversal: bool,
    
    // === Volatility Adaptation ===
    pub volatility_window_ms: i64,
    pub volatility_cap: f64,
    pub volatility_floor: f64,
    pub high_vol_stop_multiplier: f64,
    pub high_vol_tp_multiplier: f64,
    
    // === Risk Management ===
    pub max_daily_trades: u32,
    pub max_hourly_trades: u32,
    pub cooldown_after_loss_ms: i64,
    pub consecutive_loss_limit: u32,
    pub extended_cooldown_ms: i64,
    pub max_daily_drawdown_pct: f64,
    pub max_total_drawdown_pct: f64,
    pub risk_per_trade_pct: f64,
    
    // === Market Regime Filters ===
    pub trend_filter_enabled: bool,
    pub min_trend_strength: f64,
    pub chop_filter_enabled: bool,
    pub chop_threshold: f64,
    
    // === Order Execution ===
    pub use_limit_entry: bool,
    pub limit_entry_offset_bps: f64,
    pub max_slippage_bps: f64,
    pub limit_order_timeout_ms: i64,
    
    // === Fee Configuration ===
    pub taker_fee_rate: f64,
    pub maker_fee_rate: f64,
    
    // === Fill Model ===
    pub fill_model: FillModelType,
}

impl From<&MomentumChromosome> for MomentumParams {
    fn from(chromo: &MomentumChromosome) -> Self {
        Self {
            momentum_threshold_pct: chromo.momentum_threshold_pct,
            momentum_lookback_ms: chromo.momentum_lookback_ms,
            volume_spike_threshold: chromo.volume_spike_threshold,
            volume_lookback_ms: chromo.volume_lookback_ms,
            imbalance_threshold: chromo.imbalance_threshold,
            min_confirmations: chromo.min_confirmations,
            require_breakout: chromo.require_breakout,
            breakout_lookback_ms: chromo.breakout_lookback_ms,
            
            position_size_pct: chromo.position_size_pct,
            max_position_pct: chromo.max_position_pct,
            signal_strength_scaling: chromo.signal_strength_scaling,
            kelly_fraction: chromo.kelly_fraction,
            volatility_position_scaling: chromo.volatility_position_scaling,
            
            scale_in_enabled: chromo.scale_in_enabled,
            scale_in_profit_threshold_pct: chromo.scale_in_profit_threshold_pct,
            max_scale_ins: chromo.max_scale_ins,
            scale_in_size_multiplier: chromo.scale_in_size_multiplier,
            
            take_profit_pct: chromo.take_profit_pct,
            stop_loss_pct: chromo.stop_loss_pct,
            trailing_stop_enabled: chromo.trailing_stop_enabled,
            trailing_stop_pct: chromo.trailing_stop_pct,
            trailing_activation_pct: chromo.trailing_activation_pct,
            momentum_exit_threshold_pct: chromo.momentum_exit_threshold_pct,
            max_hold_time_ms: chromo.max_hold_time_ms,
            min_hold_time_ms: chromo.min_hold_time_ms,
            exit_on_imbalance_reversal: chromo.exit_on_imbalance_reversal,
            
            volatility_window_ms: chromo.volatility_window_ms,
            volatility_cap: chromo.volatility_cap,
            volatility_floor: chromo.volatility_floor,
            high_vol_stop_multiplier: chromo.high_vol_stop_multiplier,
            high_vol_tp_multiplier: chromo.high_vol_tp_multiplier,
            
            max_daily_trades: chromo.max_daily_trades,
            max_hourly_trades: chromo.max_hourly_trades,
            cooldown_after_loss_ms: chromo.cooldown_after_loss_ms,
            consecutive_loss_limit: chromo.consecutive_loss_limit,
            extended_cooldown_ms: chromo.extended_cooldown_ms,
            max_daily_drawdown_pct: chromo.max_daily_drawdown_pct,
            max_total_drawdown_pct: chromo.max_total_drawdown_pct,
            risk_per_trade_pct: chromo.risk_per_trade_pct,
            
            trend_filter_enabled: chromo.trend_filter_enabled,
            min_trend_strength: chromo.min_trend_strength,
            chop_filter_enabled: chromo.chop_filter_enabled,
            chop_threshold: chromo.chop_threshold,
            
            use_limit_entry: chromo.use_limit_entry,
            limit_entry_offset_bps: chromo.limit_entry_offset_bps,
            max_slippage_bps: chromo.max_slippage_bps,
            limit_order_timeout_ms: chromo.limit_order_timeout_ms,
            
            taker_fee_rate: chromo.taker_fee_rate,
            maker_fee_rate: chromo.maker_fee_rate,
            
            // Default to CoLocated for CEX trading
            fill_model: FillModelType::CoLocated,
        }
    }
}

impl Default for MomentumParams {
    fn default() -> Self {
        Self {
            // Entry signals - balanced defaults
            momentum_threshold_pct: 0.003,      // 0.3% move required
            momentum_lookback_ms: 10_000,       // 10 second momentum window
            volume_spike_threshold: 2.5,        // 2.5x average volume
            volume_lookback_ms: 60_000,         // 1 minute volume window
            imbalance_threshold: 0.3,           // 65%/35% bid/ask imbalance
            min_confirmations: 2,               // Need 2 of 3 signals
            require_breakout: false,            // Don't require breakout
            breakout_lookback_ms: 60_000,       // 1 minute for breakout detection
            
            // Position sizing - conservative
            position_size_pct: 0.02,            // 2% per trade
            max_position_pct: 0.10,             // 10% max position
            signal_strength_scaling: 0.5,       // Half-linear scaling
            kelly_fraction: 0.25,               // Quarter Kelly
            volatility_position_scaling: 0.5,   // Scale down 50% at 2x vol
            
            // Scale-in - conservative
            scale_in_enabled: true,
            scale_in_profit_threshold_pct: 0.01, // 1% profit before adding
            max_scale_ins: 2,                    // Max 2 additions
            scale_in_size_multiplier: 0.5,       // Half size each addition
            
            // Exit signals - balanced risk/reward
            take_profit_pct: 0.015,             // 1.5% take profit
            stop_loss_pct: 0.008,               // 0.8% stop loss (1.875 R:R)
            trailing_stop_enabled: true,
            trailing_stop_pct: 0.005,           // 0.5% trailing stop
            trailing_activation_pct: 0.01,      // Activate at 1% profit
            momentum_exit_threshold_pct: -0.002, // Exit on -0.2% momentum reversal
            max_hold_time_ms: 300_000,          // 5 minute max hold
            min_hold_time_ms: 5_000,            // 5 second min hold
            exit_on_imbalance_reversal: true,   // Exit on imbalance flip
            
            // Volatility adaptation
            volatility_window_ms: 60_000,       // 1 minute vol window
            volatility_cap: 0.03,               // Stop at 3% volatility
            volatility_floor: 0.0003,           // Stop below 0.03% (no opportunity)
            high_vol_stop_multiplier: 1.5,      // 1.5x wider stops in high vol
            high_vol_tp_multiplier: 0.8,        // 20% tighter TP in high vol
            
            // Risk management
            max_daily_trades: 50,               // Max 50 trades/day
            max_hourly_trades: 10,              // Max 10 trades/hour
            cooldown_after_loss_ms: 30_000,     // 30 sec cooldown after loss
            consecutive_loss_limit: 3,          // Extended cooldown after 3 losses
            extended_cooldown_ms: 300_000,      // 5 minute extended cooldown
            max_daily_drawdown_pct: 0.05,       // 5% daily max drawdown
            max_total_drawdown_pct: 0.15,       // 15% total max drawdown
            risk_per_trade_pct: 0.01,           // 1% risk per trade
            
            // Market regime
            trend_filter_enabled: false,
            min_trend_strength: 0.3,
            chop_filter_enabled: false,
            chop_threshold: 0.3,
            
            // Order execution
            use_limit_entry: false,             // Market orders for momentum
            limit_entry_offset_bps: 5.0,
            max_slippage_bps: 20.0,
            limit_order_timeout_ms: 1_000,
            
            // Fees (Kraken default)
            taker_fee_rate: 0.0016,
            maker_fee_rate: 0.0016,
            
            fill_model: FillModelType::CoLocated,
        }
    }
}

// ============================================================================
// SIGNAL DETECTION
// ============================================================================

/// Rolling window for price/volume statistics
#[derive(Debug, Clone)]
pub struct RollingWindow {
    prices: VecDeque<(i64, f64)>,
    volumes: VecDeque<(i64, f64)>,
    bid_volume: VecDeque<(i64, f64)>,
    ask_volume: VecDeque<(i64, f64)>,
    window_ms: i64,
}

impl RollingWindow {
    pub fn new(window_ms: i64) -> Self {
        Self {
            prices: VecDeque::with_capacity(10000),
            volumes: VecDeque::with_capacity(10000),
            bid_volume: VecDeque::with_capacity(10000),
            ask_volume: VecDeque::with_capacity(10000),
            window_ms,
        }
    }
    
    pub fn update(&mut self, timestamp_ms: i64, price: f64, volume: f64, side: Option<Side>) {
        // Expire old entries
        let cutoff = timestamp_ms - self.window_ms;
        while self.prices.front().map_or(false, |(t, _)| *t < cutoff) {
            self.prices.pop_front();
        }
        while self.volumes.front().map_or(false, |(t, _)| *t < cutoff) {
            self.volumes.pop_front();
        }
        while self.bid_volume.front().map_or(false, |(t, _)| *t < cutoff) {
            self.bid_volume.pop_front();
        }
        while self.ask_volume.front().map_or(false, |(t, _)| *t < cutoff) {
            self.ask_volume.pop_front();
        }
        
        // Add new entries
        if price > 0.0 {
            self.prices.push_back((timestamp_ms, price));
        }
        if volume > 0.0 {
            self.volumes.push_back((timestamp_ms, volume));
            match side {
                Some(Side::Buy) => self.bid_volume.push_back((timestamp_ms, volume)),
                Some(Side::Sell) => self.ask_volume.push_back((timestamp_ms, volume)),
                None => {}
            }
        }
    }
    
    /// Calculate momentum (% change from window start to now)
    pub fn momentum(&self) -> f64 {
        if self.prices.len() < 2 {
            return 0.0;
        }
        let first = self.prices.front().unwrap().1;
        let last = self.prices.back().unwrap().1;
        (last - first) / first
    }
    
    /// Calculate price high within window
    pub fn high(&self) -> f64 {
        self.prices.iter().map(|(_, p)| *p).fold(f64::MIN, f64::max)
    }
    
    /// Calculate price low within window
    pub fn low(&self) -> f64 {
        self.prices.iter().map(|(_, p)| *p).fold(f64::MAX, f64::min)
    }
    
    /// Calculate average volume
    pub fn avg_volume(&self) -> f64 {
        if self.volumes.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.volumes.iter().map(|(_, v)| *v).sum();
        sum / self.volumes.len() as f64
    }
    
    /// Calculate recent volume (last 10% of window)
    pub fn recent_volume(&self) -> f64 {
        if self.volumes.is_empty() {
            return 0.0;
        }
        let cutoff = self.volumes.back().unwrap().0 - (self.window_ms / 10);
        self.volumes.iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, v)| *v)
            .sum()
    }
    
    /// Calculate orderbook imbalance (-1 = all asks, +1 = all bids)
    pub fn imbalance(&self) -> f64 {
        let bid_sum: f64 = self.bid_volume.iter().map(|(_, v)| *v).sum();
        let ask_sum: f64 = self.ask_volume.iter().map(|(_, v)| *v).sum();
        let total = bid_sum + ask_sum;
        if total < 0.001 {
            return 0.0;
        }
        (bid_sum - ask_sum) / total
    }
    
    /// Calculate volatility (standard deviation of returns)
    pub fn volatility(&self) -> f64 {
        if self.prices.len() < 10 {
            return 0.0;
        }
        
        // Convert to vec for windows()
        let price_vec: Vec<(i64, f64)> = self.prices.iter().cloned().collect();
        let returns: Vec<f64> = price_vec.windows(2)
            .map(|w| (w[1].1 / w[0].1) - 1.0)
            .collect();
        
        if returns.is_empty() {
            return 0.0;
        }
        
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
        variance.sqrt()
    }
    
    /// Last known price
    pub fn last_price(&self) -> Option<f64> {
        self.prices.back().map(|(_, p)| *p)
    }
}

/// Momentum signal output
#[derive(Debug, Clone, Default)]
pub struct MomentumSignal {
    pub momentum: f64,          // Price momentum (% change)
    pub volume_spike: f64,      // Volume relative to average (1.0 = normal)
    pub imbalance: f64,         // Orderbook imbalance (-1 to +1)
    pub volatility: f64,        // Current volatility
    pub is_breakout_high: bool, // Price broke recent high
    pub is_breakout_low: bool,  // Price broke recent low
    pub signal_strength: f64,   // Combined signal strength (0.0 to 1.0)
    pub direction: i32,         // +1 = long signal, -1 = short signal, 0 = no signal
}

impl MomentumSignal {
    /// Generate trading signal based on parameters
    pub fn generate(
        window: &RollingWindow,
        breakout_window: &RollingWindow,
        params: &MomentumParams,
    ) -> Self {
        let momentum = window.momentum();
        let avg_vol = window.avg_volume();
        let recent_vol = window.recent_volume();
        let volume_spike = if avg_vol > 0.0 { recent_vol / avg_vol } else { 1.0 };
        let imbalance = window.imbalance();
        let volatility = window.volatility();
        
        let current_price = window.last_price().unwrap_or(0.0);
        let breakout_high = breakout_window.high();
        let breakout_low = breakout_window.low();
        let is_breakout_high = current_price > breakout_high * 0.9999; // Small tolerance
        let is_breakout_low = current_price < breakout_low * 1.0001;
        
        // Count confirmations
        let mut long_confirmations = 0u32;
        let mut short_confirmations = 0u32;
        
        // 1. Momentum confirmation
        if momentum > params.momentum_threshold_pct {
            long_confirmations += 1;
        } else if momentum < -params.momentum_threshold_pct {
            short_confirmations += 1;
        }
        
        // 2. Volume confirmation
        if volume_spike >= params.volume_spike_threshold {
            // Volume confirms the direction of momentum
            if momentum > 0.0 { long_confirmations += 1; }
            if momentum < 0.0 { short_confirmations += 1; }
        }
        
        // 3. Imbalance confirmation
        if imbalance > params.imbalance_threshold {
            long_confirmations += 1;
        } else if imbalance < -params.imbalance_threshold {
            short_confirmations += 1;
        }
        
        // 4. Breakout confirmation (if required)
        if params.require_breakout {
            if is_breakout_high { long_confirmations += 1; }
            if is_breakout_low { short_confirmations += 1; }
        }
        
        // Determine direction based on confirmations
        let (direction, confirmations) = if long_confirmations >= params.min_confirmations {
            (1, long_confirmations)
        } else if short_confirmations >= params.min_confirmations {
            (-1, short_confirmations)
        } else {
            (0, 0)
        };
        
        // Calculate signal strength (0.0 to 1.0)
        let max_possible = if params.require_breakout { 4 } else { 3 };
        let signal_strength = if direction != 0 {
            // Base strength from confirmations
            let confirmation_strength = confirmations as f64 / max_possible as f64;
            // Boost from momentum magnitude
            let momentum_boost = (momentum.abs() / params.momentum_threshold_pct - 1.0).max(0.0).min(1.0) * 0.3;
            // Boost from volume
            let volume_boost = ((volume_spike / params.volume_spike_threshold - 1.0).max(0.0).min(1.0)) * 0.2;
            (confirmation_strength + momentum_boost + volume_boost).min(1.0)
        } else {
            0.0
        };
        
        Self {
            momentum,
            volume_spike,
            imbalance,
            volatility,
            is_breakout_high,
            is_breakout_low,
            signal_strength,
            direction,
        }
    }
}

// ============================================================================
// POSITION TRACKING
// ============================================================================

/// Tracks an open position
#[derive(Debug, Clone)]
pub struct Position {
    pub direction: i32,         // +1 = long, -1 = short
    pub entry_price: f64,
    pub entry_time_ms: i64,
    pub quantity: f64,
    pub peak_price: f64,        // For trailing stop
    pub num_scale_ins: u32,     // Number of additions
    pub total_cost: f64,        // Total capital allocated
}

impl Position {
    pub fn new(direction: i32, price: f64, quantity: f64, time_ms: i64) -> Self {
        Self {
            direction,
            entry_price: price,
            entry_time_ms: time_ms,
            quantity,
            peak_price: price,
            num_scale_ins: 0,
            total_cost: price * quantity,
        }
    }
    
    /// Calculate unrealized PnL as percentage
    pub fn unrealized_pnl_pct(&self, current_price: f64) -> f64 {
        if self.entry_price == 0.0 {
            return 0.0;
        }
        let pnl_pct = (current_price - self.entry_price) / self.entry_price;
        pnl_pct * self.direction as f64
    }
    
    /// Update peak price for trailing stop
    pub fn update_peak(&mut self, current_price: f64) {
        if self.direction > 0 {
            self.peak_price = self.peak_price.max(current_price);
        } else {
            self.peak_price = self.peak_price.min(current_price);
        }
    }
    
    /// Calculate trailing stop trigger price
    pub fn trailing_stop_price(&self, trailing_stop_pct: f64) -> f64 {
        if self.direction > 0 {
            self.peak_price * (1.0 - trailing_stop_pct)
        } else {
            self.peak_price * (1.0 + trailing_stop_pct)
        }
    }
    
    /// Add to position (scale-in)
    pub fn add(&mut self, price: f64, quantity: f64) {
        let old_value = self.entry_price * self.quantity;
        let new_value = price * quantity;
        self.quantity += quantity;
        self.entry_price = (old_value + new_value) / self.quantity;
        self.total_cost += new_value;
        self.num_scale_ins += 1;
    }
    
    /// Calculate position value
    pub fn value(&self, current_price: f64) -> f64 {
        self.quantity * current_price
    }
    
    /// Time in position (milliseconds)
    pub fn hold_time_ms(&self, current_time_ms: i64) -> i64 {
        current_time_ms - self.entry_time_ms
    }
}

// ============================================================================
// SIMULATION RESULT
// ============================================================================

/// Result of momentum simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MomentumSimResult {
    pub initial_capital: f64,
    pub final_capital: f64,
    pub net_pnl: f64,
    pub net_pnl_pct: f64,
    pub total_trades: u32,
    pub winning_trades: u32,
    pub losing_trades: u32,
    pub win_rate: f64,
    
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub profit_factor: f64,
    
    pub total_fees: f64,
    pub avg_trade_pnl: f64,
    pub best_trade_pnl: f64,
    pub worst_trade_pnl: f64,
    pub avg_hold_time_ms: i64,
    
    pub max_drawdown: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    
    pub total_volume: f64,
    
    // === Risk Management Metrics ===
    pub daily_trades_hit_limit: u32,
    pub hourly_trades_hit_limit: u32,
    pub consecutive_losses_hit_limit: u32,
    pub drawdown_halt_triggered: bool,
    
    // === Signal Metrics ===
    pub total_long_signals: u32,
    pub total_short_signals: u32,
    pub signals_filtered_by_regime: u32,
    pub signals_filtered_by_volatility: u32,
    
    // === Equity Curve ===
    #[serde(skip)]
    pub equity_curve: Vec<f64>,
    pub pnl_curve: Vec<f64>,
}

impl Default for MomentumSimResult {
    fn default() -> Self {
        Self {
            initial_capital: 0.0,
            final_capital: 0.0,
            net_pnl: 0.0,
            net_pnl_pct: 0.0,
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            win_rate: 0.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
            profit_factor: 0.0,
            total_fees: 0.0,
            avg_trade_pnl: 0.0,
            best_trade_pnl: f64::MIN,
            worst_trade_pnl: f64::MAX,
            avg_hold_time_ms: 0,
            max_drawdown: 0.0,
            max_drawdown_pct: 0.0,
            sharpe_ratio: 0.0,
            sortino_ratio: 0.0,
            calmar_ratio: 0.0,
            total_volume: 0.0,
            daily_trades_hit_limit: 0,
            hourly_trades_hit_limit: 0,
            consecutive_losses_hit_limit: 0,
            drawdown_halt_triggered: false,
            total_long_signals: 0,
            total_short_signals: 0,
            signals_filtered_by_regime: 0,
            signals_filtered_by_volatility: 0,
            equity_curve: Vec::new(),
            pnl_curve: Vec::new(),
        }
    }
}

// ============================================================================
// MOMENTUM SIMULATOR
// ============================================================================

/// Momentum strategy simulator
pub struct MomentumSimulator {
    initial_capital: f64,
}

impl MomentumSimulator {
    pub fn new(initial_capital: f64) -> Self {
        Self { initial_capital }
    }
    
    /// Run momentum simulation on order book events
    pub fn run(&self, events: &[OrderBookEvent], params: &MomentumParams) -> MomentumSimResult {
        let mut result = MomentumSimResult {
            initial_capital: self.initial_capital,
            ..Default::default()
        };
        
        if events.is_empty() {
            return result;
        }
        
        // State tracking
        let mut capital = self.initial_capital;
        let mut position: Option<Position> = None;
        let mut momentum_window = RollingWindow::new(params.momentum_lookback_ms);
        let mut volume_window = RollingWindow::new(params.volume_lookback_ms);
        let mut breakout_window = RollingWindow::new(params.breakout_lookback_ms);
        let mut volatility_window = RollingWindow::new(params.volatility_window_ms);
        
        // Risk management state
        let mut peak_capital = capital;
        let mut daily_start_capital = capital;
        let mut daily_trades = 0u32;
        let mut hourly_trades = 0u32;
        let mut consecutive_losses = 0u32;
        let mut last_loss_time_ms = 0i64;
        let mut last_hour_start_ms = 0i64;
        let mut last_day_start_ms = 0i64;
        let mut in_extended_cooldown = false;
        let mut cooldown_end_ms = 0i64;
        
        // Trade tracking
        let mut trade_pnls: Vec<f64> = Vec::new();
        let mut total_hold_time_ms = 0i64;
        
        let mut current_price = 0.0;
        
        for event in events {
            // Update price from trades
            if event.event_type == EventType::Trade && event.price > 0.0 {
                current_price = event.price;
            }
            
            // Update windows
            let volume = if event.event_type == EventType::Trade { event.quantity } else { 0.0 };
            momentum_window.update(event.timestamp_ms, current_price, volume, Some(event.side));
            volume_window.update(event.timestamp_ms, current_price, volume, Some(event.side));
            breakout_window.update(event.timestamp_ms, current_price, volume, Some(event.side));
            volatility_window.update(event.timestamp_ms, current_price, volume, Some(event.side));
            
            // Reset hourly/daily counters
            if event.timestamp_ms - last_hour_start_ms >= 3_600_000 {
                hourly_trades = 0;
                last_hour_start_ms = event.timestamp_ms;
            }
            if event.timestamp_ms - last_day_start_ms >= 86_400_000 {
                daily_trades = 0;
                daily_start_capital = capital;
                last_day_start_ms = event.timestamp_ms;
            }
            
            // Skip if in cooldown
            if event.timestamp_ms < cooldown_end_ms {
                continue;
            }
            in_extended_cooldown = false;
            
            // Check max drawdown halt
            let current_dd_pct = (peak_capital - capital) / peak_capital;
            if current_dd_pct > params.max_total_drawdown_pct {
                result.drawdown_halt_triggered = true;
                break;
            }
            
            // Check daily drawdown halt
            let daily_dd_pct = (daily_start_capital - capital) / daily_start_capital;
            if daily_dd_pct > params.max_daily_drawdown_pct {
                // Skip rest of day
                cooldown_end_ms = last_day_start_ms + 86_400_000;
                continue;
            }
            
            // Generate signal
            let signal = MomentumSignal::generate(&momentum_window, &breakout_window, params);
            
            // Track signals
            if signal.direction > 0 {
                result.total_long_signals += 1;
            } else if signal.direction < 0 {
                result.total_short_signals += 1;
            }
            
            // Volatility filter
            if signal.direction != 0 {
                if signal.volatility > params.volatility_cap || signal.volatility < params.volatility_floor {
                    result.signals_filtered_by_volatility += 1;
                    continue;
                }
            }
            
            // Market regime filter
            if params.trend_filter_enabled && signal.direction != 0 {
                let trend_strength = signal.imbalance.abs();
                if trend_strength < params.min_trend_strength {
                    result.signals_filtered_by_regime += 1;
                    continue;
                }
            }
            
            // Check rate limits
            if params.max_daily_trades > 0 && daily_trades >= params.max_daily_trades {
                result.daily_trades_hit_limit += 1;
                continue;
            }
            if params.max_hourly_trades > 0 && hourly_trades >= params.max_hourly_trades {
                result.hourly_trades_hit_limit += 1;
                continue;
            }
            
            // === POSITION MANAGEMENT ===
            if let Some(ref mut pos) = position {
                pos.update_peak(current_price);
                
                let hold_time = pos.hold_time_ms(event.timestamp_ms);
                let unrealized_pnl_pct = pos.unrealized_pnl_pct(current_price);
                
                let mut should_exit = false;
                
                // 1. Minimum hold time
                if hold_time < params.min_hold_time_ms {
                    // Don't exit yet
                }
                // 2. Stop loss
                else if unrealized_pnl_pct < -params.stop_loss_pct {
                    should_exit = true;
                }
                // 3. Take profit
                else if unrealized_pnl_pct > params.take_profit_pct {
                    should_exit = true;
                }
                // 4. Trailing stop (after activation)
                else if params.trailing_stop_enabled && unrealized_pnl_pct > params.trailing_activation_pct {
                    let trailing_trigger = pos.trailing_stop_price(params.trailing_stop_pct);
                    if (pos.direction > 0 && current_price < trailing_trigger) ||
                       (pos.direction < 0 && current_price > trailing_trigger) {
                        should_exit = true;
                    }
                }
                // 5. Max hold time
                else if params.max_hold_time_ms > 0 && hold_time > params.max_hold_time_ms {
                    should_exit = true;
                }
                // 6. Momentum reversal
                else if pos.direction > 0 && signal.momentum < params.momentum_exit_threshold_pct {
                    should_exit = true;
                }
                else if pos.direction < 0 && signal.momentum > -params.momentum_exit_threshold_pct {
                    should_exit = true;
                }
                // 7. Imbalance reversal
                else if params.exit_on_imbalance_reversal {
                    if (pos.direction > 0 && signal.imbalance < -params.imbalance_threshold) ||
                       (pos.direction < 0 && signal.imbalance > params.imbalance_threshold) {
                        should_exit = true;
                    }
                }
                
                // Execute exit
                if should_exit {
                    let exit_value = pos.value(current_price);
                    let fee = exit_value * params.taker_fee_rate;
                    let pnl = (current_price - pos.entry_price) * pos.quantity * pos.direction as f64 - fee;
                    
                    capital += exit_value - fee;
                    result.total_fees += fee;
                    result.total_volume += exit_value;
                    result.total_trades += 1;
                    daily_trades += 1;
                    hourly_trades += 1;
                    
                    trade_pnls.push(pnl);
                    total_hold_time_ms += hold_time;
                    
                    if pnl > 0.0 {
                        result.winning_trades += 1;
                        result.gross_profit += pnl;
                        consecutive_losses = 0;
                    } else {
                        result.losing_trades += 1;
                        result.gross_loss += pnl.abs();
                        consecutive_losses += 1;
                        last_loss_time_ms = event.timestamp_ms;
                        
                        // Apply cooldown
                        if consecutive_losses >= params.consecutive_loss_limit {
                            cooldown_end_ms = event.timestamp_ms + params.extended_cooldown_ms;
                            in_extended_cooldown = true;
                            result.consecutive_losses_hit_limit += 1;
                        } else {
                            cooldown_end_ms = event.timestamp_ms + params.cooldown_after_loss_ms;
                        }
                    }
                    
                    if pnl > result.best_trade_pnl { result.best_trade_pnl = pnl; }
                    if pnl < result.worst_trade_pnl { result.worst_trade_pnl = pnl; }
                    
                    position = None;
                }
                // Scale-in check
                else if params.scale_in_enabled && 
                        pos.num_scale_ins < params.max_scale_ins &&
                        unrealized_pnl_pct > params.scale_in_profit_threshold_pct &&
                        signal.direction == pos.direction {
                    
                    let scale_size = pos.quantity * params.scale_in_size_multiplier;
                    let scale_cost = scale_size * current_price;
                    let scale_fee = scale_cost * params.taker_fee_rate;
                    
                    if scale_cost + scale_fee < capital * params.max_position_pct {
                        capital -= scale_cost + scale_fee;
                        pos.add(current_price, scale_size);
                        result.total_fees += scale_fee;
                        result.total_volume += scale_cost;
                    }
                }
            }
            // === ENTRY ===
            else if signal.direction != 0 {
                // Calculate position size
                let base_size_value = capital * params.position_size_pct;
                
                // Signal strength scaling
                let signal_adjusted = base_size_value * (1.0 + (signal.signal_strength - 0.5) * params.signal_strength_scaling * 2.0);
                
                // Volatility scaling (inverse)
                let normal_vol = 0.001; // 0.1% as baseline
                let vol_adjustment = if params.volatility_position_scaling > 0.0 && signal.volatility > 0.0 {
                    let vol_ratio = signal.volatility / normal_vol;
                    1.0 / (1.0 + (vol_ratio - 1.0) * params.volatility_position_scaling)
                } else {
                    1.0
                };
                
                let final_size_value = (signal_adjusted * vol_adjustment)
                    .min(capital * params.max_position_pct)
                    .max(0.0);
                
                if final_size_value > 0.0 && current_price > 0.0 {
                    let quantity = final_size_value / current_price;
                    let entry_fee = final_size_value * params.taker_fee_rate;
                    
                    capital -= final_size_value + entry_fee;
                    result.total_fees += entry_fee;
                    result.total_volume += final_size_value;
                    
                    position = Some(Position::new(signal.direction, current_price, quantity, event.timestamp_ms));
                }
            }
            
            // Update peak capital
            let equity = capital + position.as_ref().map_or(0.0, |p| p.value(current_price));
            peak_capital = peak_capital.max(equity);
            result.equity_curve.push(equity);
        }
        
        // Close any remaining position at last price
        if let Some(pos) = position {
            let exit_value = pos.value(current_price);
            let fee = exit_value * params.taker_fee_rate;
            let pnl = (current_price - pos.entry_price) * pos.quantity * pos.direction as f64 - fee;
            
            capital += exit_value - fee;
            result.total_fees += fee;
            result.total_volume += exit_value;
            result.total_trades += 1;
            trade_pnls.push(pnl);
            total_hold_time_ms += pos.hold_time_ms(events.last().unwrap().timestamp_ms);
            
            if pnl > 0.0 {
                result.winning_trades += 1;
                result.gross_profit += pnl;
            } else {
                result.losing_trades += 1;
                result.gross_loss += pnl.abs();
            }
            
            if pnl > result.best_trade_pnl { result.best_trade_pnl = pnl; }
            if pnl < result.worst_trade_pnl { result.worst_trade_pnl = pnl; }
        }
        
        // Calculate final metrics
        result.final_capital = capital;
        result.net_pnl = capital - self.initial_capital;
        result.net_pnl_pct = result.net_pnl / self.initial_capital;
        
        if result.total_trades > 0 {
            result.win_rate = result.winning_trades as f64 / result.total_trades as f64;
            result.avg_trade_pnl = result.net_pnl / result.total_trades as f64;
            result.avg_hold_time_ms = total_hold_time_ms / result.total_trades as i64;
        }
        
        if result.gross_loss > 0.0 {
            result.profit_factor = result.gross_profit / result.gross_loss;
        } else if result.gross_profit > 0.0 {
            result.profit_factor = f64::INFINITY;
        }
        
        // Max drawdown
        let mut peak = self.initial_capital;
        for &equity in &result.equity_curve {
            peak = peak.max(equity);
            let dd = peak - equity;
            if dd > result.max_drawdown {
                result.max_drawdown = dd;
                result.max_drawdown_pct = dd / peak;
            }
        }
        
        // Sharpe ratio (annualized)
        if trade_pnls.len() > 1 {
            let mean_return = trade_pnls.iter().sum::<f64>() / trade_pnls.len() as f64;
            let variance = trade_pnls.iter()
                .map(|r| (r - mean_return).powi(2))
                .sum::<f64>() / trade_pnls.len() as f64;
            let std_dev = variance.sqrt();
            
            if std_dev > 0.0 {
                // Annualize assuming ~250 trading days, ~10 trades per day
                let trades_per_year: f64 = 2500.0;
                result.sharpe_ratio = (mean_return / std_dev) * trades_per_year.sqrt();
            }
            
            // Sortino (downside deviation only)
            let downside_returns: Vec<f64> = trade_pnls.iter()
                .filter(|&&r| r < 0.0)
                .cloned()
                .collect();
            if !downside_returns.is_empty() {
                let downside_variance = downside_returns.iter()
                    .map(|r| r.powi(2))
                    .sum::<f64>() / downside_returns.len() as f64;
                let downside_dev = downside_variance.sqrt();
                if downside_dev > 0.0 {
                    result.sortino_ratio = (mean_return / downside_dev) * 2500.0_f64.sqrt();
                }
            }
        }
        
        // Calmar ratio
        if result.max_drawdown_pct > 0.0 {
            result.calmar_ratio = result.net_pnl_pct / result.max_drawdown_pct;
        }
        
        // Fix unset best/worst
        if result.best_trade_pnl == f64::MIN { result.best_trade_pnl = 0.0; }
        if result.worst_trade_pnl == f64::MAX { result.worst_trade_pnl = 0.0; }
        
        // PnL curve
        result.pnl_curve = result.equity_curve.iter()
            .map(|e| e - self.initial_capital)
            .collect();
        
        result
    }
    
    /// Run simulation from a MomentumChromosome
    pub fn run_chromosome(&self, events: &[OrderBookEvent], chromo: &MomentumChromosome) -> MomentumSimResult {
        let params = MomentumParams::from(chromo);
        self.run(events, &params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_events() -> Vec<OrderBookEvent> {
        let mut events = Vec::new();
        let mut price = 100.0;
        let mut ts = 0i64;
        
        // Create a trending market with some noise
        for i in 0..1000 {
            // Trend upward with noise
            let trend = 0.001;
            let noise = ((i as f64 * 0.1).sin() * 0.0005);
            price = price * (1.0 + trend + noise);
            
            events.push(OrderBookEvent {
                timestamp_ms: ts,
                event_type: EventType::Trade,
                order_id: format!("trade_{}", i),
                side: if i % 3 == 0 { Side::Sell } else { Side::Buy },
                price,
                quantity: 1.0 + (i % 10) as f64,
            });
            
            ts += 100; // 100ms between trades
        }
        
        events
    }
    
    #[test]
    fn test_momentum_signal_generation() {
        let params = MomentumParams::default();
        let mut window = RollingWindow::new(params.momentum_lookback_ms);
        let mut breakout_window = RollingWindow::new(params.breakout_lookback_ms);
        
        // Add upward trending prices
        for i in 0..100 {
            let price = 100.0 * (1.0 + i as f64 * 0.001);
            window.update(i * 100, price, 10.0, Some(Side::Buy));
            breakout_window.update(i * 100, price, 10.0, Some(Side::Buy));
        }
        
        let signal = MomentumSignal::generate(&window, &breakout_window, &params);
        assert!(signal.momentum > 0.0, "Should detect positive momentum");
        assert!(signal.imbalance > 0.0, "Should detect buy imbalance");
    }
    
    #[test]
    fn test_momentum_simulation_basic() {
        let events = create_test_events();
        let simulator = MomentumSimulator::new(10000.0);
        let params = MomentumParams::default();
        
        let result = simulator.run(&events, &params);
        
        assert_eq!(result.initial_capital, 10000.0);
        assert!(result.total_trades > 0, "Should execute some trades");
        println!("Result: {:?}", result);
    }
    
    #[test]
    fn test_position_pnl_calculation() {
        let pos = Position::new(1, 100.0, 10.0, 0);
        
        // Long position: +10% price move = +10% PnL
        assert!((pos.unrealized_pnl_pct(110.0) - 0.10).abs() < 0.001);
        
        // Long position: -5% price move = -5% PnL  
        assert!((pos.unrealized_pnl_pct(95.0) - (-0.05)).abs() < 0.001);
        
        // Short position
        let short = Position::new(-1, 100.0, 10.0, 0);
        
        // Short position: -10% price move = +10% PnL
        assert!((short.unrealized_pnl_pct(90.0) - 0.10).abs() < 0.001);
        
        // Short position: +5% price move = -5% PnL
        assert!((short.unrealized_pnl_pct(105.0) - (-0.05)).abs() < 0.001);
    }
}
