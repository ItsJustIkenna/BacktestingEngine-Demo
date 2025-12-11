//! Advanced metrics for professional quantitative analysis
//!
//! Includes Omega ratio, Tail ratio, Kelly criterion, and cross-asset correlation monitoring.

use std::collections::HashMap;

/// Calculates the Omega ratio at a given threshold return.
/// 
/// Omega = ∫(F(r) - threshold) dr for r > threshold / ∫(threshold - F(r)) dr for r < threshold
/// Simplified: Sum of gains above threshold / Sum of losses below threshold
/// 
/// Unlike Sharpe which assumes normal returns, Omega captures the full return distribution.
/// 
/// # Arguments
/// * `returns` - Vector of periodic returns
/// * `threshold` - Minimum acceptable return (often 0 or risk-free rate)
/// 
/// # Returns
/// Omega ratio where >1 indicates gains exceed losses above threshold
pub fn omega_ratio(returns: &[f64], threshold: f64) -> Option<f64> {
    if returns.is_empty() {
        return None;
    }
    
    let mut gains_sum = 0.0;  // Sum of (return - threshold) for returns > threshold
    let mut losses_sum = 0.0; // Sum of (threshold - return) for returns < threshold
    
    for &r in returns {
        if r > threshold {
            gains_sum += r - threshold;
        } else if r < threshold {
            losses_sum += threshold - r;
        }
    }
    
    if losses_sum.abs() < f64::EPSILON {
        // No losses - infinite Omega (cap at large value)
        if gains_sum > 0.0 {
            return Some(100.0);
        }
        return None; // No gains or losses
    }
    
    Some(gains_sum / losses_sum)
}

/// Calculates the Omega ratio with multiple thresholds for a complete picture.
/// Returns Omega at 0%, risk-free rate, and median return thresholds.
pub fn omega_ratio_suite(returns: &[f64], risk_free_rate: f64) -> OmegaRatioSuite {
    let median = if returns.is_empty() {
        0.0
    } else {
        let mut sorted = returns.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    };
    
    OmegaRatioSuite {
        omega_at_zero: omega_ratio(returns, 0.0),
        omega_at_rfr: omega_ratio(returns, risk_free_rate),
        omega_at_median: omega_ratio(returns, median),
        threshold_zero: 0.0,
        threshold_rfr: risk_free_rate,
        threshold_median: median,
    }
}

/// Suite of Omega ratios at different thresholds
#[derive(Debug, Clone, Default)]
pub struct OmegaRatioSuite {
    pub omega_at_zero: Option<f64>,
    pub omega_at_rfr: Option<f64>,
    pub omega_at_median: Option<f64>,
    pub threshold_zero: f64,
    pub threshold_rfr: f64,
    pub threshold_median: f64,
}

/// Calculates the Tail ratio - ratio of right tail gains to left tail losses.
/// 
/// Tail Ratio = 95th percentile / |5th percentile|
/// 
/// Measures asymmetry of extreme returns:
/// - > 1.0: Right tail is fatter (larger gains than losses in extremes)
/// - < 1.0: Left tail is fatter (larger losses than gains in extremes)
/// - = 1.0: Symmetric tails
/// 
/// # Arguments
/// * `returns` - Vector of periodic returns
/// * `tail_percentile` - Percentile for tail measurement (default 5 = 5th and 95th)
pub fn tail_ratio(returns: &[f64], tail_percentile: f64) -> Option<f64> {
    if returns.len() < 20 {
        // Need sufficient samples for tail analysis
        return None;
    }
    
    let mut sorted = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let lower_idx = ((tail_percentile / 100.0) * sorted.len() as f64).floor() as usize;
    let upper_idx = (((100.0 - tail_percentile) / 100.0) * sorted.len() as f64).floor() as usize;
    
    let lower_idx = lower_idx.min(sorted.len() - 1);
    let upper_idx = upper_idx.min(sorted.len() - 1);
    
    let left_tail = sorted[lower_idx];  // 5th percentile (typically negative)
    let right_tail = sorted[upper_idx]; // 95th percentile (typically positive)
    
    if left_tail.abs() < f64::EPSILON {
        if right_tail > 0.0 {
            return Some(100.0); // No left tail losses
        }
        return None;
    }
    
    Some(right_tail / left_tail.abs())
}

/// Comprehensive tail analysis
#[derive(Debug, Clone, Default)]
pub struct TailAnalysis {
    pub tail_ratio_5: Option<f64>,   // 5th vs 95th percentile
    pub tail_ratio_1: Option<f64>,   // 1st vs 99th percentile
    pub left_tail_5: f64,            // 5th percentile value
    pub right_tail_95: f64,          // 95th percentile value
    pub left_tail_1: f64,            // 1st percentile value
    pub right_tail_99: f64,          // 99th percentile value
    pub skewness: f64,               // Distribution skewness
    pub kurtosis: f64,               // Distribution kurtosis (excess)
}

/// Calculate comprehensive tail analysis
pub fn tail_analysis(returns: &[f64]) -> TailAnalysis {
    if returns.len() < 20 {
        return TailAnalysis::default();
    }
    
    let mut sorted = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let n = sorted.len();
    let idx_1 = (0.01 * n as f64).floor() as usize;
    let idx_5 = (0.05 * n as f64).floor() as usize;
    let idx_95 = (0.95 * n as f64).floor() as usize;
    let idx_99 = (0.99 * n as f64).floor() as usize;
    
    let left_tail_1 = sorted[idx_1.min(n - 1)];
    let left_tail_5 = sorted[idx_5.min(n - 1)];
    let right_tail_95 = sorted[idx_95.min(n - 1)];
    let right_tail_99 = sorted[idx_99.min(n - 1)];
    
    // Calculate skewness and kurtosis
    let mean: f64 = returns.iter().sum::<f64>() / n as f64;
    let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();
    
    let skewness = if std_dev > f64::EPSILON {
        let m3: f64 = returns.iter().map(|r| ((r - mean) / std_dev).powi(3)).sum::<f64>() / n as f64;
        m3
    } else {
        0.0
    };
    
    let kurtosis = if std_dev > f64::EPSILON {
        let m4: f64 = returns.iter().map(|r| ((r - mean) / std_dev).powi(4)).sum::<f64>() / n as f64;
        m4 - 3.0 // Excess kurtosis (0 for normal distribution)
    } else {
        0.0
    };
    
    TailAnalysis {
        tail_ratio_5: tail_ratio(returns, 5.0),
        tail_ratio_1: tail_ratio(returns, 1.0),
        left_tail_5,
        right_tail_95,
        left_tail_1,
        right_tail_99,
        skewness,
        kurtosis,
    }
}

/// Kelly Criterion for optimal position sizing.
/// 
/// Full Kelly: f* = (p * b - q) / b = p - q/b
/// where:
///   p = probability of winning
///   q = probability of losing (1 - p)
///   b = ratio of win to loss (avg_win / avg_loss)
/// 
/// For trading with continuous returns:
/// f* = μ / σ² (simplified Kelly for continuous returns)
/// 
/// Returns the optimal fraction of capital to risk.
pub fn kelly_criterion(returns: &[f64]) -> Option<KellyResult> {
    if returns.len() < 10 {
        return None;
    }
    
    let wins: Vec<f64> = returns.iter().filter(|&&r| r > 0.0).copied().collect();
    let losses: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).copied().collect();
    
    if wins.is_empty() || losses.is_empty() {
        return None;
    }
    
    // Probability of win
    let p = wins.len() as f64 / returns.len() as f64;
    let q = 1.0 - p;
    
    // Average win and loss magnitudes
    let avg_win = wins.iter().sum::<f64>() / wins.len() as f64;
    let avg_loss = losses.iter().map(|l| l.abs()).sum::<f64>() / losses.len() as f64;
    
    if avg_loss < f64::EPSILON {
        return None;
    }
    
    // Win/loss ratio
    let b = avg_win / avg_loss;
    
    // Discrete Kelly: f* = (p * b - q) / b
    let kelly_full = (p * b - q) / b;
    
    // Continuous Kelly: f* = μ / σ²
    let mean_return: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance: f64 = returns.iter()
        .map(|r| (r - mean_return).powi(2))
        .sum::<f64>() / returns.len() as f64;
    
    let kelly_continuous = if variance > f64::EPSILON {
        mean_return / variance
    } else {
        0.0
    };
    
    // Recommended Kelly fractions (full Kelly is often too aggressive)
    let kelly_half = kelly_full * 0.5;
    let kelly_quarter = kelly_full * 0.25;
    
    Some(KellyResult {
        kelly_full,
        kelly_half,
        kelly_quarter,
        kelly_continuous,
        win_probability: p,
        win_loss_ratio: b,
        avg_win,
        avg_loss,
        edge: p * avg_win - q * avg_loss, // Expected value per trade
    })
}

/// Kelly criterion calculation result
#[derive(Debug, Clone, Default)]
pub struct KellyResult {
    /// Full Kelly fraction (often too aggressive)
    pub kelly_full: f64,
    /// Half Kelly (recommended for most traders)
    pub kelly_half: f64,
    /// Quarter Kelly (conservative)
    pub kelly_quarter: f64,
    /// Continuous Kelly (μ/σ²)
    pub kelly_continuous: f64,
    /// Probability of winning trade
    pub win_probability: f64,
    /// Ratio of average win to average loss
    pub win_loss_ratio: f64,
    /// Average winning trade
    pub avg_win: f64,
    /// Average losing trade (absolute value)
    pub avg_loss: f64,
    /// Edge per trade (expected value)
    pub edge: f64,
}

impl KellyResult {
    /// Validate if a given position size is within Kelly bounds
    pub fn validate_position_size(&self, position_fraction: f64) -> KellyValidation {
        let deviation_from_optimal = (position_fraction - self.kelly_half).abs() / self.kelly_half.abs().max(0.01);
        
        let assessment = if position_fraction <= 0.0 {
            KellyAssessment::NoPosition
        } else if position_fraction > self.kelly_full {
            KellyAssessment::OverLeveraged
        } else if position_fraction > self.kelly_half {
            KellyAssessment::Aggressive
        } else if position_fraction >= self.kelly_quarter {
            KellyAssessment::Optimal
        } else {
            KellyAssessment::Conservative
        };
        
        KellyValidation {
            position_fraction,
            optimal_fraction: self.kelly_half,
            deviation_from_optimal,
            assessment,
            expected_growth_rate: self.calculate_growth_rate(position_fraction),
        }
    }
    
    /// Calculate expected log growth rate for a given fraction
    fn calculate_growth_rate(&self, f: f64) -> f64 {
        if f <= 0.0 || self.win_loss_ratio <= 0.0 {
            return 0.0;
        }
        
        // g(f) = p * ln(1 + b*f) + q * ln(1 - f)
        let p = self.win_probability;
        let q = 1.0 - p;
        let b = self.win_loss_ratio;
        
        let win_term = if 1.0 + b * f > 0.0 { p * (1.0 + b * f).ln() } else { f64::NEG_INFINITY };
        let loss_term = if 1.0 - f > 0.0 { q * (1.0 - f).ln() } else { f64::NEG_INFINITY };
        
        win_term + loss_term
    }
}

/// Kelly position sizing validation result
#[derive(Debug, Clone)]
pub struct KellyValidation {
    pub position_fraction: f64,
    pub optimal_fraction: f64,
    pub deviation_from_optimal: f64,
    pub assessment: KellyAssessment,
    pub expected_growth_rate: f64,
}

/// Kelly position sizing assessment
#[derive(Debug, Clone, PartialEq)]
pub enum KellyAssessment {
    NoPosition,
    Conservative,
    Optimal,
    Aggressive,
    OverLeveraged,
}

/// Cross-asset correlation monitoring for portfolio risk management.
/// 
/// Tracks rolling correlations between multiple asset returns.
pub struct CrossAssetMonitor {
    /// Asset return histories: asset_id -> returns
    asset_returns: HashMap<String, Vec<f64>>,
    /// Correlation matrix cache
    correlation_matrix: HashMap<(String, String), f64>,
    /// Rolling window size for correlation calculation
    window_size: usize,
}

impl CrossAssetMonitor {
    pub fn new(window_size: usize) -> Self {
        Self {
            asset_returns: HashMap::new(),
            correlation_matrix: HashMap::new(),
            window_size: window_size.max(10), // Minimum 10 periods
        }
    }
    
    /// Add a return observation for an asset
    pub fn add_return(&mut self, asset_id: &str, return_value: f64) {
        let returns = self.asset_returns.entry(asset_id.to_string()).or_insert_with(Vec::new);
        returns.push(return_value);
        
        // Keep only window_size most recent returns
        if returns.len() > self.window_size {
            returns.remove(0);
        }
    }
    
    /// Calculate correlation between two assets
    pub fn calculate_correlation(&mut self, asset_a: &str, asset_b: &str) -> Option<f64> {
        let returns_a = self.asset_returns.get(asset_a)?;
        let returns_b = self.asset_returns.get(asset_b)?;
        
        let min_len = returns_a.len().min(returns_b.len());
        if min_len < 10 {
            return None;
        }
        
        // Use most recent min_len returns
        let a: Vec<f64> = returns_a.iter().rev().take(min_len).copied().collect();
        let b: Vec<f64> = returns_b.iter().rev().take(min_len).copied().collect();
        
        let correlation = pearson_correlation(&a, &b)?;
        
        // Cache the result
        let key = if asset_a < asset_b {
            (asset_a.to_string(), asset_b.to_string())
        } else {
            (asset_b.to_string(), asset_a.to_string())
        };
        self.correlation_matrix.insert(key, correlation);
        
        Some(correlation)
    }
    
    /// Get full correlation matrix for all tracked assets
    pub fn get_correlation_matrix(&mut self) -> CorrelationMatrix {
        let assets: Vec<String> = self.asset_returns.keys().cloned().collect();
        let n = assets.len();
        
        let mut matrix = vec![vec![0.0; n]; n];
        let mut correlations = HashMap::new();
        
        for i in 0..n {
            matrix[i][i] = 1.0; // Self-correlation is always 1
            for j in (i + 1)..n {
                if let Some(corr) = self.calculate_correlation(&assets[i], &assets[j]) {
                    matrix[i][j] = corr;
                    matrix[j][i] = corr;
                    correlations.insert((assets[i].clone(), assets[j].clone()), corr);
                }
            }
        }
        
        CorrelationMatrix {
            assets,
            matrix,
            pairwise_correlations: correlations,
        }
    }
    
    /// Check for correlation breakdown (significant change from historical)
    pub fn detect_correlation_breakdown(&mut self, threshold: f64) -> Vec<CorrelationBreakdown> {
        let assets: Vec<String> = self.asset_returns.keys().cloned().collect();
        let mut breakdowns = Vec::new();
        
        for i in 0..assets.len() {
            for j in (i + 1)..assets.len() {
                let key = (assets[i].clone(), assets[j].clone());
                
                if let Some(&historical_corr) = self.correlation_matrix.get(&key) {
                    if let Some(current_corr) = self.calculate_correlation(&assets[i], &assets[j]) {
                        let change = (current_corr - historical_corr).abs();
                        if change > threshold {
                            breakdowns.push(CorrelationBreakdown {
                                asset_a: assets[i].clone(),
                                asset_b: assets[j].clone(),
                                historical_correlation: historical_corr,
                                current_correlation: current_corr,
                                change_magnitude: change,
                            });
                        }
                    }
                }
            }
        }
        
        breakdowns
    }
    
    /// Get average portfolio correlation (measure of diversification)
    pub fn average_correlation(&mut self) -> Option<f64> {
        let matrix = self.get_correlation_matrix();
        let n = matrix.assets.len();
        
        if n < 2 {
            return None;
        }
        
        let mut sum = 0.0;
        let mut count = 0;
        
        for i in 0..n {
            for j in (i + 1)..n {
                sum += matrix.matrix[i][j].abs();
                count += 1;
            }
        }
        
        if count > 0 {
            Some(sum / count as f64)
        } else {
            None
        }
    }
}

/// Correlation matrix result
#[derive(Debug, Clone)]
pub struct CorrelationMatrix {
    pub assets: Vec<String>,
    pub matrix: Vec<Vec<f64>>,
    pub pairwise_correlations: HashMap<(String, String), f64>,
}

/// Correlation breakdown alert
#[derive(Debug, Clone)]
pub struct CorrelationBreakdown {
    pub asset_a: String,
    pub asset_b: String,
    pub historical_correlation: f64,
    pub current_correlation: f64,
    pub change_magnitude: f64,
}

/// Pearson correlation coefficient between two vectors
fn pearson_correlation(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    
    let n = x.len() as f64;
    let mean_x: f64 = x.iter().sum::<f64>() / n;
    let mean_y: f64 = y.iter().sum::<f64>() / n;
    
    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;
    
    for i in 0..x.len() {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }
    
    let denominator = (var_x * var_y).sqrt();
    if denominator < f64::EPSILON {
        return None;
    }
    
    Some(cov / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_omega_ratio() {
        let returns = vec![0.01, -0.005, 0.02, -0.01, 0.015, 0.005, -0.008];
        let omega = omega_ratio(&returns, 0.0).unwrap();
        assert!(omega > 1.0, "Omega should be > 1 for positive returns");
    }
    
    #[test]
    fn test_tail_ratio() {
        // Create returns with asymmetric tails
        let mut returns: Vec<f64> = (0..100).map(|i| (i as f64 - 50.0) / 100.0).collect();
        returns.push(0.5);  // Add large positive outlier
        returns.push(-0.3); // Add smaller negative outlier
        
        let ratio = tail_ratio(&returns, 5.0);
        assert!(ratio.is_some());
    }
    
    #[test]
    fn test_kelly_criterion() {
        // Simulate a strategy with 60% win rate and 1.5:1 win/loss ratio
        let returns = vec![
            0.015, 0.012, -0.01, 0.018, -0.008, 0.014, 0.011, -0.009,
            0.016, -0.007, 0.013, 0.010, -0.011, 0.017, 0.009, -0.010,
        ];
        
        let kelly = kelly_criterion(&returns).unwrap();
        assert!(kelly.kelly_full > 0.0, "Kelly should be positive for profitable strategy");
        assert!(kelly.kelly_half < kelly.kelly_full, "Half Kelly should be less than full");
        assert!(kelly.win_probability > 0.5, "Win rate should be > 50%");
    }
    
    #[test]
    fn test_cross_asset_correlation() {
        let mut monitor = CrossAssetMonitor::new(20);
        
        // Add correlated returns
        for i in 0..30 {
            let base = (i as f64 * 0.1).sin();
            monitor.add_return("BTC", base + 0.01);
            monitor.add_return("ETH", base * 0.8 + 0.005); // Correlated with BTC
            monitor.add_return("GOLD", -base * 0.3 + 0.002); // Negatively correlated
        }
        
        let btc_eth_corr = monitor.calculate_correlation("BTC", "ETH").unwrap();
        let btc_gold_corr = monitor.calculate_correlation("BTC", "GOLD").unwrap();
        
        assert!(btc_eth_corr > 0.5, "BTC-ETH should be positively correlated");
        assert!(btc_gold_corr < 0.0, "BTC-GOLD should be negatively correlated");
    }
}
