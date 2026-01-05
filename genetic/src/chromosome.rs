//! Chromosome definitions for genetic optimization of strategy parameters

use rand::Rng;
use serde::{Serialize, Deserialize};
use crate::Chromosome;

// =============================================================================
// MOMENTUM STRATEGY CHROMOSOME
// =============================================================================

/// Chromosome for momentum/trend-following strategy optimization
/// 
/// Unlike market making (which provides liquidity and suffers adverse selection),
/// momentum strategies CONSUME liquidity and profit from price trends.
/// 
/// Key insight: You're the "informed trader" using orderbook imbalance and 
/// momentum signals to predict short-term price direction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MomentumChromosome {
    // === ENTRY SIGNALS ===
    
    /// Minimum price momentum (% change) to trigger entry
    /// Range: 0.05% - 3.0% (0.0005 - 0.03)
    /// Lower = more signals but more noise; Higher = fewer but stronger signals
    pub momentum_threshold_pct: f64,
    
    /// Lookback window for measuring momentum (milliseconds)
    /// Range: 500ms - 120,000ms (2 minutes)
    /// Short = reactive to micro-moves; Long = captures larger trends
    pub momentum_lookback_ms: i64,
    
    /// Minimum volume spike to confirm entry (multiple of average)
    /// Range: 1.0 - 8.0
    /// 1.0 = no volume filter; 3.0 = need 3x avg volume to enter
    pub volume_spike_threshold: f64,
    
    /// Volume averaging window for spike detection (milliseconds)
    /// Range: 10,000ms - 300,000ms (5 minutes)
    pub volume_lookback_ms: i64,
    
    /// Orderbook imbalance threshold to confirm entry
    /// Range: 0.0 - 0.8
    /// 0.0 = ignore imbalance; 0.5 = need 75%/25% bid/ask split
    pub imbalance_threshold: f64,
    
    /// Number of confirmations required before entry (momentum + volume + imbalance)
    /// Range: 1 - 3
    /// 1 = any signal triggers; 3 = all signals must align
    pub min_confirmations: u32,
    
    /// Require price to break recent high/low before entry
    /// Helps avoid false breakouts
    pub require_breakout: bool,
    
    /// Lookback for breakout detection (milliseconds)
    /// Range: 5,000ms - 600,000ms (10 minutes)
    pub breakout_lookback_ms: i64,
    
    // === POSITION SIZING ===
    
    /// Base position size as % of capital per trade
    /// Range: 0.5% - 15% (0.005 - 0.15)
    pub position_size_pct: f64,
    
    /// Maximum total position as % of capital
    /// Range: 5% - 100% (0.05 - 1.0)
    pub max_position_pct: f64,
    
    /// Scale position size by signal strength
    /// 0.0 = fixed size; 1.0 = linear with signal; 2.0 = aggressive scaling
    pub signal_strength_scaling: f64,
    
    /// Kelly fraction for position sizing (0.0 = ignore Kelly, 1.0 = full Kelly)
    /// Range: 0.0 - 0.5 (half-Kelly is common for risk management)
    pub kelly_fraction: f64,
    
    /// Scale position by volatility (inverse relationship)
    /// 0.0 = ignore vol; 1.0 = half size at 2x normal vol
    pub volatility_position_scaling: f64,
    
    // === SCALE-IN PARAMETERS ===
    
    /// Enable adding to winning positions
    pub scale_in_enabled: bool,
    
    /// Profit threshold (%) before adding to position
    /// Range: 0.2% - 3.0%
    pub scale_in_profit_threshold_pct: f64,
    
    /// Maximum number of scale-in additions
    /// Range: 1 - 5
    pub max_scale_ins: u32,
    
    /// Size multiplier for each scale-in (1.0 = same size, 0.5 = half size)
    /// Range: 0.25 - 1.5
    pub scale_in_size_multiplier: f64,
    
    // === EXIT SIGNALS ===
    
    /// Take profit target (%)
    /// Range: 0.2% - 10.0%
    pub take_profit_pct: f64,
    
    /// Stop loss limit (%)
    /// Range: 0.1% - 5.0%
    pub stop_loss_pct: f64,
    
    /// Enable trailing stop
    pub trailing_stop_enabled: bool,
    
    /// Trailing stop distance (% from peak)
    /// Range: 0.1% - 3.0%
    pub trailing_stop_pct: f64,
    
    /// Trailing stop activation threshold (% profit before trailing starts)
    /// Range: 0.1% - 2.0%
    pub trailing_activation_pct: f64,
    
    /// Exit when momentum reverses (goes negative relative to position)
    /// Range: -2.0% to 0.0% (negative = wait for reversal)
    pub momentum_exit_threshold_pct: f64,
    
    /// Maximum time to hold position (milliseconds)
    /// Range: 10,000ms - 3,600,000ms (1 hour)
    /// 0 = no time limit
    pub max_hold_time_ms: i64,
    
    /// Minimum time to hold (prevents overtrading)
    /// Range: 0ms - 60,000ms
    pub min_hold_time_ms: i64,
    
    /// Exit on imbalance reversal (imbalance flips against position)
    pub exit_on_imbalance_reversal: bool,
    
    // === VOLATILITY ADAPTATION ===
    
    /// Volatility measurement window (milliseconds)
    /// Range: 5,000ms - 300,000ms
    pub volatility_window_ms: i64,
    
    /// Volatility cap (stop trading above this level)
    /// Range: 0.5% - 10.0% (annualized)
    pub volatility_cap: f64,
    
    /// Volatility floor (stop trading below this level - no opportunity)
    /// Range: 0.01% - 0.5%
    pub volatility_floor: f64,
    
    /// Widen stops in high volatility (multiplier applied to stop_loss_pct)
    /// Range: 1.0 - 3.0
    pub high_vol_stop_multiplier: f64,
    
    /// Tighten take profit in high volatility (multiplier)
    /// Range: 0.5 - 1.0
    pub high_vol_tp_multiplier: f64,
    
    // === RISK MANAGEMENT ===
    
    /// Maximum trades per day (0 = unlimited)
    /// Range: 0 - 200
    pub max_daily_trades: u32,
    
    /// Maximum trades per hour
    /// Range: 0 - 50
    pub max_hourly_trades: u32,
    
    /// Cooldown after a losing trade (milliseconds)
    /// Range: 0ms - 300,000ms (5 minutes)
    pub cooldown_after_loss_ms: i64,
    
    /// Consecutive losses before extended cooldown
    /// Range: 1 - 10
    pub consecutive_loss_limit: u32,
    
    /// Extended cooldown after hitting loss limit (milliseconds)
    /// Range: 60,000ms - 1,800,000ms (30 minutes)
    pub extended_cooldown_ms: i64,
    
    /// Maximum daily drawdown before stopping (%)
    /// Range: 1% - 20%
    pub max_daily_drawdown_pct: f64,
    
    /// Maximum total drawdown before stopping (%)
    /// Range: 5% - 30%
    pub max_total_drawdown_pct: f64,
    
    /// Risk per trade (% of capital at risk considering stop loss)
    /// Range: 0.25% - 3.0%
    pub risk_per_trade_pct: f64,
    
    // === MARKET REGIME FILTERS ===
    
    /// Only trade in trending markets (imbalance > threshold)
    pub trend_filter_enabled: bool,
    
    /// Trend strength required to trade
    /// Range: 0.1 - 0.6
    pub min_trend_strength: f64,
    
    /// Avoid trading during choppy/ranging markets
    /// Uses volatility-of-volatility as chop indicator
    pub chop_filter_enabled: bool,
    
    /// Chop threshold (volatility-of-volatility)
    /// Range: 0.1 - 0.5
    pub chop_threshold: f64,
    
    // === ORDER EXECUTION ===
    
    /// Use limit orders for entry (vs market orders)
    /// Limit = better fills but may miss fast moves
    pub use_limit_entry: bool,
    
    /// Offset for limit entry orders (bps from mid)
    /// Range: 0 - 20 bps
    /// Positive = more aggressive (closer to market), Negative = passive
    pub limit_entry_offset_bps: f64,
    
    /// Maximum slippage tolerance before cancelling (bps)
    /// Range: 5 - 100 bps
    pub max_slippage_bps: f64,
    
    /// Time to wait for limit order fill before converting to market (ms)
    /// Range: 100ms - 10,000ms
    pub limit_order_timeout_ms: i64,
    
    // === FEE CONFIGURATION ===
    
    /// Taker fee rate (for market/aggressive orders)
    /// Range: 0.0001 - 0.003 (1-30 bps)
    pub taker_fee_rate: f64,
    
    /// Maker fee rate (for limit/passive orders)
    /// Range: 0.0 - 0.002 (0-20 bps, can be negative for rebates)
    pub maker_fee_rate: f64,
}

impl MomentumChromosome {
    /// Enforce bounds on all parameters
    pub fn enforce_bounds(&mut self) {
        // Entry signals
        self.momentum_threshold_pct = self.momentum_threshold_pct.clamp(0.0005, 0.03);
        self.momentum_lookback_ms = self.momentum_lookback_ms.clamp(500, 120_000);
        self.volume_spike_threshold = self.volume_spike_threshold.clamp(1.0, 8.0);
        self.volume_lookback_ms = self.volume_lookback_ms.clamp(10_000, 300_000);
        self.imbalance_threshold = self.imbalance_threshold.clamp(0.0, 0.8);
        self.min_confirmations = self.min_confirmations.clamp(1, 3);
        self.breakout_lookback_ms = self.breakout_lookback_ms.clamp(5_000, 600_000);
        
        // Position sizing
        self.position_size_pct = self.position_size_pct.clamp(0.005, 0.15);
        self.max_position_pct = self.max_position_pct.clamp(0.05, 1.0);
        self.signal_strength_scaling = self.signal_strength_scaling.clamp(0.0, 2.0);
        self.kelly_fraction = self.kelly_fraction.clamp(0.0, 0.5);
        self.volatility_position_scaling = self.volatility_position_scaling.clamp(0.0, 1.0);
        
        // Scale-in
        self.scale_in_profit_threshold_pct = self.scale_in_profit_threshold_pct.clamp(0.002, 0.03);
        self.max_scale_ins = self.max_scale_ins.clamp(1, 5);
        self.scale_in_size_multiplier = self.scale_in_size_multiplier.clamp(0.25, 1.5);
        
        // Exit signals
        self.take_profit_pct = self.take_profit_pct.clamp(0.002, 0.10);
        self.stop_loss_pct = self.stop_loss_pct.clamp(0.001, 0.05);
        self.trailing_stop_pct = self.trailing_stop_pct.clamp(0.001, 0.03);
        self.trailing_activation_pct = self.trailing_activation_pct.clamp(0.001, 0.02);
        self.momentum_exit_threshold_pct = self.momentum_exit_threshold_pct.clamp(-0.02, 0.0);
        self.max_hold_time_ms = self.max_hold_time_ms.clamp(0, 3_600_000);
        self.min_hold_time_ms = self.min_hold_time_ms.clamp(0, 60_000);
        
        // Volatility
        self.volatility_window_ms = self.volatility_window_ms.clamp(5_000, 300_000);
        self.volatility_cap = self.volatility_cap.clamp(0.005, 0.10);
        self.volatility_floor = self.volatility_floor.clamp(0.0001, 0.005);
        self.high_vol_stop_multiplier = self.high_vol_stop_multiplier.clamp(1.0, 3.0);
        self.high_vol_tp_multiplier = self.high_vol_tp_multiplier.clamp(0.5, 1.0);
        
        // Risk management
        self.max_daily_trades = self.max_daily_trades.clamp(0, 200);
        self.max_hourly_trades = self.max_hourly_trades.clamp(0, 50);
        self.cooldown_after_loss_ms = self.cooldown_after_loss_ms.clamp(0, 300_000);
        self.consecutive_loss_limit = self.consecutive_loss_limit.clamp(1, 10);
        self.extended_cooldown_ms = self.extended_cooldown_ms.clamp(60_000, 1_800_000);
        self.max_daily_drawdown_pct = self.max_daily_drawdown_pct.clamp(0.01, 0.20);
        self.max_total_drawdown_pct = self.max_total_drawdown_pct.clamp(0.05, 0.30);
        self.risk_per_trade_pct = self.risk_per_trade_pct.clamp(0.0025, 0.03);
        
        // Market regime
        self.min_trend_strength = self.min_trend_strength.clamp(0.1, 0.6);
        self.chop_threshold = self.chop_threshold.clamp(0.1, 0.5);
        
        // Order execution
        self.limit_entry_offset_bps = self.limit_entry_offset_bps.clamp(0.0, 20.0);
        self.max_slippage_bps = self.max_slippage_bps.clamp(5.0, 100.0);
        self.limit_order_timeout_ms = self.limit_order_timeout_ms.clamp(100, 10_000);
        
        // Fees
        self.taker_fee_rate = self.taker_fee_rate.clamp(0.0001, 0.003);
        self.maker_fee_rate = self.maker_fee_rate.clamp(-0.0002, 0.002);
    }
    
    /// Calculate expected risk/reward ratio based on parameters
    pub fn expected_risk_reward(&self) -> f64 {
        self.take_profit_pct / self.stop_loss_pct
    }
    
    /// Calculate required win rate to break even (accounting for fees)
    pub fn breakeven_win_rate(&self) -> f64 {
        let r = self.expected_risk_reward();
        let fee_cost = self.taker_fee_rate * 2.0; // Entry + exit fees
        // Adjusted for fees: P(win) * TP - P(lose) * SL - fees = 0
        // P(win) * TP - (1 - P(win)) * SL - fees = 0
        // P(win) = (SL + fees) / (TP + SL)
        (self.stop_loss_pct + fee_cost) / (self.take_profit_pct + self.stop_loss_pct)
    }
}

impl Chromosome for MomentumChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            // Entry signals
            momentum_threshold_pct: rng.gen_range(0.0005..0.03),
            momentum_lookback_ms: rng.gen_range(500..120_000),
            volume_spike_threshold: rng.gen_range(1.0..8.0),
            volume_lookback_ms: rng.gen_range(10_000..300_000),
            imbalance_threshold: rng.gen_range(0.0..0.8),
            min_confirmations: rng.gen_range(1..=3),
            require_breakout: rng.gen_bool(0.5),
            breakout_lookback_ms: rng.gen_range(5_000..600_000),
            
            // Position sizing
            position_size_pct: rng.gen_range(0.005..0.15),
            max_position_pct: rng.gen_range(0.05..1.0),
            signal_strength_scaling: rng.gen_range(0.0..2.0),
            kelly_fraction: rng.gen_range(0.0..0.5),
            volatility_position_scaling: rng.gen_range(0.0..1.0),
            
            // Scale-in
            scale_in_enabled: rng.gen_bool(0.5),
            scale_in_profit_threshold_pct: rng.gen_range(0.002..0.03),
            max_scale_ins: rng.gen_range(1..=5),
            scale_in_size_multiplier: rng.gen_range(0.25..1.5),
            
            // Exit signals
            take_profit_pct: rng.gen_range(0.002..0.10),
            stop_loss_pct: rng.gen_range(0.001..0.05),
            trailing_stop_enabled: rng.gen_bool(0.5),
            trailing_stop_pct: rng.gen_range(0.001..0.03),
            trailing_activation_pct: rng.gen_range(0.001..0.02),
            momentum_exit_threshold_pct: rng.gen_range(-0.02..0.0),
            max_hold_time_ms: rng.gen_range(0..3_600_000),
            min_hold_time_ms: rng.gen_range(0..60_000),
            exit_on_imbalance_reversal: rng.gen_bool(0.5),
            
            // Volatility
            volatility_window_ms: rng.gen_range(5_000..300_000),
            volatility_cap: rng.gen_range(0.005..0.10),
            volatility_floor: rng.gen_range(0.0001..0.005),
            high_vol_stop_multiplier: rng.gen_range(1.0..3.0),
            high_vol_tp_multiplier: rng.gen_range(0.5..1.0),
            
            // Risk management
            max_daily_trades: rng.gen_range(0..200),
            max_hourly_trades: rng.gen_range(0..50),
            cooldown_after_loss_ms: rng.gen_range(0..300_000),
            consecutive_loss_limit: rng.gen_range(1..=10),
            extended_cooldown_ms: rng.gen_range(60_000..1_800_000),
            max_daily_drawdown_pct: rng.gen_range(0.01..0.20),
            max_total_drawdown_pct: rng.gen_range(0.05..0.30),
            risk_per_trade_pct: rng.gen_range(0.0025..0.03),
            
            // Market regime
            trend_filter_enabled: rng.gen_bool(0.5),
            min_trend_strength: rng.gen_range(0.1..0.6),
            chop_filter_enabled: rng.gen_bool(0.5),
            chop_threshold: rng.gen_range(0.1..0.5),
            
            // Order execution
            use_limit_entry: rng.gen_bool(0.5),
            limit_entry_offset_bps: rng.gen_range(0.0..20.0),
            max_slippage_bps: rng.gen_range(5.0..100.0),
            limit_order_timeout_ms: rng.gen_range(100..10_000),
            
            // Fees (typically from exchange config, but GA can optimize around them)
            taker_fee_rate: 0.0016, // Default Kraken taker
            maker_fee_rate: 0.0016, // Default Kraken maker
        }
    }

    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        let mut child = Self {
            // Entry signals
            momentum_threshold_pct: if rng.gen_bool(0.5) { self.momentum_threshold_pct } else { other.momentum_threshold_pct },
            momentum_lookback_ms: if rng.gen_bool(0.5) { self.momentum_lookback_ms } else { other.momentum_lookback_ms },
            volume_spike_threshold: if rng.gen_bool(0.5) { self.volume_spike_threshold } else { other.volume_spike_threshold },
            volume_lookback_ms: if rng.gen_bool(0.5) { self.volume_lookback_ms } else { other.volume_lookback_ms },
            imbalance_threshold: if rng.gen_bool(0.5) { self.imbalance_threshold } else { other.imbalance_threshold },
            min_confirmations: if rng.gen_bool(0.5) { self.min_confirmations } else { other.min_confirmations },
            require_breakout: if rng.gen_bool(0.5) { self.require_breakout } else { other.require_breakout },
            breakout_lookback_ms: if rng.gen_bool(0.5) { self.breakout_lookback_ms } else { other.breakout_lookback_ms },
            
            // Position sizing
            position_size_pct: if rng.gen_bool(0.5) { self.position_size_pct } else { other.position_size_pct },
            max_position_pct: if rng.gen_bool(0.5) { self.max_position_pct } else { other.max_position_pct },
            signal_strength_scaling: if rng.gen_bool(0.5) { self.signal_strength_scaling } else { other.signal_strength_scaling },
            kelly_fraction: if rng.gen_bool(0.5) { self.kelly_fraction } else { other.kelly_fraction },
            volatility_position_scaling: if rng.gen_bool(0.5) { self.volatility_position_scaling } else { other.volatility_position_scaling },
            
            // Scale-in
            scale_in_enabled: if rng.gen_bool(0.5) { self.scale_in_enabled } else { other.scale_in_enabled },
            scale_in_profit_threshold_pct: if rng.gen_bool(0.5) { self.scale_in_profit_threshold_pct } else { other.scale_in_profit_threshold_pct },
            max_scale_ins: if rng.gen_bool(0.5) { self.max_scale_ins } else { other.max_scale_ins },
            scale_in_size_multiplier: if rng.gen_bool(0.5) { self.scale_in_size_multiplier } else { other.scale_in_size_multiplier },
            
            // Exit signals
            take_profit_pct: if rng.gen_bool(0.5) { self.take_profit_pct } else { other.take_profit_pct },
            stop_loss_pct: if rng.gen_bool(0.5) { self.stop_loss_pct } else { other.stop_loss_pct },
            trailing_stop_enabled: if rng.gen_bool(0.5) { self.trailing_stop_enabled } else { other.trailing_stop_enabled },
            trailing_stop_pct: if rng.gen_bool(0.5) { self.trailing_stop_pct } else { other.trailing_stop_pct },
            trailing_activation_pct: if rng.gen_bool(0.5) { self.trailing_activation_pct } else { other.trailing_activation_pct },
            momentum_exit_threshold_pct: if rng.gen_bool(0.5) { self.momentum_exit_threshold_pct } else { other.momentum_exit_threshold_pct },
            max_hold_time_ms: if rng.gen_bool(0.5) { self.max_hold_time_ms } else { other.max_hold_time_ms },
            min_hold_time_ms: if rng.gen_bool(0.5) { self.min_hold_time_ms } else { other.min_hold_time_ms },
            exit_on_imbalance_reversal: if rng.gen_bool(0.5) { self.exit_on_imbalance_reversal } else { other.exit_on_imbalance_reversal },
            
            // Volatility
            volatility_window_ms: if rng.gen_bool(0.5) { self.volatility_window_ms } else { other.volatility_window_ms },
            volatility_cap: if rng.gen_bool(0.5) { self.volatility_cap } else { other.volatility_cap },
            volatility_floor: if rng.gen_bool(0.5) { self.volatility_floor } else { other.volatility_floor },
            high_vol_stop_multiplier: if rng.gen_bool(0.5) { self.high_vol_stop_multiplier } else { other.high_vol_stop_multiplier },
            high_vol_tp_multiplier: if rng.gen_bool(0.5) { self.high_vol_tp_multiplier } else { other.high_vol_tp_multiplier },
            
            // Risk management
            max_daily_trades: if rng.gen_bool(0.5) { self.max_daily_trades } else { other.max_daily_trades },
            max_hourly_trades: if rng.gen_bool(0.5) { self.max_hourly_trades } else { other.max_hourly_trades },
            cooldown_after_loss_ms: if rng.gen_bool(0.5) { self.cooldown_after_loss_ms } else { other.cooldown_after_loss_ms },
            consecutive_loss_limit: if rng.gen_bool(0.5) { self.consecutive_loss_limit } else { other.consecutive_loss_limit },
            extended_cooldown_ms: if rng.gen_bool(0.5) { self.extended_cooldown_ms } else { other.extended_cooldown_ms },
            max_daily_drawdown_pct: if rng.gen_bool(0.5) { self.max_daily_drawdown_pct } else { other.max_daily_drawdown_pct },
            max_total_drawdown_pct: if rng.gen_bool(0.5) { self.max_total_drawdown_pct } else { other.max_total_drawdown_pct },
            risk_per_trade_pct: if rng.gen_bool(0.5) { self.risk_per_trade_pct } else { other.risk_per_trade_pct },
            
            // Market regime
            trend_filter_enabled: if rng.gen_bool(0.5) { self.trend_filter_enabled } else { other.trend_filter_enabled },
            min_trend_strength: if rng.gen_bool(0.5) { self.min_trend_strength } else { other.min_trend_strength },
            chop_filter_enabled: if rng.gen_bool(0.5) { self.chop_filter_enabled } else { other.chop_filter_enabled },
            chop_threshold: if rng.gen_bool(0.5) { self.chop_threshold } else { other.chop_threshold },
            
            // Order execution
            use_limit_entry: if rng.gen_bool(0.5) { self.use_limit_entry } else { other.use_limit_entry },
            limit_entry_offset_bps: if rng.gen_bool(0.5) { self.limit_entry_offset_bps } else { other.limit_entry_offset_bps },
            max_slippage_bps: if rng.gen_bool(0.5) { self.max_slippage_bps } else { other.max_slippage_bps },
            limit_order_timeout_ms: if rng.gen_bool(0.5) { self.limit_order_timeout_ms } else { other.limit_order_timeout_ms },
            
            // Fees
            taker_fee_rate: if rng.gen_bool(0.5) { self.taker_fee_rate } else { other.taker_fee_rate },
            maker_fee_rate: if rng.gen_bool(0.5) { self.maker_fee_rate } else { other.maker_fee_rate },
        };
        child.enforce_bounds();
        child
    }

    fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64) {
        // Entry signals
        if rng.gen_bool(mutation_rate) {
            self.momentum_threshold_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.momentum_lookback_ms = ((self.momentum_lookback_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.volume_spike_threshold += rng.gen_range(-1.0..1.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.volume_lookback_ms = ((self.volume_lookback_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.imbalance_threshold += rng.gen_range(-0.1..0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.min_confirmations = rng.gen_range(1..=3);
        }
        if rng.gen_bool(mutation_rate) {
            self.require_breakout = !self.require_breakout;
        }
        if rng.gen_bool(mutation_rate) {
            self.breakout_lookback_ms = ((self.breakout_lookback_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        
        // Position sizing
        if rng.gen_bool(mutation_rate) {
            self.position_size_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_position_pct += rng.gen_range(-0.1..0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.signal_strength_scaling += rng.gen_range(-0.3..0.3);
        }
        if rng.gen_bool(mutation_rate) {
            self.kelly_fraction += rng.gen_range(-0.1..0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.volatility_position_scaling += rng.gen_range(-0.2..0.2);
        }
        
        // Scale-in
        if rng.gen_bool(mutation_rate) {
            self.scale_in_enabled = !self.scale_in_enabled;
        }
        if rng.gen_bool(mutation_rate) {
            self.scale_in_profit_threshold_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_scale_ins = rng.gen_range(1..=5);
        }
        if rng.gen_bool(mutation_rate) {
            self.scale_in_size_multiplier += rng.gen_range(-0.2..0.2);
        }
        
        // Exit signals
        if rng.gen_bool(mutation_rate) {
            self.take_profit_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.stop_loss_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.trailing_stop_enabled = !self.trailing_stop_enabled;
        }
        if rng.gen_bool(mutation_rate) {
            self.trailing_stop_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.trailing_activation_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.momentum_exit_threshold_pct += rng.gen_range(-0.005..0.005);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_hold_time_ms = ((self.max_hold_time_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.min_hold_time_ms = ((self.min_hold_time_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.exit_on_imbalance_reversal = !self.exit_on_imbalance_reversal;
        }
        
        // Volatility
        if rng.gen_bool(mutation_rate) {
            self.volatility_window_ms = ((self.volatility_window_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.volatility_cap *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.volatility_floor *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.high_vol_stop_multiplier += rng.gen_range(-0.3..0.3);
        }
        if rng.gen_bool(mutation_rate) {
            self.high_vol_tp_multiplier += rng.gen_range(-0.1..0.1);
        }
        
        // Risk management
        if rng.gen_bool(mutation_rate) {
            self.max_daily_trades = ((self.max_daily_trades as f64 * rng.gen_range(0.5..2.0)) as u32);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_hourly_trades = ((self.max_hourly_trades as f64 * rng.gen_range(0.5..2.0)) as u32);
        }
        if rng.gen_bool(mutation_rate) {
            self.cooldown_after_loss_ms = ((self.cooldown_after_loss_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.consecutive_loss_limit = rng.gen_range(1..=10);
        }
        if rng.gen_bool(mutation_rate) {
            self.extended_cooldown_ms = ((self.extended_cooldown_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_daily_drawdown_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_total_drawdown_pct *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.risk_per_trade_pct *= rng.gen_range(0.5..2.0);
        }
        
        // Market regime
        if rng.gen_bool(mutation_rate) {
            self.trend_filter_enabled = !self.trend_filter_enabled;
        }
        if rng.gen_bool(mutation_rate) {
            self.min_trend_strength += rng.gen_range(-0.1..0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.chop_filter_enabled = !self.chop_filter_enabled;
        }
        if rng.gen_bool(mutation_rate) {
            self.chop_threshold += rng.gen_range(-0.1..0.1);
        }
        
        // Order execution
        if rng.gen_bool(mutation_rate) {
            self.use_limit_entry = !self.use_limit_entry;
        }
        if rng.gen_bool(mutation_rate) {
            self.limit_entry_offset_bps += rng.gen_range(-3.0..3.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_slippage_bps *= rng.gen_range(0.5..2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.limit_order_timeout_ms = ((self.limit_order_timeout_ms as f64 * rng.gen_range(0.5..2.0)) as i64);
        }
        
        // Enforce bounds after mutation
        self.enforce_bounds();
    }
    
    fn distance(&self, other: &Self) -> f64 {
        // Simplified distance calculation for the most important parameters
        let d1 = (self.momentum_threshold_pct.ln() - other.momentum_threshold_pct.ln()).abs() / 4.0;
        let d2 = ((self.momentum_lookback_ms as f64).ln() - (other.momentum_lookback_ms as f64).ln()).abs() / 5.0;
        let d3 = (self.take_profit_pct - other.take_profit_pct).abs() / 0.1;
        let d4 = (self.stop_loss_pct - other.stop_loss_pct).abs() / 0.05;
        let d5 = (self.position_size_pct - other.position_size_pct).abs() / 0.15;
        let d6 = (self.imbalance_threshold - other.imbalance_threshold).abs() / 0.8;
        let d7 = (self.volume_spike_threshold - other.volume_spike_threshold).abs() / 7.0;
        let d8 = (self.trailing_stop_pct - other.trailing_stop_pct).abs() / 0.03;
        
        (d1.powi(2) + d2.powi(2) + d3.powi(2) + d4.powi(2) + d5.powi(2) + d6.powi(2) + d7.powi(2) + d8.powi(2)).sqrt()
    }
}

// =============================================================================
// MARKET MAKING (AVELLANEDA-STOIKOV) CHROMOSOME
// =============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AvellanedaStoikovChromosome {
    pub risk_aversion: f64,
    pub order_size_pct: f64,  // Order size as percentage of capital (0.01 = 1%)
    pub window_ms: u64,
    pub min_quote_lifetime_ms: u64,
    // fee_rate removed - use exchange config instead
    // latency_buffer removed - latency now simulated in order matching
    pub inventory_target: f64,
    pub volatility_cap: f64,
    pub time_horizon_minutes: f64,
    pub market_depth_k: f64,
    
    // Phase 2: Dynamic adjustment parameters
    pub volatility_adjustment_factor: f64,
    pub inventory_penalty_factor: f64,
    pub trend_sensitivity: f64,
    pub flow_sensitivity: f64,
    
    // Trading control parameters
    pub min_spread_bps: f64,
    pub max_trades_per_minute: f64,
    
    // Adverse selection protection
    // 0.0 = ignore toxic flow, 1.0 = widen spreads proportionally, 2.0 = aggressive widening
    pub adverse_selection_sensitivity: f64,
    
    // Fee-based spread multiplier: how many times the fee rate to use as minimum spread
    // 2.0 = 2x fee (tight, more trades, higher fee drag)
    // 4.0 = 4x fee (balanced)
    // 6.0 = 6x fee (wide, fewer trades, more profit per trade)
    pub fee_spread_multiplier: f64,
    
    // Maximum inventory as multiple of order size
    // 2.0 = conservative (small positions), 10.0 = aggressive (large positions)
    // Higher values allow more directional exposure but increase risk
    pub max_inventory_multiple: f64,
    
    // Quote withdrawal threshold: flow imbalance level at which to withdraw quotes
    // 0.0 = disabled, 0.6 = withdraw when 60% of flow is one-sided (prevents adverse selection)
    // Higher values = more conservative (fewer fills but less toxic flow)
    pub quote_withdrawal_threshold: f64,
}

impl AvellanedaStoikovChromosome {
    /// Enforce bounds on all parameters to prevent invalid values
    pub fn enforce_bounds(&mut self) {
        self.risk_aversion = self.risk_aversion.clamp(0.0001, 1.0);
        // Order size: 0.5%-2% (was 5% - too large causes adverse selection on altcoins)
        self.order_size_pct = self.order_size_pct.clamp(0.005, 0.02);
        // Window: 1s-5min (was 30min - too slow for responsive MM)
        self.window_ms = self.window_ms.clamp(1_000, 300_000);
        // Quote lifetime: 10ms-10s (was 60s - too passive)
        self.min_quote_lifetime_ms = self.min_quote_lifetime_ms.clamp(10, 10_000);
        self.inventory_target = self.inventory_target.clamp(0.0, 0.5);
        self.volatility_cap = self.volatility_cap.clamp(0.20, 5.0);
        // Time horizon: 1-20 min (was 60min - too passive for active MM)
        self.time_horizon_minutes = self.time_horizon_minutes.clamp(1.0, 20.0);
        self.market_depth_k = self.market_depth_k.clamp(1.0, 50.0);
        // min_spread_bps: 5-25 bps for competitive quotes on liquid CEXs
        // The real profitability floor comes from fee_spread_multiplier × maker_fee
        // Wider spreads = fewer fills, so keep this tight for CoLocated infrastructure
        self.min_spread_bps = self.min_spread_bps.clamp(5.0, 25.0);
        // Adverse selection: 0 = aggressive, 1.5 = defensive (was 2.0 max)
        // Lower max to prevent over-widening on toxic flow
        self.adverse_selection_sensitivity = self.adverse_selection_sensitivity.clamp(0.0, 1.5);
        // Min 2x fee multiplier is breakeven: spread >= 2 × maker_fee
        // With 25 bps fee: 2x = 50 bps (breakeven), 2.5x = 62 bps, 3.0x = 75 bps max
        // Tighter upper bound (3.0 from 3.5) to encourage more fills
        self.fee_spread_multiplier = self.fee_spread_multiplier.clamp(2.0, 3.0);
        // Max inventory: 2x (conservative) to 10x (aggressive) order size
        self.max_inventory_multiple = self.max_inventory_multiple.clamp(2.0, 10.0);
        // Quote withdrawal: 0.0 (disabled) to 0.8 (very defensive)
        self.quote_withdrawal_threshold = self.quote_withdrawal_threshold.clamp(0.0, 0.8);
    }
}

impl Chromosome for AvellanedaStoikovChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            // Risk aversion (gamma): controls spread width and inventory sensitivity
            // Low (0.0001): aggressive, tight spreads - good for liquid markets
            // High (1.0): conservative, wide spreads - good for volatile/illiquid markets
            risk_aversion: rng.gen_range(0.0001..1.0),
            
            // Order size as percentage of capital
            // 0.5% = conservative to 2% = moderate (was 5% - too large causes adverse selection)
            order_size_pct: rng.gen_range(0.005..0.02),
            
            // Volatility estimation window: 1s to 5 min (was 30 min - too slow)
            // Short windows for HFT, long windows for swing trading
            // 300s (5min) max keeps strategy responsive to market changes
            window_ms: rng.gen_range(1_000..300_000),
            
            // Min time between quote updates: 10ms to 10s (was 60s - too passive)
            // Fast for HFT, slow for less active markets
            // 10s max ensures quotes stay fresh and competitive
            min_quote_lifetime_ms: rng.gen_range(10..10_000),
            
            // fee_rate removed - use exchange maker fee from config instead
            // latency_buffer removed - latency now simulated in order matching
            
            // Target inventory position - constrained for market making
            // Range (0.0..0.5) keeps inventory balanced (0.5 = target 50% in base asset)
            // Negative values don't make sense for market makers
            inventory_target: rng.gen_range(0.0..0.5),
            
            // Volatility cap to prevent extreme spread widening
            // NOTE: Must be >= vol_floor (0.15) in realistic_simulation.rs or all events get skipped
            // Range 0.20-5.0 ensures trades can execute (20% min > 15% floor)
            volatility_cap: rng.gen_range(0.20..5.0),
            
            // Time horizon: 1-20 minutes (was 60min - too passive for active MM)
            // Forces more active inventory management and quote updates
            time_horizon_minutes: rng.gen_range(1.0..20.0),
            
            // Market depth parameter k (higher = tighter spreads, divides liquidity term)
            // For liquid markets like BTC/USD, k should be 1-50
            // Low k (1.0) = wider spreads; High k (50) = very tight spreads
            // Range 1.0-50.0 for liquid crypto markets
            market_depth_k: rng.gen_range(1.0..50.0),
            
            // Phase 2: Dynamic adjustment parameters
            volatility_adjustment_factor: rng.gen_range(0.0..2.0),  // 0-200% vol adjustment
            inventory_penalty_factor: rng.gen_range(0.2..1.0),      // Minimum 20% penalty for some inventory control
            trend_sensitivity: rng.gen_range(-0.5..0.5),            // Moderate trend bias allowed
            flow_sensitivity: rng.gen_range(-1.0..1.0),             // Strong fade to strong follow
            
            // Minimum spread in basis points (soft floor)
            // The REAL profitability floor comes from fee_spread_multiplier × maker_fee
            // Range 5-25 bps for competitive quotes on liquid CEXs with CoLocated infra
            // Tighter spreads = more fills (with Beeks co-location we're front of queue)
            min_spread_bps: rng.gen_range(5.0..25.0),
            
            // Trade frequency: 0.01 to 60 trades per minute
            // 0.01 = 1 trade per 100 minutes (swing), 60 = 1 trade per second (HFT)
            max_trades_per_minute: rng.gen_range(0.01..60.0),
            
            // Adverse selection sensitivity: how much to widen spreads when detecting toxic flow
            // 0.0 = ignore (aggressive), 1.0 = proportional widening, 1.5 = defensive (was 2.0)
            adverse_selection_sensitivity: rng.gen_range(0.0..1.5),
            
            // Fee-based spread multiplier: how many times the exchange fee to use as min spread
            // Range 2.0-3.0: 2x = breakeven, 2.5x = small profit, 3.0x = max (was 3.5)
            // Tighter range encourages more fills
            fee_spread_multiplier: rng.gen_range(2.0..3.0),
            
            // Max inventory: how many order sizes of inventory to allow
            // 2x = very conservative, 5x = balanced, 10x = aggressive
            max_inventory_multiple: rng.gen_range(2.0..10.0),
            
            // Quote withdrawal threshold: withdraw quotes when flow imbalance exceeds this
            // 0.0 = disabled (aggressive), 0.6 = default, 0.8 = very defensive
            quote_withdrawal_threshold: rng.gen_range(0.4..0.8),
        }
    }

    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        let mut child = Self {
            risk_aversion: if rng.gen_bool(0.5) { self.risk_aversion } else { other.risk_aversion },
            order_size_pct: if rng.gen_bool(0.5) { self.order_size_pct } else { other.order_size_pct },
            window_ms: if rng.gen_bool(0.5) { self.window_ms } else { other.window_ms },
            min_quote_lifetime_ms: if rng.gen_bool(0.5) { self.min_quote_lifetime_ms } else { other.min_quote_lifetime_ms },
            // fee_rate removed - use exchange config
            // latency_buffer removed - latency now simulated in order matching
            inventory_target: if rng.gen_bool(0.5) { self.inventory_target } else { other.inventory_target },
            volatility_cap: if rng.gen_bool(0.5) { self.volatility_cap } else { other.volatility_cap },
            time_horizon_minutes: if rng.gen_bool(0.5) { self.time_horizon_minutes } else { other.time_horizon_minutes },
            market_depth_k: if rng.gen_bool(0.5) { self.market_depth_k } else { other.market_depth_k },
            
            // Phase 2: Crossover dynamic parameters
            volatility_adjustment_factor: if rng.gen_bool(0.5) { self.volatility_adjustment_factor } else { other.volatility_adjustment_factor },
            inventory_penalty_factor: if rng.gen_bool(0.5) { self.inventory_penalty_factor } else { other.inventory_penalty_factor },
            trend_sensitivity: if rng.gen_bool(0.5) { self.trend_sensitivity } else { other.trend_sensitivity },
            flow_sensitivity: if rng.gen_bool(0.5) { self.flow_sensitivity } else { other.flow_sensitivity },
            
            // Trading control crossover
            min_spread_bps: if rng.gen_bool(0.5) { self.min_spread_bps } else { other.min_spread_bps },
            max_trades_per_minute: if rng.gen_bool(0.5) { self.max_trades_per_minute } else { other.max_trades_per_minute },
            
            // Adverse selection crossover
            adverse_selection_sensitivity: if rng.gen_bool(0.5) { self.adverse_selection_sensitivity } else { other.adverse_selection_sensitivity },
            
            // Fee spread multiplier crossover
            fee_spread_multiplier: if rng.gen_bool(0.5) { self.fee_spread_multiplier } else { other.fee_spread_multiplier },
            
            // Max inventory crossover
            max_inventory_multiple: if rng.gen_bool(0.5) { self.max_inventory_multiple } else { other.max_inventory_multiple },
            
            // Quote withdrawal crossover
            quote_withdrawal_threshold: if rng.gen_bool(0.5) { self.quote_withdrawal_threshold } else { other.quote_withdrawal_threshold },
        };
        // Ensure bounds are enforced after crossover
        child.enforce_bounds();
        child
    }

    fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64) {
        // Risk aversion: range 0.0001-1.0
        if rng.gen_bool(mutation_rate) {
            self.risk_aversion *= rng.gen_range(0.5..2.0); // Multiplicative mutation for log-scale param
            self.risk_aversion = self.risk_aversion.clamp(0.0001, 1.0);
        }
        // Order size: range 0.5%-2% (was 5% - too large causes adverse selection)
        if rng.gen_bool(mutation_rate) {
            self.order_size_pct *= rng.gen_range(0.5..2.0);
            self.order_size_pct = self.order_size_pct.clamp(0.005, 0.02);
        }
        // Window: range 1s-5min (was 30min - too slow for responsive MM)
        if rng.gen_bool(mutation_rate) {
            let factor = rng.gen_range(0.5..2.0);
            self.window_ms = ((self.window_ms as f64 * factor) as u64).clamp(1_000, 300_000);
        }
        // Quote lifetime: range 10ms-10s (was 60s - too passive)
        if rng.gen_bool(mutation_rate) {
            let factor = rng.gen_range(0.5..2.0);
            self.min_quote_lifetime_ms = ((self.min_quote_lifetime_ms as f64 * factor) as u64).clamp(10, 10_000);
        }
        // fee_rate removed - use exchange config
        // latency_buffer removed - latency now simulated in order matching
        // Inventory target: constrained for market making (0 to 0.5 = % of capital in base asset)
        if rng.gen_bool(mutation_rate) {
            self.inventory_target += rng.gen_range(-0.1..0.1);
            self.inventory_target = self.inventory_target.clamp(0.0, 0.5);  // No negative targets for MM
        }
        // Volatility cap: range 0.20-5.0 (must be above vol_floor of 0.15)
        if rng.gen_bool(mutation_rate) {
            self.volatility_cap += rng.gen_range(-0.5..0.5);
            self.volatility_cap = self.volatility_cap.clamp(0.20, 5.0);
        }
        // Time horizon: range 1-20min (was 60min - too passive for active MM)
        if rng.gen_bool(mutation_rate) {
            self.time_horizon_minutes *= rng.gen_range(0.5..2.0);
            self.time_horizon_minutes = self.time_horizon_minutes.clamp(1.0, 20.0);
        }
        // Market depth: range 1.0-50.0
        if rng.gen_bool(mutation_rate) {
            self.market_depth_k += rng.gen_range(-5.0..5.0);
            self.market_depth_k = self.market_depth_k.clamp(1.0, 50.0);
        }
        
        // Phase 2: Mutate dynamic adjustment parameters
        if rng.gen_bool(mutation_rate) {
            self.volatility_adjustment_factor += rng.gen_range(-0.2..0.2);
            self.volatility_adjustment_factor = self.volatility_adjustment_factor.clamp(0.0, 2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.inventory_penalty_factor += rng.gen_range(-0.15..0.15);
            self.inventory_penalty_factor = self.inventory_penalty_factor.clamp(0.2, 1.0);  // Min 20% for some control
        }
        if rng.gen_bool(mutation_rate) {
            self.trend_sensitivity += rng.gen_range(-0.15..0.15);
            self.trend_sensitivity = self.trend_sensitivity.clamp(-0.5, 0.5);  // Moderate bounds
        }
        if rng.gen_bool(mutation_rate) {
            self.flow_sensitivity += rng.gen_range(-0.2..0.2);
            self.flow_sensitivity = self.flow_sensitivity.clamp(-1.0, 1.0);
        }
        
        // Trading control mutations
        // Min spread: 15-50 bps (was 3-30 - too tight causes adverse selection)
        // Wider spreads protect against volatility and informed traders
        if rng.gen_bool(mutation_rate) {
            self.min_spread_bps *= rng.gen_range(0.5..2.0);
            self.min_spread_bps = self.min_spread_bps.clamp(15.0, 50.0);
        }
        // Trade frequency: range 0.01-60 trades/min
        if rng.gen_bool(mutation_rate) {
            self.max_trades_per_minute *= rng.gen_range(0.5..2.0);
            self.max_trades_per_minute = self.max_trades_per_minute.clamp(0.01, 60.0);
        }
        // Adverse selection sensitivity: range 0.0-1.5 (was 2.0)
        if rng.gen_bool(mutation_rate) {
            self.adverse_selection_sensitivity += rng.gen_range(-0.3..0.3);
            self.adverse_selection_sensitivity = self.adverse_selection_sensitivity.clamp(0.0, 1.5);
        }
        // Fee spread multiplier: range 2.0-3.0 (was 3.5, tighter to improve fill rates)
        if rng.gen_bool(mutation_rate) {
            self.fee_spread_multiplier += rng.gen_range(-0.2..0.2);
            self.fee_spread_multiplier = self.fee_spread_multiplier.clamp(2.0, 3.0);
        }
        // Max inventory multiple: range 2.0-10.0
        if rng.gen_bool(mutation_rate) {
            self.max_inventory_multiple += rng.gen_range(-1.0..1.0);
            self.max_inventory_multiple = self.max_inventory_multiple.clamp(2.0, 10.0);
        }
        // Quote withdrawal threshold: range 0.0-0.8
        if rng.gen_bool(mutation_rate) {
            self.quote_withdrawal_threshold += rng.gen_range(-0.1..0.1);
            self.quote_withdrawal_threshold = self.quote_withdrawal_threshold.clamp(0.0, 0.8);
        }
    }
    
    fn distance(&self, other: &Self) -> f64 {
        // Normalized Euclidean distance across all parameters
        // Each parameter is normalized to [0, 1] based on its range
        // Use log-scale normalization for parameters with wide multiplicative ranges
        
        let d_risk = ((self.risk_aversion.ln() - other.risk_aversion.ln()).abs()) / (1.0_f64.ln() - 0.0001_f64.ln()).abs();
        let d_size = ((self.order_size_pct.ln() - other.order_size_pct.ln()).abs()) / (0.05_f64.ln() - 0.005_f64.ln()).abs();
        let d_window = ((self.window_ms as f64).ln() - (other.window_ms as f64).ln()).abs() / (1_800_000_f64.ln() - 1_000_f64.ln()).abs();
        let d_lifetime = ((self.min_quote_lifetime_ms as f64).ln() - (other.min_quote_lifetime_ms as f64).ln()).abs() / (60_000_f64.ln() - 10_f64.ln()).abs();
        // d_fee removed - fee_rate is not in chromosome
        // d_latency removed - latency_buffer is not in chromosome
        let d_inventory = (self.inventory_target - other.inventory_target).abs() / 2.0; // Range: -1.0 to 1.0
        let d_vol_cap = (self.volatility_cap - other.volatility_cap).abs() / 4.80; // Range: 0.20-5.0
        let d_horizon = ((self.time_horizon_minutes.ln() - other.time_horizon_minutes.ln()).abs()) / (60.0_f64.ln() - 0.167_f64.ln()).abs();
        let d_depth = (self.market_depth_k - other.market_depth_k).abs() / 49.0; // Range: 1.0-50.0
        
        // Phase 2: Dynamic parameter distances
        let d_vol_adj = (self.volatility_adjustment_factor - other.volatility_adjustment_factor).abs() / 2.0; // Range: 0-2.0
        let d_inv_penalty = (self.inventory_penalty_factor - other.inventory_penalty_factor).abs() / 1.0; // Range: 0-1.0
        let d_trend = (self.trend_sensitivity - other.trend_sensitivity).abs() / 2.0; // Range: -1.0 to 1.0
        let d_flow = (self.flow_sensitivity - other.flow_sensitivity).abs() / 2.0; // Range: -1.0 to 1.0
        
        // Trading control distances (log-scale for wide ranges)
        let d_spread = ((self.min_spread_bps.ln() - other.min_spread_bps.ln()).abs()) / (50.0_f64.ln() - 5.0_f64.ln()).abs();
        let d_trades = ((self.max_trades_per_minute.ln() - other.max_trades_per_minute.ln()).abs()) / (60.0_f64.ln() - 0.01_f64.ln()).abs();
        
        // Adverse selection distance
        let d_adverse = (self.adverse_selection_sensitivity - other.adverse_selection_sensitivity).abs() / 2.0; // Range: 0.0-2.0
        
        // Fee spread multiplier distance
        let d_fee_mult = (self.fee_spread_multiplier - other.fee_spread_multiplier).abs() / 1.5; // Range: 2.0-3.5
        
        // Max inventory multiple distance
        let d_inv_mult = (self.max_inventory_multiple - other.max_inventory_multiple).abs() / 8.0; // Range: 2.0-10.0
        
        // Quote withdrawal threshold distance
        let d_quote_wd = (self.quote_withdrawal_threshold - other.quote_withdrawal_threshold).abs() / 0.8; // Range: 0.0-0.8
        
        // Euclidean distance in 18D normalized space
        (d_risk.powi(2) + d_size.powi(2) + d_window.powi(2) + d_lifetime.powi(2) + 
         d_inventory.powi(2) + d_vol_cap.powi(2) +
         d_horizon.powi(2) + d_depth.powi(2) + d_vol_adj.powi(2) + d_inv_penalty.powi(2) +
         d_trend.powi(2) + d_flow.powi(2) +
         d_spread.powi(2) + d_trades.powi(2) + d_adverse.powi(2) + d_fee_mult.powi(2) + 
         d_inv_mult.powi(2) + d_quote_wd.powi(2)).sqrt()
    }
}
