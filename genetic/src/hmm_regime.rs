//! Hidden Markov Model (HMM) Regime Detection
//!
//! Provides probabilistic regime detection using Hidden Markov Models.
//! Unlike simple volatility thresholds, HMM captures:
//! - Regime persistence (markets tend to stay in regimes)
//! - Smooth transitions between regimes
//! - Probabilistic state assignment (uncertainty quantification)
//! - Forward-looking regime prediction

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Number of regimes in the HMM (configurable but typically 2-4)
const DEFAULT_NUM_REGIMES: usize = 3;

/// HMM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HMMConfig {
    /// Number of hidden states (regimes)
    pub num_regimes: usize,
    /// Maximum iterations for Baum-Welch algorithm
    pub max_iterations: usize,
    /// Convergence threshold for log-likelihood
    pub convergence_threshold: f64,
    /// Minimum probability floor (prevents numerical issues)
    pub min_probability: f64,
    /// Minimum variance floor for emissions
    pub min_variance: f64,
}

impl Default for HMMConfig {
    fn default() -> Self {
        Self {
            num_regimes: DEFAULT_NUM_REGIMES,
            max_iterations: 100,
            convergence_threshold: 1e-6,
            min_probability: 1e-10,
            min_variance: 1e-8,
        }
    }
}

/// Gaussian emission parameters for each regime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianEmission {
    /// Mean return in this regime
    pub mean: f64,
    /// Variance of returns in this regime
    pub variance: f64,
}

impl GaussianEmission {
    /// Calculate probability density at observation x
    pub fn pdf(&self, x: f64) -> f64 {
        let std_dev = self.variance.sqrt();
        if std_dev < 1e-10 {
            return if (x - self.mean).abs() < 1e-10 { 1.0 } else { 0.0 };
        }
        let z = (x - self.mean) / std_dev;
        (1.0 / (std_dev * (2.0 * PI).sqrt())) * (-0.5 * z * z).exp()
    }
    
    /// Calculate log probability density
    pub fn log_pdf(&self, x: f64) -> f64 {
        let std_dev = self.variance.sqrt();
        if std_dev < 1e-10 {
            return if (x - self.mean).abs() < 1e-10 { 0.0 } else { f64::NEG_INFINITY };
        }
        let z = (x - self.mean) / std_dev;
        -0.5 * (2.0 * PI).ln() - std_dev.ln() - 0.5 * z * z
    }
}

/// Hidden Markov Model for regime detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenMarkovModel {
    /// Configuration
    pub config: HMMConfig,
    /// Initial state probabilities π[i] = P(state_0 = i)
    pub initial_probs: Vec<f64>,
    /// Transition matrix A[i][j] = P(state_t+1 = j | state_t = i)
    pub transition_matrix: Vec<Vec<f64>>,
    /// Emission parameters for each state (Gaussian)
    pub emissions: Vec<GaussianEmission>,
    /// Regime labels for interpretation
    pub regime_labels: Vec<String>,
    /// Whether model has been fitted
    pub is_fitted: bool,
    /// Log-likelihood of last fit
    pub log_likelihood: f64,
}

impl HiddenMarkovModel {
    /// Create a new HMM with given number of regimes
    pub fn new(config: HMMConfig) -> Self {
        let n = config.num_regimes;
        
        // Initialize uniform initial probabilities
        let initial_probs = vec![1.0 / n as f64; n];
        
        // Initialize transition matrix with slight self-persistence bias
        // Markets tend to stay in regimes (persistence)
        let mut transition_matrix = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    transition_matrix[i][j] = 0.7; // 70% chance to stay
                } else {
                    transition_matrix[i][j] = 0.3 / (n - 1) as f64; // Split remaining
                }
            }
        }
        
        // Initialize emissions with spread-out means
        let emissions: Vec<GaussianEmission> = (0..n)
            .map(|i| {
                let mean = (i as f64 - (n as f64 - 1.0) / 2.0) * 0.001; // Spread means
                GaussianEmission {
                    mean,
                    variance: 0.0001 * (i + 1) as f64, // Increasing variance
                }
            })
            .collect();
        
        // Default regime labels
        let regime_labels = match n {
            2 => vec!["Low Volatility".to_string(), "High Volatility".to_string()],
            3 => vec![
                "Low Volatility".to_string(),
                "Normal".to_string(),
                "High Volatility".to_string(),
            ],
            4 => vec![
                "Low Volatility".to_string(),
                "Normal".to_string(),
                "High Volatility".to_string(),
                "Crisis".to_string(),
            ],
            _ => (0..n).map(|i| format!("Regime {}", i)).collect(),
        };
        
        Self {
            config,
            initial_probs,
            transition_matrix,
            emissions,
            regime_labels,
            is_fitted: false,
            log_likelihood: f64::NEG_INFINITY,
        }
    }
    
    /// Fit the HMM to observed returns using Baum-Welch (EM) algorithm
    pub fn fit(&mut self, observations: &[f64]) -> Result<FitResult, String> {
        if observations.len() < 10 {
            return Err("Need at least 10 observations to fit HMM".to_string());
        }
        
        let _n = self.config.num_regimes;
        let _t = observations.len();
        
        // Initialize emissions based on data quantiles
        self.initialize_emissions_from_data(observations);
        
        let mut prev_log_likelihood = f64::NEG_INFINITY;
        let mut iterations = 0;
        
        for iter in 0..self.config.max_iterations {
            iterations = iter + 1;
            
            // E-step: Forward-backward algorithm
            let (alpha, scale_factors) = self.forward(observations);
            let beta = self.backward(observations, &scale_factors);
            
            // Calculate log-likelihood
            let log_likelihood: f64 = scale_factors.iter()
                .map(|&s| if s > 0.0 { s.ln() } else { f64::NEG_INFINITY })
                .sum();
            
            // Check convergence
            if (log_likelihood - prev_log_likelihood).abs() < self.config.convergence_threshold {
                self.log_likelihood = log_likelihood;
                self.is_fitted = true;
                return Ok(FitResult {
                    log_likelihood,
                    iterations,
                    converged: true,
                });
            }
            prev_log_likelihood = log_likelihood;
            
            // Calculate gamma and xi
            let gamma = self.calculate_gamma(&alpha, &beta);
            let xi = self.calculate_xi(observations, &alpha, &beta);
            
            // M-step: Update parameters
            self.update_initial_probs(&gamma);
            self.update_transition_matrix(&gamma, &xi);
            self.update_emissions(observations, &gamma);
        }
        
        self.log_likelihood = prev_log_likelihood;
        self.is_fitted = true;
        
        Ok(FitResult {
            log_likelihood: prev_log_likelihood,
            iterations,
            converged: false,
        })
    }
    
    /// Initialize emissions based on data quantiles
    fn initialize_emissions_from_data(&mut self, observations: &[f64]) {
        let n = self.config.num_regimes;
        let mut sorted = observations.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        
        let overall_mean: f64 = observations.iter().sum::<f64>() / observations.len() as f64;
        let overall_var: f64 = observations.iter()
            .map(|x| (x - overall_mean).powi(2))
            .sum::<f64>() / observations.len() as f64;
        
        for i in 0..n {
            // Assign quantile-based means
            let quantile = (i as f64 + 0.5) / n as f64;
            let idx = ((quantile * sorted.len() as f64) as usize).min(sorted.len() - 1);
            
            // Calculate mean around this quantile
            let window_start = (idx as i64 - 5).max(0) as usize;
            let window_end = (idx + 5).min(sorted.len());
            let window: Vec<f64> = sorted[window_start..window_end].to_vec();
            
            let mean = window.iter().sum::<f64>() / window.len() as f64;
            
            // Variance increases with regime index (low vol to high vol)
            let variance = overall_var * (0.5 + i as f64 * 0.5);
            
            self.emissions[i] = GaussianEmission {
                mean,
                variance: variance.max(self.config.min_variance),
            };
        }
    }
    
    /// Forward algorithm (scaled to prevent underflow)
    fn forward(&self, observations: &[f64]) -> (Vec<Vec<f64>>, Vec<f64>) {
        let n = self.config.num_regimes;
        let t = observations.len();
        
        let mut alpha = vec![vec![0.0; n]; t];
        let mut scale_factors = vec![0.0; t];
        
        // Initialize t=0
        for i in 0..n {
            alpha[0][i] = self.initial_probs[i] * self.emissions[i].pdf(observations[0]);
        }
        
        // Scale to prevent underflow
        scale_factors[0] = alpha[0].iter().sum();
        if scale_factors[0] > 0.0 {
            for i in 0..n {
                alpha[0][i] /= scale_factors[0];
            }
        }
        
        // Forward pass
        for t_idx in 1..t {
            for j in 0..n {
                let mut sum = 0.0;
                for i in 0..n {
                    sum += alpha[t_idx - 1][i] * self.transition_matrix[i][j];
                }
                alpha[t_idx][j] = sum * self.emissions[j].pdf(observations[t_idx]);
            }
            
            // Scale
            scale_factors[t_idx] = alpha[t_idx].iter().sum();
            if scale_factors[t_idx] > 0.0 {
                for i in 0..n {
                    alpha[t_idx][i] /= scale_factors[t_idx];
                }
            }
        }
        
        (alpha, scale_factors)
    }
    
    /// Backward algorithm (scaled)
    fn backward(&self, observations: &[f64], scale_factors: &[f64]) -> Vec<Vec<f64>> {
        let n = self.config.num_regimes;
        let t = observations.len();
        
        let mut beta = vec![vec![0.0; n]; t];
        
        // Initialize t=T-1
        for i in 0..n {
            beta[t - 1][i] = 1.0;
        }
        
        // Backward pass
        for t_idx in (0..t - 1).rev() {
            for i in 0..n {
                let mut sum = 0.0;
                for j in 0..n {
                    sum += self.transition_matrix[i][j]
                        * self.emissions[j].pdf(observations[t_idx + 1])
                        * beta[t_idx + 1][j];
                }
                beta[t_idx][i] = sum;
            }
            
            // Scale using same factors as forward
            if scale_factors[t_idx + 1] > 0.0 {
                for i in 0..n {
                    beta[t_idx][i] /= scale_factors[t_idx + 1];
                }
            }
        }
        
        beta
    }
    
    /// Calculate gamma: P(state_t = i | observations)
    fn calculate_gamma(&self, alpha: &[Vec<f64>], beta: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let n = self.config.num_regimes;
        let t = alpha.len();
        
        let mut gamma = vec![vec![0.0; n]; t];
        
        for t_idx in 0..t {
            let mut sum = 0.0;
            for i in 0..n {
                gamma[t_idx][i] = alpha[t_idx][i] * beta[t_idx][i];
                sum += gamma[t_idx][i];
            }
            
            if sum > 0.0 {
                for i in 0..n {
                    gamma[t_idx][i] /= sum;
                }
            }
        }
        
        gamma
    }
    
    /// Calculate xi: P(state_t = i, state_t+1 = j | observations)
    fn calculate_xi(
        &self,
        observations: &[f64],
        alpha: &[Vec<f64>],
        beta: &[Vec<f64>],
    ) -> Vec<Vec<Vec<f64>>> {
        let n = self.config.num_regimes;
        let t = observations.len();
        
        let mut xi = vec![vec![vec![0.0; n]; n]; t - 1];
        
        for t_idx in 0..t - 1 {
            let mut sum = 0.0;
            
            for i in 0..n {
                for j in 0..n {
                    xi[t_idx][i][j] = alpha[t_idx][i]
                        * self.transition_matrix[i][j]
                        * self.emissions[j].pdf(observations[t_idx + 1])
                        * beta[t_idx + 1][j];
                    sum += xi[t_idx][i][j];
                }
            }
            
            if sum > 0.0 {
                for i in 0..n {
                    for j in 0..n {
                        xi[t_idx][i][j] /= sum;
                    }
                }
            }
        }
        
        xi
    }
    
    /// Update initial probabilities
    fn update_initial_probs(&mut self, gamma: &[Vec<f64>]) {
        let n = self.config.num_regimes;
        
        for i in 0..n {
            self.initial_probs[i] = gamma[0][i].max(self.config.min_probability);
        }
        
        // Normalize
        let sum: f64 = self.initial_probs.iter().sum();
        for i in 0..n {
            self.initial_probs[i] /= sum;
        }
    }
    
    /// Update transition matrix
    fn update_transition_matrix(&mut self, gamma: &[Vec<f64>], xi: &[Vec<Vec<f64>>]) {
        let n = self.config.num_regimes;
        let t = gamma.len();
        
        for i in 0..n {
            let gamma_sum: f64 = (0..t - 1).map(|t_idx| gamma[t_idx][i]).sum();
            
            for j in 0..n {
                let xi_sum: f64 = (0..t - 1).map(|t_idx| xi[t_idx][i][j]).sum();
                
                self.transition_matrix[i][j] = if gamma_sum > 0.0 {
                    (xi_sum / gamma_sum).max(self.config.min_probability)
                } else {
                    1.0 / n as f64
                };
            }
            
            // Normalize row
            let row_sum: f64 = self.transition_matrix[i].iter().sum();
            for j in 0..n {
                self.transition_matrix[i][j] /= row_sum;
            }
        }
    }
    
    /// Update emission parameters
    fn update_emissions(&mut self, observations: &[f64], gamma: &[Vec<f64>]) {
        let n = self.config.num_regimes;
        let t = observations.len();
        
        for i in 0..n {
            let gamma_sum: f64 = gamma.iter().map(|g| g[i]).sum();
            
            if gamma_sum > 0.0 {
                // Update mean
                let mean: f64 = (0..t)
                    .map(|t_idx| gamma[t_idx][i] * observations[t_idx])
                    .sum::<f64>()
                    / gamma_sum;
                
                // Update variance
                let variance: f64 = (0..t)
                    .map(|t_idx| gamma[t_idx][i] * (observations[t_idx] - mean).powi(2))
                    .sum::<f64>()
                    / gamma_sum;
                
                self.emissions[i] = GaussianEmission {
                    mean,
                    variance: variance.max(self.config.min_variance),
                };
            }
        }
    }
    
    /// Decode most likely state sequence using Viterbi algorithm
    pub fn decode(&self, observations: &[f64]) -> Vec<usize> {
        if !self.is_fitted || observations.is_empty() {
            return vec![];
        }
        
        let n = self.config.num_regimes;
        let t = observations.len();
        
        // Viterbi algorithm
        let mut delta = vec![vec![f64::NEG_INFINITY; n]; t];
        let mut psi = vec![vec![0usize; n]; t];
        
        // Initialize
        for i in 0..n {
            delta[0][i] = self.initial_probs[i].ln() + self.emissions[i].log_pdf(observations[0]);
        }
        
        // Forward pass
        for t_idx in 1..t {
            for j in 0..n {
                let mut max_val = f64::NEG_INFINITY;
                let mut max_state = 0;
                
                for i in 0..n {
                    let val = delta[t_idx - 1][i] + self.transition_matrix[i][j].ln();
                    if val > max_val {
                        max_val = val;
                        max_state = i;
                    }
                }
                
                delta[t_idx][j] = max_val + self.emissions[j].log_pdf(observations[t_idx]);
                psi[t_idx][j] = max_state;
            }
        }
        
        // Backtrack
        let mut states = vec![0; t];
        
        // Find best final state
        let mut max_val = f64::NEG_INFINITY;
        for i in 0..n {
            if delta[t - 1][i] > max_val {
                max_val = delta[t - 1][i];
                states[t - 1] = i;
            }
        }
        
        // Backtrack
        for t_idx in (0..t - 1).rev() {
            states[t_idx] = psi[t_idx + 1][states[t_idx + 1]];
        }
        
        states
    }
    
    /// Get current regime probabilities given observations
    pub fn get_regime_probabilities(&self, observations: &[f64]) -> Vec<f64> {
        if !self.is_fitted || observations.is_empty() {
            return vec![1.0 / self.config.num_regimes as f64; self.config.num_regimes];
        }
        
        let (alpha, _) = self.forward(observations);
        let last_alpha = &alpha[alpha.len() - 1];
        
        // Already normalized by forward algorithm
        last_alpha.clone()
    }
    
    /// Predict next regime probabilities
    pub fn predict_next_regime(&self, observations: &[f64]) -> Vec<f64> {
        let current_probs = self.get_regime_probabilities(observations);
        let n = self.config.num_regimes;
        
        let mut next_probs = vec![0.0; n];
        
        for j in 0..n {
            for i in 0..n {
                next_probs[j] += current_probs[i] * self.transition_matrix[i][j];
            }
        }
        
        next_probs
    }
    
    /// Get regime characteristics
    pub fn get_regime_characteristics(&self) -> Vec<RegimeCharacteristics> {
        self.emissions
            .iter()
            .enumerate()
            .map(|(i, emission)| RegimeCharacteristics {
                regime_id: i,
                label: self.regime_labels[i].clone(),
                mean_return: emission.mean,
                volatility: emission.variance.sqrt(),
                persistence: self.transition_matrix[i][i],
            })
            .collect()
    }
}

/// Result of HMM fitting
#[derive(Debug, Clone)]
pub struct FitResult {
    pub log_likelihood: f64,
    pub iterations: usize,
    pub converged: bool,
}

/// Characteristics of each regime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeCharacteristics {
    pub regime_id: usize,
    pub label: String,
    pub mean_return: f64,
    pub volatility: f64,
    pub persistence: f64, // P(stay in regime)
}

/// Real-time regime tracker using HMM
pub struct HMMRegimeTracker {
    hmm: HiddenMarkovModel,
    recent_returns: Vec<f64>,
    window_size: usize,
    current_regime: usize,
    regime_probabilities: Vec<f64>,
}

impl HMMRegimeTracker {
    pub fn new(hmm: HiddenMarkovModel, window_size: usize) -> Self {
        let n = hmm.config.num_regimes;
        Self {
            hmm,
            recent_returns: Vec::with_capacity(window_size),
            window_size,
            current_regime: 0,
            regime_probabilities: vec![1.0 / n as f64; n],
        }
    }
    
    /// Update with new return observation
    pub fn update(&mut self, return_value: f64) -> RegimeUpdate {
        self.recent_returns.push(return_value);
        
        if self.recent_returns.len() > self.window_size {
            self.recent_returns.remove(0);
        }
        
        // Get new probabilities
        self.regime_probabilities = self.hmm.get_regime_probabilities(&self.recent_returns);
        
        // Decode most likely current state
        let states = self.hmm.decode(&self.recent_returns);
        let new_regime = *states.last().unwrap_or(&0);
        
        let regime_changed = new_regime != self.current_regime;
        let previous_regime = self.current_regime;
        self.current_regime = new_regime;
        
        RegimeUpdate {
            current_regime: self.current_regime,
            regime_label: self.hmm.regime_labels[self.current_regime].clone(),
            regime_probabilities: self.regime_probabilities.clone(),
            regime_changed,
            previous_regime: if regime_changed { Some(previous_regime) } else { None },
            predicted_next_regime: self.hmm.predict_next_regime(&self.recent_returns),
        }
    }
    
    /// Get current regime
    pub fn current_regime(&self) -> usize {
        self.current_regime
    }
    
    /// Get regime probabilities
    pub fn regime_probabilities(&self) -> &[f64] {
        &self.regime_probabilities
    }
}

/// Update from regime tracker
#[derive(Debug, Clone)]
pub struct RegimeUpdate {
    pub current_regime: usize,
    pub regime_label: String,
    pub regime_probabilities: Vec<f64>,
    pub regime_changed: bool,
    pub previous_regime: Option<usize>,
    pub predicted_next_regime: Vec<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hmm_creation() {
        let config = HMMConfig::default();
        let hmm = HiddenMarkovModel::new(config);
        
        assert_eq!(hmm.config.num_regimes, 3);
        assert_eq!(hmm.emissions.len(), 3);
        assert_eq!(hmm.transition_matrix.len(), 3);
    }
    
    #[test]
    fn test_hmm_fitting() {
        let config = HMMConfig {
            num_regimes: 2,
            max_iterations: 50,
            ..Default::default()
        };
        let mut hmm = HiddenMarkovModel::new(config);
        
        // Generate synthetic data with regime switches
        let mut returns = Vec::new();
        
        // Low volatility regime
        for _ in 0..50 {
            returns.push(0.001 + (rand::random::<f64>() - 0.5) * 0.005);
        }
        
        // High volatility regime
        for _ in 0..50 {
            returns.push(-0.002 + (rand::random::<f64>() - 0.5) * 0.03);
        }
        
        // Back to low volatility
        for _ in 0..50 {
            returns.push(0.001 + (rand::random::<f64>() - 0.5) * 0.005);
        }
        
        let result = hmm.fit(&returns).unwrap();
        
        assert!(hmm.is_fitted);
        assert!(result.iterations > 0);
    }
    
    #[test]
    fn test_viterbi_decoding() {
        let config = HMMConfig {
            num_regimes: 2,
            max_iterations: 50,
            ..Default::default()
        };
        let mut hmm = HiddenMarkovModel::new(config);
        
        let mut returns = Vec::new();
        for _ in 0..30 {
            returns.push(0.001 + (rand::random::<f64>() - 0.5) * 0.005);
        }
        for _ in 0..30 {
            returns.push(-0.002 + (rand::random::<f64>() - 0.5) * 0.03);
        }
        
        hmm.fit(&returns).unwrap();
        
        let states = hmm.decode(&returns);
        
        assert_eq!(states.len(), returns.len());
        // States should be mostly consistent within each block
    }
    
    #[test]
    fn test_regime_tracker() {
        let config = HMMConfig {
            num_regimes: 2,
            max_iterations: 30,
            ..Default::default()
        };
        let mut hmm = HiddenMarkovModel::new(config);
        
        let training_data: Vec<f64> = (0..100)
            .map(|i| {
                if i < 50 {
                    0.001 + (rand::random::<f64>() - 0.5) * 0.005
                } else {
                    -0.001 + (rand::random::<f64>() - 0.5) * 0.02
                }
            })
            .collect();
        
        hmm.fit(&training_data).unwrap();
        
        let mut tracker = HMMRegimeTracker::new(hmm, 30);
        
        // Feed some observations
        for r in &training_data[0..20] {
            let update = tracker.update(*r);
            assert!(update.regime_probabilities.len() == 2);
        }
    }
}
