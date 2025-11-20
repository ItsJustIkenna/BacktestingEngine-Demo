//! Chromosome definitions for genetic optimization of strategy parameters

use rand::Rng;
use crate::Chromosome;

#[derive(Clone, Debug)]
pub struct AvellanedaStoikovChromosome {
    pub risk_aversion: f64,
    pub order_size: f64,
    pub window_ms: u64,
    pub min_quote_lifetime_ms: u64,
    pub fee_rate: f64,
    pub latency_buffer: f64,
    pub inventory_target: f64,
    pub volatility_cap: f64,
    pub time_horizon_minutes: f64,
    pub market_depth_k: f64,
}

impl Chromosome for AvellanedaStoikovChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            risk_aversion: rng.gen_range(0.001..0.1),
            order_size: rng.gen_range(0.001..1.0),
            window_ms: rng.gen_range(10_000..300_000), // 10s to 5 min
            min_quote_lifetime_ms: rng.gen_range(100..5_000), // 100ms to 5s
            fee_rate: rng.gen_range(0.0001..0.01),
            latency_buffer: rng.gen_range(0.1..2.0),
            inventory_target: rng.gen_range(-1.0..1.0),
            volatility_cap: rng.gen_range(0.1..3.0),
            time_horizon_minutes: rng.gen_range(0.5..5.0), // 30s to 5 min
            market_depth_k: rng.gen_range(0.5..5.0), // 0.5 to 5.0
        }
    }

    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        Self {
            risk_aversion: if rng.gen_bool(0.5) { self.risk_aversion } else { other.risk_aversion },
            order_size: if rng.gen_bool(0.5) { self.order_size } else { other.order_size },
            window_ms: if rng.gen_bool(0.5) { self.window_ms } else { other.window_ms },
            min_quote_lifetime_ms: if rng.gen_bool(0.5) { self.min_quote_lifetime_ms } else { other.min_quote_lifetime_ms },
            fee_rate: if rng.gen_bool(0.5) { self.fee_rate } else { other.fee_rate },
            latency_buffer: if rng.gen_bool(0.5) { self.latency_buffer } else { other.latency_buffer },
            inventory_target: if rng.gen_bool(0.5) { self.inventory_target } else { other.inventory_target },
            volatility_cap: if rng.gen_bool(0.5) { self.volatility_cap } else { other.volatility_cap },
            time_horizon_minutes: if rng.gen_bool(0.5) { self.time_horizon_minutes } else { other.time_horizon_minutes },
            market_depth_k: if rng.gen_bool(0.5) { self.market_depth_k } else { other.market_depth_k },
        }
    }

    fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64) {
        // Use the adaptive mutation_rate parameter instead of fixed 0.3
        if rng.gen_bool(mutation_rate) {
            self.risk_aversion += rng.gen_range(-0.01..0.01);
            self.risk_aversion = self.risk_aversion.clamp(0.001, 0.1);
        }
        if rng.gen_bool(mutation_rate) {
            self.order_size += rng.gen_range(-0.1..0.1);
            self.order_size = self.order_size.clamp(0.001, 1.0);
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
    }
    
    fn distance(&self, other: &Self) -> f64 {
        // Normalized Euclidean distance across all parameters
        // Each parameter is normalized to [0, 1] based on its range
        
        let d_risk = (self.risk_aversion - other.risk_aversion).abs() / 0.1; // Range: 0.001-0.1
        let d_size = (self.order_size - other.order_size).abs() / 1.0; // Range: 0.001-1.0
        let d_window = (self.window_ms as f64 - other.window_ms as f64).abs() / 290_000.0; // Range: 10k-300k
        let d_lifetime = (self.min_quote_lifetime_ms as f64 - other.min_quote_lifetime_ms as f64).abs() / 4_900.0; // Range: 100-5000
        let d_fee = (self.fee_rate - other.fee_rate).abs() / 0.01; // Range: 0.0001-0.01
        let d_latency = (self.latency_buffer - other.latency_buffer).abs() / 2.0; // Range: 0.1-2.0
        let d_inventory = (self.inventory_target - other.inventory_target).abs() / 2.0; // Range: -1.0 to 1.0
        let d_vol_cap = (self.volatility_cap - other.volatility_cap).abs() / 3.0; // Range: 0.1-3.0
        let d_horizon = (self.time_horizon_minutes - other.time_horizon_minutes).abs() / 5.0; // Range: 0.5-5.0
        let d_depth = (self.market_depth_k - other.market_depth_k).abs() / 5.0; // Range: 0.5-5.0
        
        // Euclidean distance in 10D normalized space
        (d_risk.powi(2) + d_size.powi(2) + d_window.powi(2) + d_lifetime.powi(2) + 
         d_fee.powi(2) + d_latency.powi(2) + d_inventory.powi(2) + d_vol_cap.powi(2) +
         d_horizon.powi(2) + d_depth.powi(2)).sqrt()
    }
}
