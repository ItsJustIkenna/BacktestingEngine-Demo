//! Adaptive Sampling for Genetic Algorithm Optimization
//!
//! Provides data sampling strategies that adapt based on the GA generation:
//! - Early generations: Coarser sampling for faster exploration
//! - Later generations: Full resolution for precise fine-tuning
//!
//! This is intended to reduce run time by cutting iterations in early generations
//! while keeping full resolution for final tuning (not benchmarked in this repo).

use dataloader::MarketData;

/// Configuration for adaptive sampling
#[derive(Debug, Clone)]
pub struct AdaptiveSamplingConfig {
    /// Number of generations to use full resolution (last N generations)
    pub full_resolution_generations: usize,
    /// Minimum sample rate (1 = every tick, 10 = every 10th tick)
    pub min_sample_rate: usize,
    /// Maximum sample rate for early exploration
    pub max_sample_rate: usize,
    /// Whether adaptive sampling is enabled
    pub enabled: bool,
}

impl Default for AdaptiveSamplingConfig {
    fn default() -> Self {
        Self {
            full_resolution_generations: 3, // Last 3 generations use full data
            min_sample_rate: 1,             // No sampling (full data)
            max_sample_rate: 5,             // Sample every 5th tick in early gens
            enabled: true,
        }
    }
}

impl AdaptiveSamplingConfig {
    /// Create a disabled config (always use full data)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Calculate sample rate for a given generation
    /// 
    /// Returns the sampling rate (1 = every tick, N = every Nth tick)
    /// 
    /// # Arguments
    /// * `generation` - Current generation (0-indexed)
    /// * `total_generations` - Total number of generations
    pub fn sample_rate_for_generation(&self, generation: usize, total_generations: usize) -> usize {
        if !self.enabled {
            return 1;
        }

        // Last N generations always use full resolution
        let generations_remaining = total_generations.saturating_sub(generation + 1);
        if generations_remaining < self.full_resolution_generations {
            return self.min_sample_rate;
        }

        // Calculate progress through exploration phase
        let exploration_generations = total_generations.saturating_sub(self.full_resolution_generations);
        if exploration_generations == 0 {
            return self.min_sample_rate;
        }

        // Linear interpolation from max_sample_rate to min_sample_rate
        // Early generations use more aggressive sampling
        let progress = generation as f64 / exploration_generations as f64;
        let rate = self.max_sample_rate as f64 - 
            progress * (self.max_sample_rate - self.min_sample_rate) as f64;
        
        (rate.round() as usize).max(self.min_sample_rate)
    }

    /// Check if this generation should use full resolution
    pub fn is_full_resolution(&self, generation: usize, total_generations: usize) -> bool {
        self.sample_rate_for_generation(generation, total_generations) == 1
    }
}

/// Sample market data based on generation
/// 
/// Returns a view or sampled copy of the data depending on sample rate.
/// For rate=1, returns a reference to avoid allocation.
pub fn sample_market_data<'a>(
    data: &'a [MarketData],
    sample_rate: usize,
) -> SampledData<'a> {
    if sample_rate <= 1 {
        return SampledData::Reference(data);
    }

    // Sample every Nth element
    let sampled: Vec<MarketData> = data
        .iter()
        .step_by(sample_rate)
        .cloned()
        .collect();

    SampledData::Owned(sampled)
}

/// Result of sampling - either a reference (zero-copy) or owned sampled data
pub enum SampledData<'a> {
    /// Direct reference to original data (no sampling)
    Reference(&'a [MarketData]),
    /// Sampled copy of data
    Owned(Vec<MarketData>),
}

impl<'a> SampledData<'a> {
    /// Get a slice of the data
    pub fn as_slice(&self) -> &[MarketData] {
        match self {
            SampledData::Reference(data) => data,
            SampledData::Owned(data) => data.as_slice(),
        }
    }

    /// Get length of data
    pub fn len(&self) -> usize {
        match self {
            SampledData::Reference(data) => data.len(),
            SampledData::Owned(data) => data.len(),
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Stable region detector for skipping unchanged market periods
/// 
/// Tracks price changes and identifies regions where the market is stable,
/// allowing the simulation to skip unnecessary strategy recalculations.
#[derive(Debug, Clone)]
pub struct StableRegionDetector {
    /// Minimum price change (as ratio) to trigger strategy update
    pub price_threshold: f64,
    /// Maximum ticks to skip between forced updates
    pub max_skip_ticks: usize,
    /// Last price that triggered an update
    last_trigger_price: f64,
    /// Ticks since last update
    ticks_since_update: usize,
}

impl Default for StableRegionDetector {
    fn default() -> Self {
        Self {
            price_threshold: 0.0001,  // 0.01% = 1 basis point
            max_skip_ticks: 100,      // Force update every 100 ticks minimum
            last_trigger_price: 0.0,
            ticks_since_update: 0,
        }
    }
}

impl StableRegionDetector {
    /// Create a new detector with custom thresholds
    pub fn new(price_threshold: f64, max_skip_ticks: usize) -> Self {
        Self {
            price_threshold,
            max_skip_ticks,
            last_trigger_price: 0.0,
            ticks_since_update: 0,
        }
    }

    /// Disabled detector that always triggers updates
    pub fn disabled() -> Self {
        Self {
            price_threshold: 0.0,
            max_skip_ticks: 1,
            last_trigger_price: 0.0,
            ticks_since_update: 0,
        }
    }

    /// Check if we should process this tick
    /// 
    /// Returns true if strategy should be updated, false if we can skip
    #[inline]
    pub fn should_process(&mut self, current_price: f64) -> bool {
        self.ticks_since_update += 1;

        // First tick always processes
        if self.last_trigger_price == 0.0 {
            self.last_trigger_price = current_price;
            self.ticks_since_update = 0;
            return true;
        }

        // Force update after max_skip_ticks
        if self.ticks_since_update >= self.max_skip_ticks {
            self.last_trigger_price = current_price;
            self.ticks_since_update = 0;
            return true;
        }

        // Check price change threshold
        let price_change = (current_price - self.last_trigger_price).abs() / self.last_trigger_price;
        if price_change >= self.price_threshold {
            self.last_trigger_price = current_price;
            self.ticks_since_update = 0;
            return true;
        }

        false
    }

    /// Reset the detector state
    pub fn reset(&mut self) {
        self.last_trigger_price = 0.0;
        self.ticks_since_update = 0;
    }

    /// Get statistics about skipping efficiency
    pub fn skip_ratio(&self) -> f64 {
        // This would need external tracking; placeholder for now
        0.0
    }
}

/// Dynamic concurrency calculator based on available memory
/// 
/// Determines optimal number of concurrent evaluations based on
/// system memory and per-evaluation memory requirements.
#[derive(Debug, Clone)]
pub struct DynamicConcurrency {
    /// Memory per evaluation in GB
    pub memory_per_eval_gb: f64,
    /// Target memory usage as fraction of available (0.75 = 75%)
    pub target_memory_fraction: f64,
    /// Minimum concurrent evaluations (floor)
    pub min_concurrent: usize,
    /// Maximum concurrent evaluations (ceiling)
    pub max_concurrent: usize,
}

impl Default for DynamicConcurrency {
    fn default() -> Self {
        Self {
            memory_per_eval_gb: 2.0,    // ~2GB per evaluation
            target_memory_fraction: 0.7, // Use 70% of available memory
            min_concurrent: 2,           // At least 2 concurrent
            max_concurrent: 16,          // Cap at 16 for most systems
        }
    }
}

impl DynamicConcurrency {
    /// Calculate optimal concurrent evaluations based on available memory
    /// 
    /// # Arguments
    /// * `available_memory_gb` - Available system memory in GB
    pub fn calculate_concurrent(&self, available_memory_gb: f64) -> usize {
        let usable_memory = available_memory_gb * self.target_memory_fraction;
        let optimal = (usable_memory / self.memory_per_eval_gb).floor() as usize;
        optimal.clamp(self.min_concurrent, self.max_concurrent)
    }

    /// Get concurrent evaluations from environment or calculate from memory
    /// 
    /// Checks MAX_CONCURRENT_EVALS env var first, then calculates dynamically
    pub fn get_concurrent_evals(&self) -> usize {
        // Check environment variable first
        if let Ok(val) = std::env::var("MAX_CONCURRENT_EVALS") {
            if let Ok(n) = val.parse::<usize>() {
                return n.clamp(self.min_concurrent, self.max_concurrent);
            }
        }

        // Check POD_MEMORY_LIMIT_GB for Kubernetes environments
        if let Ok(val) = std::env::var("POD_MEMORY_LIMIT_GB") {
            if let Ok(mem) = val.parse::<f64>() {
                return self.calculate_concurrent(mem);
            }
        }

        // Default to 6 for 32GB pods (standard configuration)
        6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_rate_for_generation() {
        let config = AdaptiveSamplingConfig::default();
        
        // With 15 generations, last 3 should be full resolution
        assert_eq!(config.sample_rate_for_generation(14, 15), 1); // Last gen
        assert_eq!(config.sample_rate_for_generation(13, 15), 1); // Second to last
        assert_eq!(config.sample_rate_for_generation(12, 15), 1); // Third to last
        
        // Early generations should have higher sample rates
        let early_rate = config.sample_rate_for_generation(0, 15);
        let mid_rate = config.sample_rate_for_generation(6, 15);
        assert!(early_rate >= mid_rate);
    }

    #[test]
    fn test_stable_region_detector() {
        let mut detector = StableRegionDetector::new(0.01, 10); // 1% threshold, max 10 skip
        
        // First tick always processes
        assert!(detector.should_process(100.0));
        
        // Small change - skip
        assert!(!detector.should_process(100.5)); // 0.5% < 1%
        
        // Large change - process
        assert!(detector.should_process(102.0)); // 2% > 1% (from last trigger at 100.0)
    }

    #[test]
    fn test_dynamic_concurrency() {
        let calc = DynamicConcurrency::default();
        
        // 32GB pod should give ~11 concurrent (32 * 0.7 / 2 = 11.2)
        assert_eq!(calc.calculate_concurrent(32.0), 11);
        
        // 16GB pod should give ~5 concurrent
        assert_eq!(calc.calculate_concurrent(16.0), 5);
        
        // 64GB pod should give 16 (capped at max)
        assert_eq!(calc.calculate_concurrent(64.0), 16);
    }
}
