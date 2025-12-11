//! Variable latency simulation with jitter, spikes, and realistic network modeling
//!
//! Provides realistic latency simulation for backtesting that accounts for:
//! - Network jitter (random variation around base latency)
//! - Latency spikes (occasional high-latency events)
//! - Time-of-day patterns (optional)

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for variable latency simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableLatencyConfig {
    /// Base latency in milliseconds
    pub base_latency_ms: u64,
    /// Jitter as fraction of base latency (0.0-1.0)
    pub jitter_fraction: f64,
    /// Minimum latency floor in milliseconds
    pub min_latency_ms: u64,
    /// Maximum latency cap in milliseconds
    pub max_latency_ms: u64,
    /// Probability of a latency spike (0.0-1.0)
    pub spike_probability: f64,
    /// Spike multiplier range (min, max) - e.g., (3.0, 10.0) means 3x-10x normal latency
    pub spike_multiplier_range: (f64, f64),
}

impl Default for VariableLatencyConfig {
    fn default() -> Self {
        Self {
            base_latency_ms: 10,
            jitter_fraction: 0.2,
            min_latency_ms: 1,
            max_latency_ms: 500,
            spike_probability: 0.01,
            spike_multiplier_range: (3.0, 10.0),
        }
    }
}

impl VariableLatencyConfig {
    /// Create from TradingConfig fields
    pub fn from_trading_config(
        latency_ms: u64,
        jitter_pct: f64,
        min_latency_ms: u64,
        spike_probability: f64,
    ) -> Self {
        Self {
            base_latency_ms: latency_ms,
            jitter_fraction: jitter_pct,
            min_latency_ms,
            max_latency_ms: latency_ms * 20, // Cap at 20x base latency
            spike_probability,
            spike_multiplier_range: (3.0, 10.0),
        }
    }
    
    /// Generate a sample latency value with jitter and optional spikes
    pub fn sample_latency<R: Rng>(&self, rng: &mut R) -> u64 {
        let base = self.base_latency_ms as f64;
        
        // Check for latency spike
        let random_val: f64 = rng.gen();
        let is_spike = random_val < self.spike_probability;
        
        let latency = if is_spike {
            // Generate spike multiplier
            let multiplier = rng.gen_range(self.spike_multiplier_range.0..self.spike_multiplier_range.1);
            base * multiplier
        } else {
            // Normal jitter: uniform distribution around base
            let jitter_range = base * self.jitter_fraction;
            let jitter = rng.gen_range(-jitter_range..jitter_range);
            base + jitter
        };
        
        // Clamp to bounds
        let clamped = latency.clamp(self.min_latency_ms as f64, self.max_latency_ms as f64);
        clamped.round() as u64
    }
    
    /// Generate a batch of latency samples (useful for pre-generating latencies)
    pub fn sample_latencies<R: Rng>(&self, rng: &mut R, count: usize) -> Vec<u64> {
        (0..count).map(|_| self.sample_latency(rng)).collect()
    }
}

/// Latency simulator that maintains state across samples
pub struct LatencySimulator {
    config: VariableLatencyConfig,
    /// Running estimate of network conditions (for autocorrelation)
    current_baseline: f64,
    /// Mean reversion speed
    mean_reversion_speed: f64,
    /// Volatility of baseline changes
    baseline_volatility: f64,
}

impl LatencySimulator {
    pub fn new(config: VariableLatencyConfig) -> Self {
        Self {
            current_baseline: config.base_latency_ms as f64,
            mean_reversion_speed: 0.1,
            baseline_volatility: 0.05,
            config,
        }
    }
    
    /// Sample latency with autocorrelation (network conditions persist)
    pub fn sample_with_autocorrelation<R: Rng>(&mut self, rng: &mut R) -> u64 {
        // Update baseline with mean reversion
        let target = self.config.base_latency_ms as f64;
        let noise_raw: f64 = rng.gen();
        let noise = noise_raw * 2.0 - 1.0;
        
        self.current_baseline = self.current_baseline 
            + self.mean_reversion_speed * (target - self.current_baseline)
            + self.baseline_volatility * noise * target;
        
        // Clamp baseline
        self.current_baseline = self.current_baseline.clamp(
            self.config.min_latency_ms as f64,
            self.config.max_latency_ms as f64 / 2.0,
        );
        
        // Now sample with jitter around current baseline
        let jitter_range = self.current_baseline * self.config.jitter_fraction;
        let jitter = rng.gen_range(-jitter_range..jitter_range);
        
        // Check for spike
        let spike_random: f64 = rng.gen();
        let is_spike = spike_random < self.config.spike_probability;
        
        let latency = if is_spike {
            let multiplier = rng.gen_range(
                self.config.spike_multiplier_range.0..self.config.spike_multiplier_range.1
            );
            (self.current_baseline + jitter) * multiplier
        } else {
            self.current_baseline + jitter
        };
        
        latency.clamp(
            self.config.min_latency_ms as f64,
            self.config.max_latency_ms as f64,
        ).round() as u64
    }
    
    /// Get current baseline latency estimate
    pub fn current_baseline(&self) -> f64 {
        self.current_baseline
    }
    
    /// Reset baseline to configured default
    pub fn reset(&mut self) {
        self.current_baseline = self.config.base_latency_ms as f64;
    }
}

/// Time-of-day latency patterns (optional enhancement)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeOfDayLatencyPattern {
    /// Hour of day (0-23) -> latency multiplier
    /// Higher during market open/close, lower during quiet periods
    pub hourly_multipliers: [f64; 24],
}

impl Default for TimeOfDayLatencyPattern {
    fn default() -> Self {
        // Pattern: higher at market open (9-10), lunch (12), close (15-16)
        let mut multipliers = [1.0; 24];
        multipliers[9] = 1.5;   // Market open
        multipliers[10] = 1.3;
        multipliers[12] = 1.1;  // Lunch
        multipliers[15] = 1.4;  // Near close
        multipliers[16] = 1.6;  // Market close
        
        Self {
            hourly_multipliers: multipliers,
        }
    }
}

impl TimeOfDayLatencyPattern {
    pub fn get_multiplier(&self, hour: u32) -> f64 {
        self.hourly_multipliers[(hour % 24) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    
    #[test]
    fn test_latency_sampling() {
        let config = VariableLatencyConfig::default();
        let mut rng = thread_rng();
        
        let samples: Vec<u64> = (0..1000).map(|_| config.sample_latency(&mut rng)).collect();
        
        let mean: f64 = samples.iter().map(|&x| x as f64).sum::<f64>() / samples.len() as f64;
        
        // Mean should be close to base latency
        assert!(mean > 5.0 && mean < 20.0, "Mean latency {} should be close to base 10ms", mean);
        
        // Should have some variation
        let min = *samples.iter().min().unwrap();
        let max = *samples.iter().max().unwrap();
        assert!(max > min, "Should have variation in samples");
    }
    
    #[test]
    fn test_latency_spikes() {
        let config = VariableLatencyConfig {
            spike_probability: 0.5, // 50% for testing
            ..Default::default()
        };
        let mut rng = thread_rng();
        
        let samples: Vec<u64> = (0..1000).map(|_| config.sample_latency(&mut rng)).collect();
        
        // With 50% spike probability, should have some high values
        let high_latency_count = samples.iter().filter(|&&x| x > 20).count();
        assert!(high_latency_count > 100, "Should have spikes with 50% probability");
    }
    
    #[test]
    fn test_latency_bounds() {
        let config = VariableLatencyConfig {
            min_latency_ms: 5,
            max_latency_ms: 100,
            ..Default::default()
        };
        let mut rng = thread_rng();
        
        for _ in 0..1000 {
            let latency = config.sample_latency(&mut rng);
            assert!(latency >= 5, "Latency {} should be >= min 5", latency);
            assert!(latency <= 100, "Latency {} should be <= max 100", latency);
        }
    }
    
    #[test]
    fn test_autocorrelated_latency() {
        let config = VariableLatencyConfig::default();
        let mut simulator = LatencySimulator::new(config);
        let mut rng = thread_rng();
        
        let samples: Vec<u64> = (0..100).map(|_| simulator.sample_with_autocorrelation(&mut rng)).collect();
        
        // Should have temporal correlation - consecutive values shouldn't jump wildly
        let mut big_jumps = 0;
        for i in 1..samples.len() {
            let diff = (samples[i] as i64 - samples[i-1] as i64).abs();
            if diff > 30 {
                big_jumps += 1;
            }
        }
        
        // Most transitions should be smooth
        assert!(big_jumps < 20, "Should have mostly smooth transitions, got {} big jumps", big_jumps);
    }
}
