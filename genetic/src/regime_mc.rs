//! Regime-Aware Monte Carlo Simulation
//!
//! Provides statistically valid Monte Carlo simulations that respect market regimes.
//! Unlike simple shuffling, this approach samples from similar volatility regimes
//! to maintain realistic return distributions.

use rand::prelude::*;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for regime-aware Monte Carlo simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeMCConfig {
    /// Number of Monte Carlo iterations
    pub num_iterations: usize,
    /// Number of regimes to identify (typically 2-4)
    pub num_regimes: usize,
    /// Window size for volatility calculation
    pub volatility_window: usize,
    /// Whether to use block bootstrap (preserves autocorrelation)
    pub use_block_bootstrap: bool,
    /// Block size for block bootstrap
    pub block_size: usize,
    /// Minimum samples per regime for validity
    pub min_samples_per_regime: usize,
}

impl Default for RegimeMCConfig {
    fn default() -> Self {
        Self {
            num_iterations: 1000,
            num_regimes: 3,
            volatility_window: 20,
            use_block_bootstrap: true,
            block_size: 5,
            min_samples_per_regime: 30,
        }
    }
}

/// Identified market regime
#[derive(Debug, Clone)]
pub struct MarketRegime {
    /// Regime identifier (0, 1, 2, ...)
    pub id: usize,
    /// Average volatility in this regime
    pub mean_volatility: f64,
    /// Volatility boundaries [lower, upper]
    pub volatility_bounds: (f64, f64),
    /// Indices of returns belonging to this regime
    pub member_indices: Vec<usize>,
    /// Returns in this regime
    pub returns: Vec<f64>,
}

/// Result of regime-aware Monte Carlo simulation
#[derive(Debug, Clone, Default)]
pub struct RegimeMCResult {
    /// Original metric value
    pub original_value: f64,
    /// Mean across simulations
    pub mc_mean: f64,
    /// Standard deviation across simulations
    pub mc_std: f64,
    /// 5th percentile (lower confidence bound)
    pub percentile_5: f64,
    /// 25th percentile
    pub percentile_25: f64,
    /// 50th percentile (median)
    pub percentile_50: f64,
    /// 75th percentile
    pub percentile_75: f64,
    /// 95th percentile (upper confidence bound)
    pub percentile_95: f64,
    /// Probability of achieving worse than original
    pub probability_worse: f64,
    /// Identified regimes
    pub regimes: Vec<RegimeInfo>,
}

/// Summary info about identified regimes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegimeInfo {
    pub id: usize,
    pub mean_volatility: f64,
    pub sample_count: usize,
    pub mean_return: f64,
}

/// Regime-aware Monte Carlo simulator
pub struct RegimeAwareMC {
    config: RegimeMCConfig,
    regimes: Vec<MarketRegime>,
    regime_sequence: Vec<usize>, // Original sequence of regimes
}

impl RegimeAwareMC {
    /// Create new simulator with given configuration
    pub fn new(config: RegimeMCConfig) -> Self {
        Self {
            config,
            regimes: Vec::new(),
            regime_sequence: Vec::new(),
        }
    }
    
    /// Identify regimes from returns based on rolling volatility
    pub fn identify_regimes(&mut self, returns: &[f64]) {
        if returns.len() < self.config.volatility_window + self.config.min_samples_per_regime {
            // Not enough data - use single regime
            self.regimes = vec![MarketRegime {
                id: 0,
                mean_volatility: calculate_volatility(returns),
                volatility_bounds: (0.0, f64::MAX),
                member_indices: (0..returns.len()).collect(),
                returns: returns.to_vec(),
            }];
            self.regime_sequence = vec![0; returns.len()];
            return;
        }
        
        // Calculate rolling volatility
        let mut volatilities: Vec<f64> = Vec::with_capacity(returns.len());
        for i in 0..returns.len() {
            if i < self.config.volatility_window {
                volatilities.push(f64::NAN);
            } else {
                let window = &returns[i - self.config.volatility_window..i];
                volatilities.push(calculate_volatility(window));
            }
        }
        
        // Filter out NaN values for regime identification
        let valid_vols: Vec<(usize, f64)> = volatilities.iter()
            .enumerate()
            .filter(|(_, v)| !v.is_nan())
            .map(|(i, &v)| (i, v))
            .collect();
        
        if valid_vols.is_empty() {
            // Fallback to single regime
            self.regimes = vec![MarketRegime {
                id: 0,
                mean_volatility: calculate_volatility(returns),
                volatility_bounds: (0.0, f64::MAX),
                member_indices: (0..returns.len()).collect(),
                returns: returns.to_vec(),
            }];
            self.regime_sequence = vec![0; returns.len()];
            return;
        }
        
        // Use quantile-based regime classification
        let regime_boundaries = self.calculate_regime_boundaries(&valid_vols);
        
        // Assign each return to a regime based on volatility
        self.regime_sequence = vec![0; returns.len()];
        let mut regime_members: HashMap<usize, Vec<usize>> = HashMap::new();
        
        for (idx, vol) in volatilities.iter().enumerate() {
            let regime_id = if vol.is_nan() {
                0 // Default to first regime for early samples
            } else {
                self.classify_regime(*vol, &regime_boundaries)
            };
            
            self.regime_sequence[idx] = regime_id;
            regime_members.entry(regime_id).or_insert_with(Vec::new).push(idx);
        }
        
        // Build regime structures
        self.regimes = (0..self.config.num_regimes)
            .map(|id| {
                let members = regime_members.get(&id).cloned().unwrap_or_default();
                let member_returns: Vec<f64> = members.iter().map(|&i| returns[i]).collect();
                let member_vols: Vec<f64> = members.iter()
                    .filter_map(|&i| {
                        let v = volatilities[i];
                        if v.is_nan() { None } else { Some(v) }
                    })
                    .collect();
                
                let mean_vol = if member_vols.is_empty() {
                    0.0
                } else {
                    member_vols.iter().sum::<f64>() / member_vols.len() as f64
                };
                
                let bounds = if id == 0 {
                    (0.0, *regime_boundaries.get(0).unwrap_or(&f64::MAX))
                } else if id == self.config.num_regimes - 1 {
                    (*regime_boundaries.get(id - 1).unwrap_or(&0.0), f64::MAX)
                } else {
                    (
                        *regime_boundaries.get(id - 1).unwrap_or(&0.0),
                        *regime_boundaries.get(id).unwrap_or(&f64::MAX),
                    )
                };
                
                MarketRegime {
                    id,
                    mean_volatility: mean_vol,
                    volatility_bounds: bounds,
                    member_indices: members,
                    returns: member_returns,
                }
            })
            .collect();
    }
    
    /// Calculate regime boundaries using quantiles
    fn calculate_regime_boundaries(&self, valid_vols: &[(usize, f64)]) -> Vec<f64> {
        let mut sorted_vols: Vec<f64> = valid_vols.iter().map(|(_, v)| *v).collect();
        sorted_vols.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let n = sorted_vols.len();
        let mut boundaries = Vec::with_capacity(self.config.num_regimes - 1);
        
        for i in 1..self.config.num_regimes {
            let quantile_pos = (i as f64 / self.config.num_regimes as f64 * n as f64) as usize;
            let quantile_pos = quantile_pos.min(n - 1);
            boundaries.push(sorted_vols[quantile_pos]);
        }
        
        boundaries
    }
    
    /// Classify a volatility value into a regime
    fn classify_regime(&self, volatility: f64, boundaries: &[f64]) -> usize {
        for (i, &boundary) in boundaries.iter().enumerate() {
            if volatility <= boundary {
                return i;
            }
        }
        boundaries.len() // Last regime
    }
    
    /// Run regime-aware Monte Carlo simulation on Sharpe ratio
    pub fn run_sharpe_simulation(&self, returns: &[f64]) -> RegimeMCResult {
        if returns.is_empty() {
            return RegimeMCResult::default();
        }
        
        // Calculate original Sharpe
        let original_sharpe = calculate_sharpe(returns);
        
        // Run Monte Carlo simulations in parallel
        let simulated_sharpes: Vec<f64> = (0..self.config.num_iterations)
            .into_par_iter()
            .map(|_| {
                let simulated_returns = self.generate_regime_aware_sample(returns);
                calculate_sharpe(&simulated_returns)
            })
            .collect();
        
        self.build_result(original_sharpe, simulated_sharpes)
    }
    
    /// Generate a regime-aware sample of returns
    fn generate_regime_aware_sample(&self, original_returns: &[f64]) -> Vec<f64> {
        let mut rng = rand::thread_rng();
        let mut result = Vec::with_capacity(original_returns.len());
        
        if self.config.use_block_bootstrap {
            // Block bootstrap: sample blocks while respecting regime sequence
            let mut i = 0;
            while i < self.regime_sequence.len() {
                let current_regime = self.regime_sequence[i];
                let regime = &self.regimes[current_regime];
                
                if regime.returns.is_empty() {
                    // Fallback to random return
                    result.push(*original_returns.choose(&mut rng).unwrap_or(&0.0));
                    i += 1;
                    continue;
                }
                
                // Sample a block from this regime
                let block_start = rng.gen_range(0..regime.returns.len());
                let block_len = self.config.block_size.min(regime.returns.len() - block_start);
                
                for j in 0..block_len {
                    if i + j >= original_returns.len() {
                        break;
                    }
                    let idx = (block_start + j) % regime.returns.len();
                    result.push(regime.returns[idx]);
                }
                
                i += block_len;
            }
        } else {
            // Simple regime-aware sampling
            for &regime_id in &self.regime_sequence {
                let regime = &self.regimes[regime_id];
                if regime.returns.is_empty() {
                    result.push(*original_returns.choose(&mut rng).unwrap_or(&0.0));
                } else {
                    result.push(*regime.returns.choose(&mut rng).unwrap());
                }
            }
        }
        
        // Ensure we have the right length
        result.truncate(original_returns.len());
        while result.len() < original_returns.len() {
            result.push(*original_returns.choose(&mut rng).unwrap_or(&0.0));
        }
        
        result
    }
    
    /// Build result from simulated values
    fn build_result(&self, original_value: f64, mut simulated_values: Vec<f64>) -> RegimeMCResult {
        if simulated_values.is_empty() {
            return RegimeMCResult::default();
        }
        
        simulated_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let n = simulated_values.len();
        let mean: f64 = simulated_values.iter().sum::<f64>() / n as f64;
        let variance: f64 = simulated_values.iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>() / (n - 1).max(1) as f64;
        let std = variance.sqrt();
        
        let percentile = |p: f64| -> f64 {
            let idx = ((p / 100.0) * n as f64) as usize;
            simulated_values[idx.min(n - 1)]
        };
        
        let worse_count = simulated_values.iter().filter(|&&v| v < original_value).count();
        
        let regime_info: Vec<RegimeInfo> = self.regimes.iter()
            .map(|r| {
                let mean_return = if r.returns.is_empty() {
                    0.0
                } else {
                    r.returns.iter().sum::<f64>() / r.returns.len() as f64
                };
                RegimeInfo {
                    id: r.id,
                    mean_volatility: r.mean_volatility,
                    sample_count: r.returns.len(),
                    mean_return,
                }
            })
            .collect();
        
        RegimeMCResult {
            original_value,
            mc_mean: mean,
            mc_std: std,
            percentile_5: percentile(5.0),
            percentile_25: percentile(25.0),
            percentile_50: percentile(50.0),
            percentile_75: percentile(75.0),
            percentile_95: percentile(95.0),
            probability_worse: worse_count as f64 / n as f64,
            regimes: regime_info,
        }
    }
    
    /// Get identified regimes
    pub fn get_regimes(&self) -> &[MarketRegime] {
        &self.regimes
    }
}

/// Calculate volatility (standard deviation of returns)
fn calculate_volatility(returns: &[f64]) -> f64 {
    if returns.len() < 2 {
        return 0.0;
    }
    
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance: f64 = returns.iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / (returns.len() - 1) as f64;
    
    variance.sqrt()
}

/// Calculate Sharpe ratio (annualized)
fn calculate_sharpe(returns: &[f64]) -> f64 {
    if returns.len() < 2 {
        return 0.0;
    }
    
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let std = calculate_volatility(returns);
    
    if std > f64::EPSILON {
        mean / std * 252.0_f64.sqrt()
    } else {
        0.0
    }
}

/// Convenience function to run regime-aware Monte Carlo
pub fn run_regime_aware_monte_carlo(
    returns: &[f64],
    config: Option<RegimeMCConfig>,
) -> RegimeMCResult {
    let config = config.unwrap_or_default();
    let mut simulator = RegimeAwareMC::new(config);
    simulator.identify_regimes(returns);
    simulator.run_sharpe_simulation(returns)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_regime_identification() {
        let mut returns = Vec::new();
        
        // Low volatility regime
        for _ in 0..50 {
            returns.push(0.001 + (rand::random::<f64>() - 0.5) * 0.002);
        }
        
        // High volatility regime
        for _ in 0..50 {
            returns.push(0.002 + (rand::random::<f64>() - 0.5) * 0.02);
        }
        
        // Back to low volatility
        for _ in 0..50 {
            returns.push(0.001 + (rand::random::<f64>() - 0.5) * 0.002);
        }
        
        let config = RegimeMCConfig {
            num_regimes: 2,
            volatility_window: 10,
            min_samples_per_regime: 10,
            ..Default::default()
        };
        
        let mut simulator = RegimeAwareMC::new(config);
        simulator.identify_regimes(&returns);
        
        assert_eq!(simulator.regimes.len(), 2);
    }
    
    #[test]
    fn test_monte_carlo_simulation() {
        let returns: Vec<f64> = (0..200)
            .map(|_| 0.001 + (rand::random::<f64>() - 0.5) * 0.01)
            .collect();
        
        let config = RegimeMCConfig {
            num_iterations: 100,
            num_regimes: 2,
            ..Default::default()
        };
        
        let result = run_regime_aware_monte_carlo(&returns, Some(config));
        
        assert!(result.mc_std > 0.0, "Should have variation in MC results");
        assert!(result.percentile_5 <= result.percentile_50);
        assert!(result.percentile_50 <= result.percentile_95);
    }
    
    #[test]
    fn test_block_bootstrap() {
        let returns: Vec<f64> = (0..100)
            .map(|i| 0.001 * (i as f64 / 10.0).sin())
            .collect();
        
        let config = RegimeMCConfig {
            num_iterations: 50,
            use_block_bootstrap: true,
            block_size: 5,
            ..Default::default()
        };
        
        let mut simulator = RegimeAwareMC::new(config);
        simulator.identify_regimes(&returns);
        
        let sample = simulator.generate_regime_aware_sample(&returns);
        assert_eq!(sample.len(), returns.len());
    }
}
