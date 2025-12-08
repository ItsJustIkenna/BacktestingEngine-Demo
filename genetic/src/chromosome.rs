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
            risk_aversion: rng.gen_range(0.001..0.1),
            // order_size_pct: percentage of capital per order (0.5% to 15%)
            // Wide range lets genetic algorithm find optimal sizing
            // At $10k capital: 0.5% = $50, 15% = $1,500 per trade
            order_size_pct: rng.gen_range(0.005..0.15),
            window_ms: rng.gen_range(10_000..300_000), // 10s to 5 min
            min_quote_lifetime_ms: rng.gen_range(100..5_000), // 100ms to 5s
            fee_rate: rng.gen_range(0.0001..0.01),
            latency_buffer: rng.gen_range(0.1..2.0),
            inventory_target: rng.gen_range(-1.0..1.0),
            volatility_cap: rng.gen_range(0.1..3.0),
            time_horizon_minutes: rng.gen_range(0.5..5.0), // 30s to 5 min
            market_depth_k: rng.gen_range(0.5..5.0), // 0.5 to 5.0
            
            // Phase 2: Dynamic adjustment parameters
            volatility_adjustment_factor: rng.gen_range(0.0..1.0), // 0-100% adjustment
            inventory_penalty_factor: rng.gen_range(0.0..0.8),     // 0-80% penalty
            trend_sensitivity: rng.gen_range(-0.5..0.5),           // Contrarian to trend-following
            flow_sensitivity: rng.gen_range(-0.3..0.3),            // Fade or follow flow
            
            // Exit and trading control parameters
            take_profit_pct: rng.gen_range(0.002..0.02),           // 0.2% to 2% take profit
            stop_loss_pct: rng.gen_range(0.001..0.01),             // 0.1% to 1% stop loss
            // min_spread_bps range depends on exchange fees:
            // - DeepBook (SUI): 10 bps round-trip → min 25 bps for profitability
            // - Kraken (low vol): 50 bps round-trip → min 125 bps for profitability
            // Range: 20-150 bps to cover both scenarios
            min_spread_bps: rng.gen_range(20.0..150.0),
            // Trade frequency - balance between opportunity capture and cost
            max_trades_per_minute: rng.gen_range(0.5..15.0),       // 0.5 to 15 trades per minute
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
        // Use the adaptive mutation_rate parameter instead of fixed 0.3
        if rng.gen_bool(mutation_rate) {
            self.risk_aversion += rng.gen_range(-0.01..0.01);
            self.risk_aversion = self.risk_aversion.clamp(0.001, 0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.order_size_pct += rng.gen_range(-0.02..0.02);
            self.order_size_pct = self.order_size_pct.clamp(0.005, 0.15);
        }
        if rng.gen_bool(mutation_rate) {
            let delta = rng.gen_range(-10_000..10_000);
            self.window_ms = (self.window_ms as i64 + delta).clamp(10_000, 300_000) as u64;
        }
        if rng.gen_bool(mutation_rate) {
            let delta = rng.gen_range(-500..500);
            self.min_quote_lifetime_ms = (self.min_quote_lifetime_ms as i64 + delta).clamp(100, 5_000) as u64;
        }
        if rng.gen_bool(mutation_rate) {
            self.fee_rate += rng.gen_range(-0.001..0.001);
            self.fee_rate = self.fee_rate.clamp(0.0001, 0.01);
        }
        if rng.gen_bool(mutation_rate) {
            self.latency_buffer += rng.gen_range(-0.2..0.2);
            self.latency_buffer = self.latency_buffer.clamp(0.1, 2.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.inventory_target += rng.gen_range(-0.2..0.2);
            self.inventory_target = self.inventory_target.clamp(-1.0, 1.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.volatility_cap += rng.gen_range(-0.2..0.2);
            self.volatility_cap = self.volatility_cap.clamp(0.1, 3.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.time_horizon_minutes += rng.gen_range(-0.5..0.5);
            self.time_horizon_minutes = self.time_horizon_minutes.clamp(0.5, 5.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.market_depth_k += rng.gen_range(-0.5..0.5);
            self.market_depth_k = self.market_depth_k.clamp(0.5, 5.0);
        }
        
        // Phase 2: Mutate dynamic adjustment parameters
        if rng.gen_bool(mutation_rate) {
            self.volatility_adjustment_factor += rng.gen_range(-0.1..0.1);
            self.volatility_adjustment_factor = self.volatility_adjustment_factor.clamp(0.0, 1.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.inventory_penalty_factor += rng.gen_range(-0.1..0.1);
            self.inventory_penalty_factor = self.inventory_penalty_factor.clamp(0.0, 0.8);
        }
        if rng.gen_bool(mutation_rate) {
            self.trend_sensitivity += rng.gen_range(-0.1..0.1);
            self.trend_sensitivity = self.trend_sensitivity.clamp(-0.5, 0.5);
        }
        if rng.gen_bool(mutation_rate) {
            self.flow_sensitivity += rng.gen_range(-0.05..0.05);
            self.flow_sensitivity = self.flow_sensitivity.clamp(-0.3, 0.3);
        }
        
        // Exit and trading control mutations
        if rng.gen_bool(mutation_rate) {
            self.take_profit_pct += rng.gen_range(-0.002..0.002);
            self.take_profit_pct = self.take_profit_pct.clamp(0.002, 0.02);
        }
        if rng.gen_bool(mutation_rate) {
            self.stop_loss_pct += rng.gen_range(-0.001..0.001);
            self.stop_loss_pct = self.stop_loss_pct.clamp(0.001, 0.01);
        }
        if rng.gen_bool(mutation_rate) {
            self.min_spread_bps += rng.gen_range(-15.0..15.0);
            self.min_spread_bps = self.min_spread_bps.clamp(20.0, 150.0);
        }
        if rng.gen_bool(mutation_rate) {
            self.max_trades_per_minute += rng.gen_range(-2.0..2.0);
            self.max_trades_per_minute = self.max_trades_per_minute.clamp(0.5, 15.0);
        }
    }
    
    fn distance(&self, other: &Self) -> f64 {
        // Normalized Euclidean distance across all parameters
        // Each parameter is normalized to [0, 1] based on its range
        
        let d_risk = (self.risk_aversion - other.risk_aversion).abs() / 0.1; // Range: 0.001-0.1
        let d_size = (self.order_size_pct - other.order_size_pct).abs() / 0.145; // Range: 0.005-0.15 (0.5-15% of capital)
        let d_window = (self.window_ms as f64 - other.window_ms as f64).abs() / 290_000.0; // Range: 10k-300k
        let d_lifetime = (self.min_quote_lifetime_ms as f64 - other.min_quote_lifetime_ms as f64).abs() / 4_900.0; // Range: 100-5000
        let d_fee = (self.fee_rate - other.fee_rate).abs() / 0.01; // Range: 0.0001-0.01
        let d_latency = (self.latency_buffer - other.latency_buffer).abs() / 2.0; // Range: 0.1-2.0
        let d_inventory = (self.inventory_target - other.inventory_target).abs() / 2.0; // Range: -1.0 to 1.0
        let d_vol_cap = (self.volatility_cap - other.volatility_cap).abs() / 3.0; // Range: 0.1-3.0
        let d_horizon = (self.time_horizon_minutes - other.time_horizon_minutes).abs() / 5.0; // Range: 0.5-5.0
        let d_depth = (self.market_depth_k - other.market_depth_k).abs() / 5.0; // Range: 0.5-5.0
        
        // Phase 2: Add dynamic parameter distances
        let d_vol_adj = (self.volatility_adjustment_factor - other.volatility_adjustment_factor).abs() / 1.0; // Range: 0-1
        let d_inv_penalty = (self.inventory_penalty_factor - other.inventory_penalty_factor).abs() / 0.8; // Range: 0-0.8
        let d_trend = (self.trend_sensitivity - other.trend_sensitivity).abs() / 1.0; // Range: -0.5 to 0.5
        let d_flow = (self.flow_sensitivity - other.flow_sensitivity).abs() / 0.6; // Range: -0.3 to 0.3
        
        // Exit and trading control distances
        let d_tp = (self.take_profit_pct - other.take_profit_pct).abs() / 0.018; // Range: 0.002-0.02
        let d_sl = (self.stop_loss_pct - other.stop_loss_pct).abs() / 0.009; // Range: 0.001-0.01
        let d_spread = (self.min_spread_bps - other.min_spread_bps).abs() / 45.0; // Range: 5-50
        let d_trades = (self.max_trades_per_minute - other.max_trades_per_minute).abs() / 55.0; // Range: 5-60
        
        // Euclidean distance in 18D normalized space
        (d_risk.powi(2) + d_size.powi(2) + d_window.powi(2) + d_lifetime.powi(2) + 
         d_fee.powi(2) + d_latency.powi(2) + d_inventory.powi(2) + d_vol_cap.powi(2) +
         d_horizon.powi(2) + d_depth.powi(2) + d_vol_adj.powi(2) + d_inv_penalty.powi(2) +
         d_trend.powi(2) + d_flow.powi(2) + d_tp.powi(2) + d_sl.powi(2) +
         d_spread.powi(2) + d_trades.powi(2)).sqrt()
    }
}
