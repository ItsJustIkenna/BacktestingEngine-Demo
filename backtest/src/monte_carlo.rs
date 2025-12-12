//! Monte Carlo simulation module for backtesting
//! 
//! Performs Monte Carlo analysis by randomizing trade sequences to assess
//! strategy robustness and generate confidence intervals for performance metrics.
//! 
//! Methods:
//! - Shuffle: Randomize trade order to test path dependency
//! - Bootstrap: Sample with replacement for variance estimation
//! - Block Bootstrap: Preserve autocorrelation in trade sequences
//! - Regime-Aware: Sample from similar market conditions

use crate::BacktestResult;
use crate::logging_facade::BACKTEST_LOGGER;
use crate::log_info;
use metrics::PerformanceMetrics;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use rand::prelude::*;
use rayon::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub num_runs: usize,
    pub mean_metrics: PerformanceMetrics,
    pub stddev_metrics: PerformanceMetrics,
    pub percentile_metrics: HashMap<u8, PerformanceMetrics>, // percentile -> metrics
    pub var_95: f64, // Value at Risk (95th percentile) in USD
    pub cvar_95: f64, // Conditional VaR (expected loss beyond 95th percentile) in USD
    /// Number of data points used for Monte Carlo (equity curve length - 1)
    /// VaR/CVaR are unreliable if this is < 30
    #[serde(default)]
    pub data_points: usize,
    /// True if VaR/CVaR should be considered unreliable due to insufficient data
    #[serde(default)]
    pub var_unreliable: bool,
}

// ============================================
// Block Bootstrap Configuration
// ============================================

/// Configuration for block bootstrap Monte Carlo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBootstrapConfig {
    /// Block size for preserving autocorrelation (typically 5-20 trades)
    pub block_size: usize,
    /// Whether to use overlapping blocks (circular block bootstrap)
    pub overlapping: bool,
    /// Minimum block size (auto-adjusts if data is too short)
    pub min_block_size: usize,
}

impl Default for BlockBootstrapConfig {
    fn default() -> Self {
        Self {
            block_size: 10,
            overlapping: true,
            min_block_size: 3,
        }
    }
}

impl BlockBootstrapConfig {
    /// Optimal block size based on data length (Politis & White rule of thumb)
    pub fn optimal_block_size(data_length: usize) -> usize {
        // Rule of thumb: block_size ≈ n^(1/3)
        let optimal = (data_length as f64).powf(1.0 / 3.0).ceil() as usize;
        optimal.max(3).min(data_length / 4).max(1)
    }
}

// ============================================
// Regime-Aware Sampling Configuration
// ============================================

/// Market regime classification for regime-aware sampling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketRegimeClass {
    LowVolatility,
    NormalVolatility,
    HighVolatility,
    Crisis,
}

/// Configuration for regime-aware Monte Carlo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeAwareConfig {
    /// Lookback window for volatility calculation
    pub volatility_lookback: usize,
    /// Current volatility level (for regime detection)
    pub current_volatility: f64,
    /// Regime similarity weight (0.0 = random, 1.0 = only same regime)
    pub regime_weight: f64,
    /// Volatility thresholds: [low/normal, normal/high, high/crisis]
    pub volatility_thresholds: [f64; 3],
}

impl Default for RegimeAwareConfig {
    fn default() -> Self {
        Self {
            volatility_lookback: 20,
            current_volatility: 0.02,
            regime_weight: 0.7,
            volatility_thresholds: [0.01, 0.025, 0.05], // 1%, 2.5%, 5% daily vol
        }
    }
}

impl RegimeAwareConfig {
    /// Classify volatility into regime
    pub fn classify_regime(&self, volatility: f64) -> MarketRegimeClass {
        if volatility < self.volatility_thresholds[0] {
            MarketRegimeClass::LowVolatility
        } else if volatility < self.volatility_thresholds[1] {
            MarketRegimeClass::NormalVolatility
        } else if volatility < self.volatility_thresholds[2] {
            MarketRegimeClass::HighVolatility
        } else {
            MarketRegimeClass::Crisis
        }
    }
    
    /// Get regime for current conditions
    pub fn current_regime(&self) -> MarketRegimeClass {
        self.classify_regime(self.current_volatility)
    }
}

/// Helper functions for aggregation
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn std_dev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        0.0
    } else {
        let mean_val = mean(values);
        let variance = values.iter()
            .map(|x| (x - mean_val).powi(2))
            .sum::<f64>() / (values.len() - 1) as f64;
        variance.sqrt()
    }
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = (p / 100.0 * (values.len() - 1) as f64) as usize;
    values[index.min(values.len() - 1)]
}

/// Run Monte Carlo simulation with progress callback
/// 
/// This simulates alternative "what if" scenarios by shuffling the order of trades
/// while keeping the same trades, helping assess if results are due to luck or skill.
pub fn run_monte_carlo_simulation_with_progress(
    base_result: &BacktestResult,
    num_runs: usize,
    progress_callback: Option<std::sync::Arc<dyn Fn(&str, usize, usize) + Send + Sync>>,
) -> anyhow::Result<MonteCarloResult> {
    run_monte_carlo_simulation_internal(base_result, num_runs, progress_callback)
}

/// Run Monte Carlo simulation by randomizing trade sequences
/// 
/// This simulates alternative "what if" scenarios by shuffling the order of trades
/// while keeping the same trades, helping assess if results are due to luck or skill.
pub fn run_monte_carlo_simulation(
    base_result: &BacktestResult,
    num_runs: usize,
) -> anyhow::Result<MonteCarloResult> {
    run_monte_carlo_simulation_internal(base_result, num_runs, None)
}

/// Run lightweight Monte Carlo for genetic fitness evaluation
/// 
/// Returns only the mean and standard deviation of Sharpe ratios for fast consistency checking.
/// Used in lazy evaluation to assess parameter stability without full analysis.
pub fn run_lightweight_monte_carlo(
    base_result: &BacktestResult,
    num_runs: usize,
) -> anyhow::Result<(f64, f64)> {
    // Extract individual trade returns from equity curve
    let mut trade_returns: Vec<f64> = Vec::new();
    if base_result.equity_curve.len() > 1 {
        for i in 1..base_result.equity_curve.len() {
            let ret = base_result.equity_curve[i] - base_result.equity_curve[i - 1];
            trade_returns.push(ret);
        }
    }
    
    // If no trades, return zero mean/stddev
    if trade_returns.is_empty() {
        return Ok((0.0, 0.0));
    }
    
    // Run Monte Carlo simulations in parallel
    let sharpe_ratios: Vec<f64> = (0..num_runs)
        .into_par_iter()
        .map(|_| {
            let mut rng = rand::thread_rng();
            let mut shuffled_returns = trade_returns.clone();
            shuffled_returns.shuffle(&mut rng);
            
            // Calculate Sharpe ratio for this shuffle
            let mean_ret = mean(&shuffled_returns);
            let std_ret = std_dev(&shuffled_returns);
            if std_ret > 0.0 {
                mean_ret / std_ret * (252.0_f64.sqrt()) // Annualized
            } else {
                0.0
            }
        })
        .collect();
    
    // Return mean and standard deviation of Sharpe ratios
    let mean_sharpe = mean(&sharpe_ratios);
    let std_sharpe = std_dev(&sharpe_ratios);
    
    Ok((mean_sharpe, std_sharpe))
}

fn run_monte_carlo_simulation_internal(
    base_result: &BacktestResult,
    num_runs: usize,
    progress_callback: Option<std::sync::Arc<dyn Fn(&str, usize, usize) + Send + Sync>>,
) -> anyhow::Result<MonteCarloResult> {
    // Extract individual returns from equity curve for portfolio path simulation
    // These are period-by-period equity changes, used for drawdown and Sharpe calculation
    let mut equity_deltas: Vec<f64> = Vec::new();
    if base_result.equity_curve.len() > 1 {
        for i in 1..base_result.equity_curve.len() {
            let ret = base_result.equity_curve[i] - base_result.equity_curve[i - 1];
            equity_deltas.push(ret);
        }
    }
    
    // Use actual trade returns for win_rate calculation (if available)
    // This ensures Monte Carlo win_rate matches the trade-level win_rate metric
    let actual_trade_returns: Vec<f64> = if !base_result.trade_returns.is_empty() {
        base_result.trade_returns.clone()
    } else {
        // Fallback: use equity deltas (less accurate for win_rate)
        equity_deltas.clone()
    };
    
    // If no data, return default result
    if equity_deltas.is_empty() {
        let mut percentiles = HashMap::new();
        percentiles.insert(5, PerformanceMetrics::default());
        percentiles.insert(25, PerformanceMetrics::default());
        percentiles.insert(50, PerformanceMetrics::default());
        percentiles.insert(75, PerformanceMetrics::default());
        percentiles.insert(95, PerformanceMetrics::default());
        
        return Ok(MonteCarloResult {
            num_runs,
            mean_metrics: PerformanceMetrics::default(),
            stddev_metrics: PerformanceMetrics::default(),
            percentile_metrics: percentiles,
            var_95: 0.0,
            cvar_95: 0.0,
            data_points: 0,
            var_unreliable: true, // No data = completely unreliable
        });
    }
    
    // Run Monte Carlo simulations using two methods:
    // 1. Trade order shuffling (traditional Monte Carlo)
    // 2. Bootstrap resampling (sampling with replacement)
    
    let bootstrap_runs = num_runs / 2; // Half bootstrap, half shuffle
    let shuffle_runs = num_runs - bootstrap_runs;
    
    // Progress tracking with atomic counter
    let completed_runs = AtomicUsize::new(0);
    let log_interval = (num_runs / 10).max(100); // Log every 10% or every 100 runs
    
    log_info!(BACKTEST_LOGGER, "Monte Carlo starting | total_runs={} shuffle={} bootstrap={}", 
              num_runs, shuffle_runs, bootstrap_runs);
    
    // Calculate base win rate from actual trades (used for shuffle method)
    let base_win_rate = if !actual_trade_returns.is_empty() {
        let wins = actual_trade_returns.iter().filter(|&&r| r > 0.0).count();
        wins as f64 / actual_trade_returns.len() as f64
    } else {
        0.0
    };
    
    // Method 1: Trade order shuffling (PARALLEL with Rayon)
    // Note: Shuffling doesn't change win_rate (same trades, different order)
    // We use this for path-dependent metrics (drawdown, Sharpe)
    let shuffle_results: Vec<_> = (0..shuffle_runs)
        .into_par_iter()
        .map(|_i| {
            // Progress logging
            let completed = completed_runs.fetch_add(1, Ordering::Relaxed);
            if completed % log_interval == 0 {
                log_info!(BACKTEST_LOGGER, "Monte Carlo progress | run={}/{} ({:.0}%)", 
                         completed, num_runs, (completed as f64 / num_runs as f64) * 100.0);
            }
            
            let mut rng = rand::thread_rng();
            
            // Shuffle equity deltas for portfolio path simulation
            let mut shuffled_returns = equity_deltas.clone();
            shuffled_returns.shuffle(&mut rng);
        
        // Reconstruct equity curve from shuffled returns
        let mut equity = base_result.initial_capital;
        let mut equity_curve = vec![equity];
        for ret in &shuffled_returns {
            equity += ret;
            equity_curve.push(equity);
        }
        
        // Calculate metrics for this run
        let net_profit = equity - base_result.initial_capital;
        
        // Calculate Sharpe ratio
        let sharpe = if !shuffled_returns.is_empty() {
            let mean_ret = mean(&shuffled_returns);
            let std_ret = std_dev(&shuffled_returns);
            if std_ret > 0.0 {
                mean_ret / std_ret * (252.0_f64.sqrt()) // Annualized
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // Calculate max drawdown
        let mut peak = equity_curve[0];
        let mut max_dd = 0.0;
        for &val in &equity_curve {
            if val > peak {
                peak = val;
            }
            let dd = (peak - val) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
        
        // Win rate is constant for shuffle (same trades, different order)
        (net_profit, sharpe, max_dd, base_win_rate)
    }).collect();
    
    // Extract results from parallel computation
    let mut all_net_profits = Vec::with_capacity(num_runs);
    let mut all_sharpe_ratios = Vec::with_capacity(num_runs);
    let mut all_max_drawdowns = Vec::with_capacity(num_runs);
    let mut all_win_rates = Vec::with_capacity(num_runs);
    
    for (net_profit, sharpe, max_dd, win_rate) in shuffle_results {
        all_net_profits.push(net_profit);
        all_sharpe_ratios.push(sharpe);
        all_max_drawdowns.push(max_dd);
        all_win_rates.push(win_rate);
    }
    
    log_info!(BACKTEST_LOGGER, "Monte Carlo shuffle phase complete | runs={}", shuffle_runs);
    
    // Report progress after shuffle phase
    if let Some(ref callback) = progress_callback {
        callback("monte_carlo", shuffle_runs, num_runs);
    }
    
    // Method 2: Bootstrap resampling (PARALLEL with Rayon)
    // Bootstrap on actual trade returns gives win_rate variation
    let bootstrap_results: Vec<_> = (0..bootstrap_runs)
        .into_par_iter()
        .map(|_i| {
            // Progress logging
            let completed = completed_runs.fetch_add(1, Ordering::Relaxed);
            if completed % log_interval == 0 {
                log_info!(BACKTEST_LOGGER, "Monte Carlo progress | run={}/{} ({:.0}%)", 
                         completed, num_runs, (completed as f64 / num_runs as f64) * 100.0);
            }
            
            let mut rng = rand::thread_rng();
            
            // Bootstrap sample equity deltas for portfolio path simulation
            let mut bootstrap_equity_deltas = Vec::with_capacity(equity_deltas.len());
        for _ in 0..equity_deltas.len() {
            let idx = rng.gen_range(0..equity_deltas.len());
            bootstrap_equity_deltas.push(equity_deltas[idx]);
        }
        
        // Bootstrap sample actual trade returns for win_rate calculation
        let bootstrap_win_rate = if !actual_trade_returns.is_empty() {
            let mut bootstrap_trades = Vec::with_capacity(actual_trade_returns.len());
            for _ in 0..actual_trade_returns.len() {
                let idx = rng.gen_range(0..actual_trade_returns.len());
                bootstrap_trades.push(actual_trade_returns[idx]);
            }
            let wins = bootstrap_trades.iter().filter(|&&r| r > 0.0).count();
            wins as f64 / bootstrap_trades.len() as f64
        } else {
            0.0
        };
        
        // Reconstruct equity curve from bootstrap sample
        let mut equity = base_result.initial_capital;
        let mut equity_curve = vec![equity];
        for ret in &bootstrap_equity_deltas {
            equity += ret;
            equity_curve.push(equity);
        }
        
        // Calculate metrics for this run
        let net_profit = equity - base_result.initial_capital;
        
        // Calculate Sharpe ratio
        let sharpe = if !bootstrap_equity_deltas.is_empty() {
            let mean_ret = mean(&bootstrap_equity_deltas);
            let std_ret = std_dev(&bootstrap_equity_deltas);
            if std_ret > 0.0 {
                mean_ret / std_ret * (252.0_f64.sqrt()) // Annualized
            } else {
                0.0
            }
        } else {
            0.0
        };
        
        // Calculate max drawdown
        let mut peak = equity_curve[0];
        let mut max_dd = 0.0;
        for &val in &equity_curve {
            if val > peak {
                peak = val;
            }
            let dd = (peak - val) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
        
        (net_profit, sharpe, max_dd, bootstrap_win_rate)
    }).collect();
    
    // Extract bootstrap results
    for (net_profit, sharpe, max_dd, win_rate) in bootstrap_results {
        all_net_profits.push(net_profit);
        all_sharpe_ratios.push(sharpe);
        all_max_drawdowns.push(max_dd);
        all_win_rates.push(win_rate);
    }
    
    // Report final progress (100%)
    if let Some(ref callback) = progress_callback {
        callback("monte_carlo", num_runs, num_runs);
    }
    
    // Calculate aggregate statistics
    let mean_net_profit = mean(&all_net_profits);
    let std_net_profit = std_dev(&all_net_profits);
    let mean_sharpe = mean(&all_sharpe_ratios);
    let std_sharpe = std_dev(&all_sharpe_ratios);
    let mean_dd = mean(&all_max_drawdowns);
    let std_dd = std_dev(&all_max_drawdowns);
    let mean_win_rate = mean(&all_win_rates);
    let std_win_rate = std_dev(&all_win_rates);
    
    // Calculate percentiles
    let mut percentiles = HashMap::new();
    for &p in &[5, 25, 50, 75, 95] {
        let mut profits_copy = all_net_profits.clone();
        let mut sharpe_copy = all_sharpe_ratios.clone();
        let mut dd_copy = all_max_drawdowns.clone();
        let mut wr_copy = all_win_rates.clone();
        
        percentiles.insert(p, PerformanceMetrics {
            net_profit: percentile(&mut profits_copy, p as f64),
            sharpe_ratio: percentile(&mut sharpe_copy, p as f64),
            max_drawdown: percentile(&mut dd_copy, p as f64),
            win_rate: percentile(&mut wr_copy, p as f64),
            gross_profit: 0.0, // Not calculated in Monte Carlo
            gross_loss: 0.0,
            profit_factor: 0.0,
            average_trade_return: 0.0,
            median_trade_return: 0.0,
            number_of_trades: base_result.num_trades,
            volatility: base_result.volatility.unwrap_or(0.0),
            average_trade_duration: base_result.avg_trade_duration.unwrap_or(0.0),
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        });
    }
    
    // Calculate VaR and CVaR (95% confidence)
    // VaR = Value at Risk = potential loss at 95% confidence
    // This means: "We are 95% confident we won't lose more than VaR"
    // Convention: VaR is expressed as a POSITIVE number representing potential loss
    let mut sorted_profits = all_net_profits.clone();
    sorted_profits.sort_by(|a, b| a.partial_cmp(b).unwrap()); // Ascending: worst to best
    let var_95_index = (0.05 * sorted_profits.len() as f64) as usize;
    let percentile_5_profit = sorted_profits[var_95_index];
    
    // VaR = negative of the 5th percentile (if 5th percentile is -$1000, VaR is $1000)
    // If even the 5th percentile is positive (all runs profitable), VaR is 0 (no expected loss)
    let var_95 = if percentile_5_profit < 0.0 {
        -percentile_5_profit  // Convert loss to positive VaR
    } else {
        0.0  // No VaR if even worst outcomes are profitable
    };
    
    // CVaR (Conditional VaR / Expected Shortfall): average loss BEYOND the VaR threshold
    // This is the expected loss in the worst 5% of scenarios
    let losses_beyond_var: Vec<f64> = sorted_profits.iter()
        .take(var_95_index)
        .copied()
        .collect();
    let cvar_95 = if !losses_beyond_var.is_empty() {
        let avg_worst_profit = mean(&losses_beyond_var);
        if avg_worst_profit < 0.0 {
            -avg_worst_profit  // Convert to positive expected loss
        } else {
            0.0
        }
    } else {
        var_95  // Fall back to VaR if no data
    };
    
    log_info!(BACKTEST_LOGGER, "Monte Carlo complete | runs={} mean_sharpe={:.3} VaR95=${:.2} CVaR95=${:.2}", 
              num_runs, mean_sharpe, var_95, cvar_95);
    
    Ok(MonteCarloResult {
        num_runs,
        mean_metrics: PerformanceMetrics {
            net_profit: mean_net_profit,
            gross_profit: base_result.gross_profit.unwrap_or(0.0),
            gross_loss: base_result.gross_loss.unwrap_or(0.0),
            sharpe_ratio: mean_sharpe,
            max_drawdown: mean_dd,
            win_rate: mean_win_rate,
            profit_factor: base_result.profit_factor.unwrap_or(0.0),
            average_trade_return: base_result.avg_trade_return.unwrap_or(0.0),
            median_trade_return: base_result.median_trade_return.unwrap_or(0.0),
            number_of_trades: base_result.num_trades,
            volatility: base_result.volatility.unwrap_or(0.0),
            average_trade_duration: base_result.avg_trade_duration.unwrap_or(0.0),
            total_commission: base_result.transaction_costs.as_ref().map(|tc| tc.total_commission).unwrap_or(0.0),
            total_slippage: base_result.transaction_costs.as_ref().map(|tc| tc.total_slippage).unwrap_or(0.0),
            total_market_impact: base_result.transaction_costs.as_ref().map(|tc| tc.total_market_impact).unwrap_or(0.0),
            transaction_costs_percentage: base_result.transaction_costs.as_ref().map(|tc| tc.costs_as_percentage_of_pnl).unwrap_or(0.0),
        },
        stddev_metrics: PerformanceMetrics {
            net_profit: std_net_profit,
            gross_profit: 0.0,
            gross_loss: 0.0,
            sharpe_ratio: std_sharpe,
            max_drawdown: std_dd,
            win_rate: std_win_rate,
            profit_factor: 0.0,
            average_trade_return: 0.0,
            median_trade_return: 0.0,
            number_of_trades: 0,
            volatility: 0.0,
            average_trade_duration: 0.0,
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        },
        percentile_metrics: percentiles,
        var_95,
        cvar_95,
        // Track data reliability - use actual trade count for win_rate reliability
        data_points: actual_trade_returns.len(),
        var_unreliable: actual_trade_returns.len() < 30, // VaR needs at least ~30 samples to be meaningful
    })
}

// ============================================
// Block Bootstrap Monte Carlo
// ============================================

/// Run block bootstrap Monte Carlo simulation
/// 
/// Preserves autocorrelation in trade sequences by sampling blocks of consecutive trades
/// rather than individual trades. This is important for strategies where trade outcomes
/// are correlated (e.g., trend following).
pub fn run_block_bootstrap_simulation(
    base_result: &BacktestResult,
    num_runs: usize,
    config: BlockBootstrapConfig,
) -> anyhow::Result<MonteCarloResult> {
    // Extract equity deltas
    let mut equity_deltas: Vec<f64> = Vec::new();
    if base_result.equity_curve.len() > 1 {
        for i in 1..base_result.equity_curve.len() {
            equity_deltas.push(base_result.equity_curve[i] - base_result.equity_curve[i - 1]);
        }
    }
    
    if equity_deltas.is_empty() {
        return run_monte_carlo_simulation(base_result, num_runs);
    }
    
    // Determine effective block size
    let block_size = config.block_size
        .max(config.min_block_size)
        .min(equity_deltas.len() / 2)
        .max(1);
    
    let n = equity_deltas.len();
    
    // Run block bootstrap in parallel
    let results: Vec<(f64, f64, f64)> = (0..num_runs)
        .into_par_iter()
        .map(|_| {
            let mut rng = rand::thread_rng();
            
            // Sample blocks to reconstruct sequence
            let mut bootstrap_sample = Vec::with_capacity(n);
            
            while bootstrap_sample.len() < n {
                // Random starting point for block
                let start_idx = if config.overlapping {
                    rng.gen_range(0..n)
                } else {
                    rng.gen_range(0..n.saturating_sub(block_size).max(1))
                };
                
                // Extract block (with wrap-around for circular bootstrap)
                for offset in 0..block_size {
                    if bootstrap_sample.len() >= n {
                        break;
                    }
                    let idx = if config.overlapping {
                        (start_idx + offset) % n // Circular
                    } else {
                        (start_idx + offset).min(n - 1)
                    };
                    bootstrap_sample.push(equity_deltas[idx]);
                }
            }
            
            // Truncate to exact length
            bootstrap_sample.truncate(n);
            
            // Calculate metrics
            let net_profit: f64 = bootstrap_sample.iter().sum();
            
            let mean_ret = if !bootstrap_sample.is_empty() {
                bootstrap_sample.iter().sum::<f64>() / bootstrap_sample.len() as f64
            } else {
                0.0
            };
            
            let std_ret = if bootstrap_sample.len() > 1 {
                let variance = bootstrap_sample.iter()
                    .map(|x| (x - mean_ret).powi(2))
                    .sum::<f64>() / (bootstrap_sample.len() - 1) as f64;
                variance.sqrt()
            } else {
                0.0
            };
            
            let sharpe = if std_ret > 0.0 {
                mean_ret / std_ret * 252.0_f64.sqrt()
            } else {
                0.0
            };
            
            // Max drawdown
            let mut equity = base_result.initial_capital;
            let mut peak = equity;
            let mut max_dd = 0.0;
            for ret in &bootstrap_sample {
                equity += ret;
                if equity > peak {
                    peak = equity;
                }
                let dd = (peak - equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
            
            (net_profit, sharpe, max_dd)
        })
        .collect();
    
    // Aggregate results
    let all_net_profits: Vec<f64> = results.iter().map(|r| r.0).collect();
    let all_sharpes: Vec<f64> = results.iter().map(|r| r.1).collect();
    let all_drawdowns: Vec<f64> = results.iter().map(|r| r.2).collect();
    
    // Build result using helper functions
    build_monte_carlo_result(
        base_result,
        num_runs,
        all_net_profits,
        all_sharpes,
        all_drawdowns,
    )
}

// ============================================
// Regime-Aware Monte Carlo
// ============================================

/// Run regime-aware Monte Carlo simulation
/// 
/// Samples trades from similar market regimes to provide more realistic
/// expectations for current conditions. Uses volatility-based regime classification.
pub fn run_regime_aware_simulation(
    base_result: &BacktestResult,
    num_runs: usize,
    config: RegimeAwareConfig,
    volatility_per_trade: &[f64], // Volatility at each trade
) -> anyhow::Result<MonteCarloResult> {
    // Extract equity deltas
    let mut equity_deltas: Vec<f64> = Vec::new();
    if base_result.equity_curve.len() > 1 {
        for i in 1..base_result.equity_curve.len() {
            equity_deltas.push(base_result.equity_curve[i] - base_result.equity_curve[i - 1]);
        }
    }
    
    if equity_deltas.is_empty() {
        return run_monte_carlo_simulation(base_result, num_runs);
    }
    
    // Classify each trade into a regime
    let trade_regimes: Vec<MarketRegimeClass> = volatility_per_trade.iter()
        .map(|&vol| config.classify_regime(vol))
        .collect();
    
    // If volatility data doesn't match, use uniform regime
    let trade_regimes = if trade_regimes.len() != equity_deltas.len() {
        vec![MarketRegimeClass::NormalVolatility; equity_deltas.len()]
    } else {
        trade_regimes
    };
    
    let current_regime = config.current_regime();
    let regime_weight = config.regime_weight.clamp(0.0, 1.0);
    
    // Group trades by regime
    let mut regime_indices: HashMap<MarketRegimeClass, Vec<usize>> = HashMap::new();
    for (idx, &regime) in trade_regimes.iter().enumerate() {
        regime_indices.entry(regime).or_default().push(idx);
    }
    
    let n = equity_deltas.len();
    
    // Run regime-aware sampling in parallel
    let results: Vec<(f64, f64, f64)> = (0..num_runs)
        .into_par_iter()
        .map(|_| {
            let mut rng = rand::thread_rng();
            let mut bootstrap_sample = Vec::with_capacity(n);
            
            for _ in 0..n {
                // Weighted selection: prefer same regime with probability regime_weight
                let use_same_regime = rng.gen::<f64>() < regime_weight;
                
                let selected_idx = if use_same_regime {
                    // Sample from same regime
                    if let Some(indices) = regime_indices.get(&current_regime) {
                        if !indices.is_empty() {
                            indices[rng.gen_range(0..indices.len())]
                        } else {
                            rng.gen_range(0..n)
                        }
                    } else {
                        rng.gen_range(0..n)
                    }
                } else {
                    // Random sample from any regime
                    rng.gen_range(0..n)
                };
                
                bootstrap_sample.push(equity_deltas[selected_idx]);
            }
            
            // Calculate metrics
            let net_profit: f64 = bootstrap_sample.iter().sum();
            
            let mean_ret = if !bootstrap_sample.is_empty() {
                bootstrap_sample.iter().sum::<f64>() / bootstrap_sample.len() as f64
            } else {
                0.0
            };
            
            let std_ret = if bootstrap_sample.len() > 1 {
                let variance = bootstrap_sample.iter()
                    .map(|x| (x - mean_ret).powi(2))
                    .sum::<f64>() / (bootstrap_sample.len() - 1) as f64;
                variance.sqrt()
            } else {
                0.0
            };
            
            let sharpe = if std_ret > 0.0 {
                mean_ret / std_ret * 252.0_f64.sqrt()
            } else {
                0.0
            };
            
            // Max drawdown
            let mut equity = base_result.initial_capital;
            let mut peak = equity;
            let mut max_dd = 0.0;
            for ret in &bootstrap_sample {
                equity += ret;
                if equity > peak {
                    peak = equity;
                }
                let dd = (peak - equity) / peak;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
            
            (net_profit, sharpe, max_dd)
        })
        .collect();
    
    // Aggregate results
    let all_net_profits: Vec<f64> = results.iter().map(|r| r.0).collect();
    let all_sharpes: Vec<f64> = results.iter().map(|r| r.1).collect();
    let all_drawdowns: Vec<f64> = results.iter().map(|r| r.2).collect();
    
    build_monte_carlo_result(
        base_result,
        num_runs,
        all_net_profits,
        all_sharpes,
        all_drawdowns,
    )
}

/// Helper to build MonteCarloResult from simulation outputs
fn build_monte_carlo_result(
    base_result: &BacktestResult,
    num_runs: usize,
    all_net_profits: Vec<f64>,
    all_sharpes: Vec<f64>,
    all_drawdowns: Vec<f64>,
) -> anyhow::Result<MonteCarloResult> {
    let mean_net_profit = mean(&all_net_profits);
    let std_net_profit = std_dev(&all_net_profits);
    let mean_sharpe = mean(&all_sharpes);
    let std_sharpe = std_dev(&all_sharpes);
    let mean_dd = mean(&all_drawdowns);
    let std_dd = std_dev(&all_drawdowns);
    
    // Win rate from base result
    let base_win_rate = base_result.win_rate.unwrap_or(0.0);
    
    // Calculate percentiles
    let mut percentiles = HashMap::new();
    for &p in &[5, 25, 50, 75, 95] {
        let mut profits_copy = all_net_profits.clone();
        let mut sharpe_copy = all_sharpes.clone();
        let mut dd_copy = all_drawdowns.clone();
        
        percentiles.insert(p, PerformanceMetrics {
            net_profit: percentile(&mut profits_copy, p as f64),
            sharpe_ratio: percentile(&mut sharpe_copy, p as f64),
            max_drawdown: percentile(&mut dd_copy, p as f64),
            win_rate: base_win_rate,
            gross_profit: 0.0,
            gross_loss: 0.0,
            profit_factor: 0.0,
            average_trade_return: 0.0,
            median_trade_return: 0.0,
            number_of_trades: base_result.num_trades,
            volatility: base_result.volatility.unwrap_or(0.0),
            average_trade_duration: base_result.avg_trade_duration.unwrap_or(0.0),
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        });
    }
    
    // VaR and CVaR
    let mut sorted_profits = all_net_profits.clone();
    sorted_profits.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let var_95_index = (0.05 * sorted_profits.len() as f64) as usize;
    let percentile_5_profit = sorted_profits[var_95_index];
    
    let var_95 = if percentile_5_profit < 0.0 {
        -percentile_5_profit
    } else {
        0.0
    };
    
    let losses_beyond_var: Vec<f64> = sorted_profits.iter()
        .take(var_95_index)
        .copied()
        .collect();
    let cvar_95 = if !losses_beyond_var.is_empty() {
        let avg_worst = mean(&losses_beyond_var);
        if avg_worst < 0.0 { -avg_worst } else { 0.0 }
    } else {
        var_95
    };
    
    let data_points = base_result.equity_curve.len().saturating_sub(1);
    
    Ok(MonteCarloResult {
        num_runs,
        mean_metrics: PerformanceMetrics {
            net_profit: mean_net_profit,
            gross_profit: base_result.gross_profit.unwrap_or(0.0),
            gross_loss: base_result.gross_loss.unwrap_or(0.0),
            sharpe_ratio: mean_sharpe,
            max_drawdown: mean_dd,
            win_rate: base_win_rate,
            profit_factor: base_result.profit_factor.unwrap_or(0.0),
            average_trade_return: base_result.avg_trade_return.unwrap_or(0.0),
            median_trade_return: base_result.median_trade_return.unwrap_or(0.0),
            number_of_trades: base_result.num_trades,
            volatility: base_result.volatility.unwrap_or(0.0),
            average_trade_duration: base_result.avg_trade_duration.unwrap_or(0.0),
            total_commission: base_result.transaction_costs.as_ref().map(|tc| tc.total_commission).unwrap_or(0.0),
            total_slippage: base_result.transaction_costs.as_ref().map(|tc| tc.total_slippage).unwrap_or(0.0),
            total_market_impact: base_result.transaction_costs.as_ref().map(|tc| tc.total_market_impact).unwrap_or(0.0),
            transaction_costs_percentage: base_result.transaction_costs.as_ref().map(|tc| tc.costs_as_percentage_of_pnl).unwrap_or(0.0),
        },
        stddev_metrics: PerformanceMetrics {
            net_profit: std_net_profit,
            gross_profit: 0.0,
            gross_loss: 0.0,
            sharpe_ratio: std_sharpe,
            max_drawdown: std_dd,
            win_rate: 0.0,
            profit_factor: 0.0,
            average_trade_return: 0.0,
            median_trade_return: 0.0,
            number_of_trades: 0,
            volatility: 0.0,
            average_trade_duration: 0.0,
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        },
        percentile_metrics: percentiles,
        var_95,
        cvar_95,
        data_points,
        var_unreliable: data_points < 30,
    })
}