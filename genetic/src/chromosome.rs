//! Chromosome definitions for genetic optimization of strategy parameters

use rand::Rng;
use serde::{Serialize, Deserialize};
use crate::Chromosome;

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
}

impl AvellanedaStoikovChromosome {
    /// Enforce bounds on all parameters to prevent invalid values
    pub fn enforce_bounds(&mut self) {
        self.risk_aversion = self.risk_aversion.clamp(0.0001, 1.0);
        self.order_size_pct = self.order_size_pct.clamp(0.005, 0.05);
        self.inventory_target = self.inventory_target.clamp(0.0, 0.5);
        self.volatility_cap = self.volatility_cap.clamp(0.20, 5.0);
        self.time_horizon_minutes = self.time_horizon_minutes.clamp(0.167, 60.0);
        self.market_depth_k = self.market_depth_k.clamp(1.0, 50.0);
        // min_spread_bps is a soft floor - the REAL floor comes from fee_spread_multiplier
        // Range 5-50 bps - tighter bounds to prevent wide spreads that kill fill rates
        self.min_spread_bps = self.min_spread_bps.clamp(5.0, 50.0);
        self.adverse_selection_sensitivity = self.adverse_selection_sensitivity.clamp(0.0, 2.0);
        // Min 2x fee multiplier is breakeven: spread >= 2 × maker_fee
        // With 25 bps fee: 2x = 50 bps (breakeven), 3x = 75 bps, 3.5x = 87 bps max
        // Tighter upper bound (3.5) prevents wide spreads that kill fill rates
        self.fee_spread_multiplier = self.fee_spread_multiplier.clamp(2.0, 3.5);
        // Max inventory: 2x (conservative) to 10x (aggressive) order size
        self.max_inventory_multiple = self.max_inventory_multiple.clamp(2.0, 10.0);
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
            // 0.5% = conservative to 5% = moderate (avoid excessive commission at higher sizes)
            order_size_pct: rng.gen_range(0.005..0.05),
            
            // Volatility estimation window: 1s to 30 min
            // Short windows for HFT, long windows for swing trading
            window_ms: rng.gen_range(1_000..1_800_000),
            
            // Min time between quote updates: 10ms to 60s
            // Fast for HFT, slow for less active markets
            min_quote_lifetime_ms: rng.gen_range(10..60_000),
            
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
            
            // Time horizon: 10 seconds to 60 minutes
            // Short for scalping, long for position trading
            time_horizon_minutes: rng.gen_range(0.167..60.0),
            
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
            // This is a secondary constraint for market conditions (volatility, liquidity)
            // Range 5-50 bps - tighter bounds to ensure reasonable fill rates
            min_spread_bps: rng.gen_range(5.0..50.0),
            
            // Trade frequency: 0.01 to 60 trades per minute
            // 0.01 = 1 trade per 100 minutes (swing), 60 = 1 trade per second (HFT)
            max_trades_per_minute: rng.gen_range(0.01..60.0),
            
            // Adverse selection sensitivity: how much to widen spreads when detecting toxic flow
            // 0.0 = ignore (aggressive), 1.0 = proportional widening, 2.0 = defensive
            adverse_selection_sensitivity: rng.gen_range(0.0..2.0),
            
            // Fee-based spread multiplier: how many times the exchange fee to use as min spread
            // Range 2.0-3.5: 2x = breakeven, 2.5x = small profit, 3.5x = max (87 bps with 25 bps fee)
            // Tighter range prevents wide spreads that kill fill rates
            fee_spread_multiplier: rng.gen_range(2.0..3.5),
            
            // Max inventory: how many order sizes of inventory to allow
            // 2x = very conservative, 5x = balanced, 10x = aggressive
            max_inventory_multiple: rng.gen_range(2.0..10.0),
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
        // Order size: range 0.5%-5% (reduced to avoid excessive commission)
        if rng.gen_bool(mutation_rate) {
            self.order_size_pct *= rng.gen_range(0.5..2.0);
            self.order_size_pct = self.order_size_pct.clamp(0.005, 0.05);
        }
        // Window: range 1s-30min
        if rng.gen_bool(mutation_rate) {
            let factor = rng.gen_range(0.5..2.0);
            self.window_ms = ((self.window_ms as f64 * factor) as u64).clamp(1_000, 1_800_000);
        }
        // Quote lifetime: range 10ms-60s
        if rng.gen_bool(mutation_rate) {
            let factor = rng.gen_range(0.5..2.0);
            self.min_quote_lifetime_ms = ((self.min_quote_lifetime_ms as f64 * factor) as u64).clamp(10, 60_000);
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
        // Time horizon: range 10s-60min
        if rng.gen_bool(mutation_rate) {
            self.time_horizon_minutes *= rng.gen_range(0.5..2.0);
            self.time_horizon_minutes = self.time_horizon_minutes.clamp(0.167, 60.0);
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
        // Min spread: soft floor (profitability ensured by fee_spread_multiplier)
        if rng.gen_bool(mutation_rate) {
            self.min_spread_bps *= rng.gen_range(0.5..2.0);
            self.min_spread_bps = self.min_spread_bps.clamp(5.0, 50.0);
        }
        // Trade frequency: range 0.01-60 trades/min
        if rng.gen_bool(mutation_rate) {
            self.max_trades_per_minute *= rng.gen_range(0.5..2.0);
            self.max_trades_per_minute = self.max_trades_per_minute.clamp(0.01, 60.0);
        }
        // Adverse selection sensitivity: range 0.0-2.0
        if rng.gen_bool(mutation_rate) {
            self.adverse_selection_sensitivity += rng.gen_range(-0.3..0.3);
            self.adverse_selection_sensitivity = self.adverse_selection_sensitivity.clamp(0.0, 2.0);
        }
        // Fee spread multiplier: range 2.0-3.5 (tighter to improve fill rates)
        if rng.gen_bool(mutation_rate) {
            self.fee_spread_multiplier += rng.gen_range(-0.3..0.3);
            self.fee_spread_multiplier = self.fee_spread_multiplier.clamp(2.0, 3.5);
        }
        // Max inventory multiple: range 2.0-10.0
        if rng.gen_bool(mutation_rate) {
            self.max_inventory_multiple += rng.gen_range(-1.0..1.0);
            self.max_inventory_multiple = self.max_inventory_multiple.clamp(2.0, 10.0);
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
        
        // Euclidean distance in 17D normalized space
        (d_risk.powi(2) + d_size.powi(2) + d_window.powi(2) + d_lifetime.powi(2) + 
         d_inventory.powi(2) + d_vol_cap.powi(2) +
         d_horizon.powi(2) + d_depth.powi(2) + d_vol_adj.powi(2) + d_inv_penalty.powi(2) +
         d_trend.powi(2) + d_flow.powi(2) +
         d_spread.powi(2) + d_trades.powi(2) + d_adverse.powi(2) + d_fee_mult.powi(2) + d_inv_mult.powi(2)).sqrt()
    }
}
