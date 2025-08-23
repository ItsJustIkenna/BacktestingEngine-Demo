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
        }
    }

    fn mutate(&mut self, rng: &mut impl Rng) {
        if rng.gen_bool(0.3) {
            self.risk_aversion += rng.gen_range(-0.01..0.01);
            self.risk_aversion = self.risk_aversion.clamp(0.001, 0.1);
        }
        if rng.gen_bool(0.3) {
            self.order_size += rng.gen_range(-0.1..0.1);
            self.order_size = self.order_size.clamp(0.001, 1.0);
        }
        if rng.gen_bool(0.3) {
            let delta = rng.gen_range(-10_000..10_000);
            self.window_ms = (self.window_ms as i64 + delta).clamp(10_000, 300_000) as u64;
        }
        if rng.gen_bool(0.3) {
            let delta = rng.gen_range(-500..500);
            self.min_quote_lifetime_ms = (self.min_quote_lifetime_ms as i64 + delta).clamp(100, 5_000) as u64;
        }
        if rng.gen_bool(0.3) {
            self.fee_rate += rng.gen_range(-0.001..0.001);
            self.fee_rate = self.fee_rate.clamp(0.0001, 0.01);
        }
        if rng.gen_bool(0.3) {
            self.latency_buffer += rng.gen_range(-0.2..0.2);
            self.latency_buffer = self.latency_buffer.clamp(0.1, 2.0);
        }
        if rng.gen_bool(0.3) {
            self.inventory_target += rng.gen_range(-0.2..0.2);
            self.inventory_target = self.inventory_target.clamp(-1.0, 1.0);
        }
        if rng.gen_bool(0.3) {
            self.volatility_cap += rng.gen_range(-0.2..0.2);
            self.volatility_cap = self.volatility_cap.clamp(0.1, 3.0);
        }
    }
}
