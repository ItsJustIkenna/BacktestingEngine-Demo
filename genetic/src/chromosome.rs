//! Chromosome definitions for genetic optimization of strategy parameters

use rand::Rng;
use crate::Chromosome;

#[derive(Clone, Debug)]
pub struct AvellanedaStoikovChromosome {
    pub risk_aversion: f64,
    pub order_size_pct: f64,  // Order size as percentage of capital (0.01 = 1%)
    pub window_ms: u64,
    pub min_quote_lifetime_ms: u64,
    pub fee_rate: f64,
    pub latency_buffer: f64,
    pub inventory_target: f64,
    pub volatility_cap: f64,
    pub time_horizon_minutes: f64,
    pub market_depth_k: f64,
    
    // Phase 2: Dynamic adjustment parameters
    pub volatility_adjustment_factor: f64,
    pub inventory_penalty_factor: f64,
    pub trend_sensitivity: f64,
    pub flow_sensitivity: f64,
    
    // Exit and trading control parameters
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    pub min_spread_bps: f64,
    pub max_trades_per_minute: f64,
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
            
            // Expected fee rate for spread calculation (informational)
            // 0.1 bps (institutional) to 50 bps (retail altcoin exchanges)
            fee_rate: rng.gen_range(0.00001..0.005),
            
            // Latency buffer for slippage adjustment (as fraction)
            // 0.001% to 0.1% - very small buffer for price adjustment
            latency_buffer: rng.gen_range(0.00001..0.001),
            
            // Target inventory position (-1 = short bias, 0 = neutral, +1 = long bias)
            inventory_target: rng.gen_range(-1.0..1.0),
            
            // Volatility cap to prevent extreme spread widening
            volatility_cap: rng.gen_range(0.05..5.0),
            
            // Time horizon: 10 seconds to 60 minutes
            // Short for scalping, long for position trading
            time_horizon_minutes: rng.gen_range(0.167..60.0),
            
            // Market depth parameter (higher = tighter spreads)
            // Low for illiquid markets, high for liquid markets
            market_depth_k: rng.gen_range(0.1..10.0),
            
            // Phase 2: Dynamic adjustment parameters
            volatility_adjustment_factor: rng.gen_range(0.0..2.0),  // 0-200% vol adjustment
            inventory_penalty_factor: rng.gen_range(0.0..1.0),      // 0-100% penalty
            trend_sensitivity: rng.gen_range(-1.0..1.0),            // Strong contrarian to strong trend-following
            flow_sensitivity: rng.gen_range(-1.0..1.0),             // Strong fade to strong follow
            
            // Exit and trading control parameters
            // Take profit: 0.05% to 10% (scalping to swing)
            take_profit_pct: rng.gen_range(0.0005..0.10),
            // Stop loss: 0.05% to 5% (tight to wide)
            stop_loss_pct: rng.gen_range(0.0005..0.05),
            
            // Minimum spread in basis points
            // 1 bps (HFT on liquid markets) to 30 bps (retail exchanges)
            // Reduced from 200 - spreads > 30 bps rarely get fills
            min_spread_bps: rng.gen_range(1.0..30.0),
            
            // Trade frequency: 0.01 to 60 trades per minute
            // 0.01 = 1 trade per 100 minutes (swing), 60 = 1 trade per second (HFT)
            max_trades_per_minute: rng.gen_range(0.01..60.0),
        }
    }

    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        Self {
            risk_aversion: if rng.gen_bool(0.5) { self.risk_aversion } else { other.risk_aversion },
            order_size_pct: if rng.gen_bool(0.5) { self.order_size_pct } else { other.order_size_pct },
            window_ms: if rng.gen_bool(0.5) { self.window_ms } else { other.window_ms },
            min_quote_lifetime_ms: if rng.gen_bool(0.5) { self.min_quote_lifetime_ms } else { other.min_quote_lifetime_ms },
            fee_rate: if rng.gen_bool(0.5) { self.fee_rate } else { other.fee_rate },
            latency_buffer: if rng.gen_bool(0.5) { self.latency_buffer } else { other.latency_buffer },
            inventory_target: if rng.gen_bool(0.5) { self.inventory_target } else { other.inventory_target },
            volatility_cap: if rng.gen_bool(0.5) { self.volatility_cap } else { other.volatility_cap },
            time_horizon_minutes: if rng.gen_bool(0.5) { self.time_horizon_minutes } else { other.time_horizon_minutes },
            market_depth_k: if rng.gen_bool(0.5) { self.market_depth_k } else { other.market_depth_k },
            
            // Phase 2: Crossover dynamic parameters
            volatility_adjustment_factor: if rng.gen_bool(0.5) { self.volatility_adjustment_factor } else { other.volatility_adjustment_factor },
            inventory_penalty_factor: if rng.gen_bool(0.5) { self.inventory_penalty_factor } else { other.inventory_penalty_factor },
            trend_sensitivity: if rng.gen_bool(0.5) { self.trend_sensitivity } else { other.trend_sensitivity },
            flow_sensitivity: if rng.gen_bool(0.5) { self.flow_sensitivity } else { other.flow_sensitivity },
            
            // Exit and trading control crossover
            take_profit_pct: if rng.gen_bool(0.5) { self.take_profit_pct } else { other.take_profit_pct },
            stop_loss_pct: if rng.gen_bool(0.5) { self.stop_loss_pct } else { other.stop_loss_pct },
            min_spread_bps: if rng.gen_bool(0.5) { self.min_spread_bps } else { other.min_spread_bps },
            max_trades_per_minute: if rng.gen_bool(0.5) { self.max_trades_per_minute } else { other.max_trades_per_minute },
        }
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
        // Fee rate: range 0.001-50 bps
        if rng.gen_bool(mutation_rate) {
            self.fee_rate *= rng.gen_range(0.5..2.0);
            self.fee_rate = self.fee_rate.clamp(0.00001, 0.005);
        }
        // Latency buffer: range 0.00001-0.001 (tiny slippage buffer)
        if rng.gen_bool(mutation_rate) {
            self.latency_buffer *= rng.gen_range(0.5..2.0);
            self.latency_buffer = self.latency_buffer.clamp(0.00001, 0.001);
        }
        // Inventory target: range -1.0 to 1.0
        if rng.gen_bool(mutation_rate) {
            self.inventory_target += rng.gen_range(-0.3..0.3);
            self.inventory_target = self.inventory_target.clamp(-1.0, 1.0);
        }
        // Volatility cap: range 0.05-5.0
        if rng.gen_bool(mutation_rate) {
            self.volatility_cap += rng.gen_range(-0.5..0.5);
            self.volatility_cap = self.volatility_cap.clamp(0.05, 5.0);
        }
        // Time horizon: range 10s-60min
        if rng.gen_bool(mutation_rate) {
            self.time_horizon_minutes *= rng.gen_range(0.5..2.0);
            self.time_horizon_minutes = self.time_horizon_minutes.clamp(0.167, 60.0);
        }
        // Market depth: range 0.1-10.0
        if rng.gen_bool(mutation_rate) {
            self.market_depth_k += rng.gen_range(-1.0..1.0);
            self.market_depth_k = self.market_depth_k.clamp(0.1, 10.0);
        }
        
        // Phase 2: Mutate dynamic adjustment parameters
        if rng.gen_bool(mutation_rate) {
            self.volatility_adjustment_factor += rng.gen_range(-0.2..0.2);
            self.volatility_adjustment_factor = self.volatility_adjustment_factor.clamp(0.0, 2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.inventory_penalty_factor += rng.gen_range(-0.15..0.15);
            self.inventory_penalty_factor = self.inventory_penalty_factor.clamp(0.0, 1.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.trend_sensitivity += rng.gen_range(-0.2..0.2);
            self.trend_sensitivity = self.trend_sensitivity.clamp(-1.0, 1.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.flow_sensitivity += rng.gen_range(-0.2..0.2);
            self.flow_sensitivity = self.flow_sensitivity.clamp(-1.0, 1.0);
        }
        
        // Exit and trading control mutations
        if rng.gen_bool(mutation_rate) {
            self.take_profit_pct *= rng.gen_range(0.5..2.0);
            self.take_profit_pct = self.take_profit_pct.clamp(0.0005, 0.10);
        }
        if rng.gen_bool(mutation_rate) {
            self.stop_loss_pct *= rng.gen_range(0.5..2.0);
            self.stop_loss_pct = self.stop_loss_pct.clamp(0.0005, 0.05);
        }
        // Min spread: range 1-30 bps (tighter for fills)
        if rng.gen_bool(mutation_rate) {
            self.min_spread_bps *= rng.gen_range(0.5..2.0);
            self.min_spread_bps = self.min_spread_bps.clamp(1.0, 30.0);
        }
        // Trade frequency: range 0.01-60 trades/min
        if rng.gen_bool(mutation_rate) {
            self.max_trades_per_minute *= rng.gen_range(0.5..2.0);
            self.max_trades_per_minute = self.max_trades_per_minute.clamp(0.01, 60.0);
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
        let d_fee = ((self.fee_rate.ln() - other.fee_rate.ln()).abs()) / (0.005_f64.ln() - 0.00001_f64.ln()).abs();
        let d_latency = (self.latency_buffer - other.latency_buffer).abs() / 0.00099; // Range: 0.00001-0.001
        let d_inventory = (self.inventory_target - other.inventory_target).abs() / 2.0; // Range: -1.0 to 1.0
        let d_vol_cap = (self.volatility_cap - other.volatility_cap).abs() / 4.95; // Range: 0.05-5.0
        let d_horizon = ((self.time_horizon_minutes.ln() - other.time_horizon_minutes.ln()).abs()) / (60.0_f64.ln() - 0.167_f64.ln()).abs();
        let d_depth = (self.market_depth_k - other.market_depth_k).abs() / 9.9; // Range: 0.1-10.0
        
        // Phase 2: Dynamic parameter distances
        let d_vol_adj = (self.volatility_adjustment_factor - other.volatility_adjustment_factor).abs() / 2.0; // Range: 0-2.0
        let d_inv_penalty = (self.inventory_penalty_factor - other.inventory_penalty_factor).abs() / 1.0; // Range: 0-1.0
        let d_trend = (self.trend_sensitivity - other.trend_sensitivity).abs() / 2.0; // Range: -1.0 to 1.0
        let d_flow = (self.flow_sensitivity - other.flow_sensitivity).abs() / 2.0; // Range: -1.0 to 1.0
        
        // Exit and trading control distances (log-scale for wide ranges)
        let d_tp = ((self.take_profit_pct.ln() - other.take_profit_pct.ln()).abs()) / (0.10_f64.ln() - 0.0005_f64.ln()).abs();
        let d_sl = ((self.stop_loss_pct.ln() - other.stop_loss_pct.ln()).abs()) / (0.05_f64.ln() - 0.0005_f64.ln()).abs();
        let d_spread = ((self.min_spread_bps.ln() - other.min_spread_bps.ln()).abs()) / (200.0_f64.ln() - 1.0_f64.ln()).abs();
        let d_trades = ((self.max_trades_per_minute.ln() - other.max_trades_per_minute.ln()).abs()) / (60.0_f64.ln() - 0.01_f64.ln()).abs();
        
        // Euclidean distance in 18D normalized space
        (d_risk.powi(2) + d_size.powi(2) + d_window.powi(2) + d_lifetime.powi(2) + 
         d_fee.powi(2) + d_latency.powi(2) + d_inventory.powi(2) + d_vol_cap.powi(2) +
         d_horizon.powi(2) + d_depth.powi(2) + d_vol_adj.powi(2) + d_inv_penalty.powi(2) +
         d_trend.powi(2) + d_flow.powi(2) + d_tp.powi(2) + d_sl.powi(2) +
         d_spread.powi(2) + d_trades.powi(2)).sqrt()
    }
}
