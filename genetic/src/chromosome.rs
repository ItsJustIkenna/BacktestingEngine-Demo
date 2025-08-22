//! Chromosome definitions for genetic optimization of strategy parameters

use rand::Rng;
use crate::Chromosome;

#[derive(Clone, Debug)]
pub struct AvellanedaStoikovChromosome {
    pub risk_aversion: f64,
    pub inventory_target: f64,
    pub quote_width: f64,
}

impl Chromosome for AvellanedaStoikovChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            risk_aversion: rng.gen_range(0.01..1.0),
            inventory_target: rng.gen_range(-1.0..1.0),
            quote_width: rng.gen_range(0.01..0.5),
        }
    }

    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        Self {
            risk_aversion: if rng.gen_bool(0.5) { self.risk_aversion } else { other.risk_aversion },
            inventory_target: if rng.gen_bool(0.5) { self.inventory_target } else { other.inventory_target },
            quote_width: if rng.gen_bool(0.5) { self.quote_width } else { other.quote_width },
        }
    }

    fn mutate(&mut self, rng: &mut impl Rng) {
        if rng.gen_bool(0.3) {
            self.risk_aversion += rng.gen_range(-0.05..0.05);
            self.risk_aversion = self.risk_aversion.clamp(0.01, 1.0);
        }
        if rng.gen_bool(0.3) {
            self.inventory_target += rng.gen_range(-0.1..0.1);
            self.inventory_target = self.inventory_target.clamp(-1.0, 1.0);
        }
        if rng.gen_bool(0.3) {
            self.quote_width += rng.gen_range(-0.01..0.01);
            self.quote_width = self.quote_width.clamp(0.01, 0.5);
        }
    }
}
