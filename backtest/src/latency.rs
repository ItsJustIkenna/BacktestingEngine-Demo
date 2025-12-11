//! Variable Latency Utilities for Backtesting
//!
//! Provides helper functions for sampling variable latency values
//! to integrate with the simulation loop.

use rand::Rng;

/// Sample a variable latency value based on configuration.
/// 
/// # Arguments
/// * `base_latency_ms` - Base latency in milliseconds
/// * `jitter_pct` - Jitter as fraction (0.0-1.0) of base latency
/// * `min_latency_ms` - Minimum latency floor
/// * `spike_probability` - Probability of a latency spike (0.0-1.0)
/// 
/// # Returns
/// Sampled latency in milliseconds
pub fn sample_latency<R: Rng>(
    rng: &mut R,
    base_latency_ms: u64,
    jitter_pct: f64,
    min_latency_ms: u64,
    spike_probability: f64,
) -> i64 {
    let base = base_latency_ms as f64;
    
    // Check for latency spike
    let random_val: f64 = rng.gen();
    let is_spike = random_val < spike_probability;
    
    let latency = if is_spike {
        // Spike: 3x-10x normal latency
        let multiplier = rng.gen_range(3.0..10.0);
        base * multiplier
    } else {
        // Normal jitter
        let jitter_range = base * jitter_pct;
        let jitter = rng.gen_range(-jitter_range..jitter_range);
        base + jitter
    };
    
    // Clamp to bounds
    let max_latency = base * 20.0; // Cap at 20x base
    let clamped = latency.clamp(min_latency_ms as f64, max_latency);
    clamped.round() as i64
}

/// Calculate latency from TradingConfig with variable latency support
pub fn get_latency_from_config<R: Rng>(
    rng: &mut R,
    config: &config::TradingConfig,
) -> i64 {
    sample_latency(
        rng,
        config.latency_ms,
        config.latency_jitter_pct,
        config.min_latency_ms,
        config.latency_spike_probability,
    )
}

/// Thread-local latency sampler to avoid passing RNG everywhere
pub struct LatencySampler {
    base_latency_ms: u64,
    jitter_pct: f64,
    min_latency_ms: u64,
    spike_probability: f64,
}

impl LatencySampler {
    pub fn new(
        base_latency_ms: u64,
        jitter_pct: f64,
        min_latency_ms: u64,
        spike_probability: f64,
    ) -> Self {
        Self {
            base_latency_ms,
            jitter_pct,
            min_latency_ms,
            spike_probability,
        }
    }
    
    pub fn from_config(config: &config::TradingConfig) -> Self {
        Self::new(
            config.latency_ms,
            config.latency_jitter_pct,
            config.min_latency_ms,
            config.latency_spike_probability,
        )
    }
    
    /// Sample latency value
    pub fn sample(&self) -> i64 {
        let mut rng = rand::thread_rng();
        sample_latency(
            &mut rng,
            self.base_latency_ms,
            self.jitter_pct,
            self.min_latency_ms,
            self.spike_probability,
        )
    }
    
    /// Get fixed latency (base value, for deterministic mode)
    pub fn fixed(&self) -> i64 {
        self.base_latency_ms as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sample_latency() {
        let mut rng = rand::thread_rng();
        
        // Sample many times
        let samples: Vec<i64> = (0..1000)
            .map(|_| sample_latency(&mut rng, 10, 0.2, 1, 0.01))
            .collect();
        
        // Check bounds
        for s in &samples {
            assert!(*s >= 1, "Should be >= min_latency");
            assert!(*s <= 200, "Should be capped reasonably");
        }
        
        // Check variation
        let min = *samples.iter().min().unwrap();
        let max = *samples.iter().max().unwrap();
        assert!(max > min, "Should have variation");
    }
    
    #[test]
    fn test_latency_sampler() {
        // Use higher jitter (0.5 = 50%) to ensure variation in sampled values
        let sampler = LatencySampler::new(10, 0.5, 1, 0.05);
        
        let samples: Vec<i64> = (0..200).map(|_| sampler.sample()).collect();
        
        // Should have variation - with 50% jitter on base 10, we get range 5-15
        // plus occasional spikes (5% chance), so expect at least 3 unique values
        let unique: std::collections::HashSet<_> = samples.iter().collect();
        assert!(unique.len() >= 3, "Should have varied samples, got {} unique values", unique.len());
    }
}
