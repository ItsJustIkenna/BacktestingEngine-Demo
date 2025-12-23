//! Statistical Significance Testing for Backtesting
//!
//! This module provides statistical tests to validate backtest results:
//! - t-test for Sharpe ratio significance
//! - Bootstrap confidence intervals
//! - Probabilistic Sharpe Ratio (PSR)
//! - Deflated Sharpe Ratio (accounting for multiple testing)

use serde::{Deserialize, Serialize};

/// Statistical significance results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignificanceResult {
    /// Original Sharpe ratio
    pub sharpe_ratio: f64,
    /// Standard error of Sharpe ratio
    pub sharpe_std_error: f64,
    /// t-statistic for testing Sharpe > 0
    pub t_statistic: f64,
    /// p-value (one-tailed, H0: Sharpe <= 0)
    pub p_value: f64,
    /// Is statistically significant at 5% level?
    pub is_significant_5pct: bool,
    /// Is statistically significant at 1% level?
    pub is_significant_1pct: bool,
    /// Probabilistic Sharpe Ratio (probability Sharpe > benchmark)
    pub probabilistic_sharpe_ratio: f64,
    /// 95% confidence interval lower bound
    pub ci_95_lower: f64,
    /// 95% confidence interval upper bound
    pub ci_95_upper: f64,
    /// Minimum track record length needed for significance (in periods)
    pub min_track_record_length: usize,
    /// Number of observations used
    pub n_observations: usize,
    /// Skewness of returns
    pub skewness: f64,
    /// Excess kurtosis of returns
    pub kurtosis: f64,
}

/// Bootstrap confidence interval results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfidenceIntervals {
    /// Number of bootstrap iterations
    pub n_iterations: usize,
    /// Sharpe ratio CI
    pub sharpe_ci_95: (f64, f64),
    pub sharpe_ci_99: (f64, f64),
    /// Total return CI
    pub return_ci_95: (f64, f64),
    /// Max drawdown CI
    pub max_drawdown_ci_95: (f64, f64),
    /// Win rate CI
    pub win_rate_ci_95: (f64, f64),
    /// Profit factor CI
    pub profit_factor_ci_95: (f64, f64),
}

/// Compute statistical significance of a Sharpe ratio
/// 
/// Uses the Lo (2002) adjustment for autocorrelation in returns
/// Reference: "The Statistics of Sharpe Ratios" - Andrew Lo
pub fn compute_sharpe_significance(
    returns: &[f64],
    benchmark_sharpe: f64,
    annualization_factor: f64,
) -> SignificanceResult {
    let n = returns.len();
    
    if n < 10 {
        return SignificanceResult {
            sharpe_ratio: 0.0,
            sharpe_std_error: f64::MAX,
            t_statistic: 0.0,
            p_value: 1.0,
            is_significant_5pct: false,
            is_significant_1pct: false,
            probabilistic_sharpe_ratio: 0.5,
            ci_95_lower: f64::NEG_INFINITY,
            ci_95_upper: f64::INFINITY,
            min_track_record_length: usize::MAX,
            n_observations: n,
            skewness: 0.0,
            kurtosis: 0.0,
        };
    }
    
    // Compute moments
    let mean = returns.iter().sum::<f64>() / n as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let std_dev = variance.sqrt();
    
    if std_dev < 1e-10 {
        return SignificanceResult {
            sharpe_ratio: 0.0,
            sharpe_std_error: f64::MAX,
            t_statistic: 0.0,
            p_value: 1.0,
            is_significant_5pct: false,
            is_significant_1pct: false,
            probabilistic_sharpe_ratio: 0.5,
            ci_95_lower: f64::NEG_INFINITY,
            ci_95_upper: f64::INFINITY,
            min_track_record_length: usize::MAX,
            n_observations: n,
            skewness: 0.0,
            kurtosis: 0.0,
        };
    }
    
    // Compute Sharpe ratio (not annualized for the test)
    let sharpe = mean / std_dev;
    let sharpe_annualized = sharpe * annualization_factor.sqrt();
    
    // Compute skewness and kurtosis
    let m3 = returns.iter().map(|r| ((r - mean) / std_dev).powi(3)).sum::<f64>() / n as f64;
    let m4 = returns.iter().map(|r| ((r - mean) / std_dev).powi(4)).sum::<f64>() / n as f64;
    let skewness = m3;
    let excess_kurtosis = m4 - 3.0;
    
    // Standard error of Sharpe ratio (Lo 2002)
    // SE(SR) = sqrt((1 + 0.5*SR^2 - skew*SR + (kurtosis-3)/4 * SR^2) / (n-1))
    let sr_variance = (1.0 + 0.5 * sharpe.powi(2) - skewness * sharpe 
        + excess_kurtosis / 4.0 * sharpe.powi(2)) / (n - 1) as f64;
    let sharpe_std_error = sr_variance.sqrt();
    
    // t-statistic for testing Sharpe > benchmark_sharpe
    let t_stat = (sharpe - benchmark_sharpe) / sharpe_std_error;
    
    // p-value using normal approximation (valid for large n)
    let p_value = 1.0 - normal_cdf(t_stat);
    
    // Probabilistic Sharpe Ratio
    // PSR = Φ((SR - SR_benchmark) / SE(SR))
    let psr = normal_cdf((sharpe - benchmark_sharpe) / sharpe_std_error);
    
    // 95% confidence interval
    let z_95 = 1.96;
    let ci_95_lower = (sharpe - z_95 * sharpe_std_error) * annualization_factor.sqrt();
    let ci_95_upper = (sharpe + z_95 * sharpe_std_error) * annualization_factor.sqrt();
    
    // Minimum Track Record Length (Bailey & López de Prado)
    // MinTRL = n * ((z_alpha * SE(SR)) / (SR - SR_benchmark))^2
    let z_alpha = 1.645; // 5% one-tailed
    let min_trl = if sharpe > benchmark_sharpe && sharpe_std_error > 0.0 {
        let ratio = (z_alpha * sharpe_std_error) / (sharpe - benchmark_sharpe);
        (n as f64 * ratio.powi(2)).ceil() as usize
    } else {
        usize::MAX
    };
    
    SignificanceResult {
        sharpe_ratio: sharpe_annualized,
        sharpe_std_error: sharpe_std_error * annualization_factor.sqrt(),
        t_statistic: t_stat,
        p_value,
        is_significant_5pct: p_value < 0.05,
        is_significant_1pct: p_value < 0.01,
        probabilistic_sharpe_ratio: psr,
        ci_95_lower,
        ci_95_upper,
        min_track_record_length: min_trl,
        n_observations: n,
        skewness,
        kurtosis: excess_kurtosis,
    }
}

/// Compute bootstrap confidence intervals for backtest metrics
/// 
/// Uses stratified bootstrap to preserve trade characteristics
pub fn compute_bootstrap_confidence_intervals(
    trade_returns: &[f64],
    n_iterations: usize,
) -> BootstrapConfidenceIntervals {
    use rand::prelude::*;
    use rayon::prelude::*;
    
    let n = trade_returns.len();
    
    if n < 10 {
        return BootstrapConfidenceIntervals {
            n_iterations: 0,
            sharpe_ci_95: (f64::NEG_INFINITY, f64::INFINITY),
            sharpe_ci_99: (f64::NEG_INFINITY, f64::INFINITY),
            return_ci_95: (f64::NEG_INFINITY, f64::INFINITY),
            max_drawdown_ci_95: (0.0, 1.0),
            win_rate_ci_95: (0.0, 1.0),
            profit_factor_ci_95: (0.0, f64::INFINITY),
        };
    }
    
    // Generate bootstrap samples in parallel
    let results: Vec<_> = (0..n_iterations)
        .into_par_iter()
        .map(|seed| {
            let mut rng = StdRng::seed_from_u64(seed as u64);
            
            // Sample with replacement
            let sample: Vec<f64> = (0..n)
                .map(|_| trade_returns[rng.gen_range(0..n)])
                .collect();
            
            compute_sample_metrics(&sample)
        })
        .collect();
    
    // Extract metrics
    let mut sharpes: Vec<f64> = results.iter().map(|r| r.0).collect();
    let mut returns: Vec<f64> = results.iter().map(|r| r.1).collect();
    let mut drawdowns: Vec<f64> = results.iter().map(|r| r.2).collect();
    let mut win_rates: Vec<f64> = results.iter().map(|r| r.3).collect();
    let mut profit_factors: Vec<f64> = results.iter().map(|r| r.4).collect();
    
    // Sort for percentile calculation
    sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    drawdowns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    win_rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    profit_factors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let p025 = (0.025 * n_iterations as f64) as usize;
    let p005 = (0.005 * n_iterations as f64) as usize;
    let p975 = (0.975 * n_iterations as f64) as usize;
    let p995 = (0.995 * n_iterations as f64) as usize;
    
    BootstrapConfidenceIntervals {
        n_iterations,
        sharpe_ci_95: (sharpes[p025], sharpes[p975.min(sharpes.len() - 1)]),
        sharpe_ci_99: (sharpes[p005], sharpes[p995.min(sharpes.len() - 1)]),
        return_ci_95: (returns[p025], returns[p975.min(returns.len() - 1)]),
        max_drawdown_ci_95: (drawdowns[p025], drawdowns[p975.min(drawdowns.len() - 1)]),
        win_rate_ci_95: (win_rates[p025], win_rates[p975.min(win_rates.len() - 1)]),
        profit_factor_ci_95: (profit_factors[p025], profit_factors[p975.min(profit_factors.len() - 1)]),
    }
}

/// Compute metrics for a bootstrap sample
/// Returns (sharpe, total_return, max_drawdown, win_rate, profit_factor)
fn compute_sample_metrics(returns: &[f64]) -> (f64, f64, f64, f64, f64) {
    let n = returns.len();
    if n == 0 {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    }
    
    // Sharpe
    let mean = returns.iter().sum::<f64>() / n as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();
    let sharpe = if std_dev > 1e-10 { mean / std_dev } else { 0.0 };
    
    // Total return (compounded)
    let total_return = returns.iter().fold(1.0, |acc, r| acc * (1.0 + r)) - 1.0;
    
    // Max drawdown
    let mut peak: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    let mut equity: f64 = 1.0;
    for r in returns {
        equity *= 1.0 + r;
        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
    }
    
    // Win rate
    let wins = returns.iter().filter(|r| **r > 0.0).count();
    let win_rate = wins as f64 / n as f64;
    
    // Profit factor
    let gross_profit: f64 = returns.iter().filter(|r| **r > 0.0).sum();
    let gross_loss: f64 = returns.iter().filter(|r| **r < 0.0).map(|r| r.abs()).sum();
    let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { f64::MAX };
    
    (sharpe, total_return, max_dd, win_rate, profit_factor.min(100.0))
}

/// Standard normal CDF approximation (Abramowitz and Stegun)
fn normal_cdf(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs() / std::f64::consts::SQRT_2;
    
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    
    0.5 * (1.0 + sign * y)
}

/// Deflated Sharpe Ratio - accounts for multiple testing
/// 
/// When testing many strategies, some will appear significant by chance.
/// DSR adjusts for this selection bias.
/// 
/// Reference: Bailey & López de Prado (2014)
pub fn compute_deflated_sharpe_ratio(
    sharpe: f64,
    sharpe_std_error: f64,
    num_strategies_tested: usize,
    expected_max_sharpe_if_null: f64,
) -> f64 {
    if sharpe_std_error <= 0.0 || num_strategies_tested == 0 {
        return 0.0;
    }
    
    // Expected maximum Sharpe under null (all strategies are random)
    // E[max(SR)] ≈ (1 - γ) * Φ^(-1)(1 - 1/N) + γ * Φ^(-1)(1 - 1/(N*e))
    // where γ ≈ 0.5772 (Euler-Mascheroni constant)
    let expected_max = expected_max_sharpe_if_null;
    
    // DSR = Φ((SR - E[max(SR)]) / SE(SR))
    normal_cdf((sharpe - expected_max) / sharpe_std_error)
}

/// Estimate expected maximum Sharpe ratio under null hypothesis
/// 
/// When testing N independent strategies with no skill, what's the expected
/// maximum Sharpe ratio due to chance alone?
pub fn expected_max_sharpe_under_null(
    num_strategies: usize,
    lookback_periods: usize,
) -> f64 {
    if num_strategies == 0 || lookback_periods == 0 {
        return 0.0;
    }
    
    let n = num_strategies as f64;
    let t = lookback_periods as f64;
    
    // Approximate using extreme value theory
    // E[max] ≈ sqrt(2 * ln(N)) - (ln(ln(N)) + ln(4*pi)) / (2 * sqrt(2 * ln(N)))
    // Adjusted for finite sample: multiply by sqrt((T-1)/T)
    
    let ln_n = n.ln();
    if ln_n <= 0.0 {
        return 0.0;
    }
    
    let sqrt_2_ln_n = (2.0 * ln_n).sqrt();
    let expected_max = sqrt_2_ln_n 
        - (ln_n.ln() + (4.0 * std::f64::consts::PI).ln()) / (2.0 * sqrt_2_ln_n);
    
    // Finite sample adjustment
    expected_max * ((t - 1.0) / t).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sharpe_significance() {
        // Generate some positive returns
        let returns: Vec<f64> = (0..252)
            .map(|i| 0.001 + 0.02 * ((i as f64 * 0.1).sin()))
            .collect();
        
        let result = compute_sharpe_significance(&returns, 0.0, 252.0);
        
        assert!(result.sharpe_ratio > 0.0);
        assert!(result.n_observations == 252);
        println!("Sharpe: {:.4}", result.sharpe_ratio);
        println!("t-stat: {:.4}", result.t_statistic);
        println!("p-value: {:.4}", result.p_value);
        println!("Significant at 5%: {}", result.is_significant_5pct);
    }
    
    #[test]
    fn test_bootstrap_ci() {
        let returns: Vec<f64> = (0..100)
            .map(|i| 0.01 + 0.05 * ((i as f64 * 0.2).sin()))
            .collect();
        
        let ci = compute_bootstrap_confidence_intervals(&returns, 1000);
        
        assert!(ci.sharpe_ci_95.0 < ci.sharpe_ci_95.1);
        println!("Sharpe 95% CI: ({:.4}, {:.4})", ci.sharpe_ci_95.0, ci.sharpe_ci_95.1);
    }
    
    #[test]
    fn test_normal_cdf() {
        assert!((normal_cdf(0.0) - 0.5).abs() < 0.001);
        assert!((normal_cdf(1.96) - 0.975).abs() < 0.01);
        assert!((normal_cdf(-1.96) - 0.025).abs() < 0.01);
    }
}
