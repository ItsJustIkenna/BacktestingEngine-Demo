//! Monte Carlo simulation module for backtesting
//! 
//! Performs Monte Carlo analysis by randomizing trade sequences to assess
//! strategy robustness and generate confidence intervals for performance metrics.

use crate::BacktestResult;
use metrics::PerformanceMetrics;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use rand::prelude::*;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub num_runs: usize,
    pub mean_metrics: PerformanceMetrics,
    pub stddev_metrics: PerformanceMetrics,
    pub percentile_metrics: HashMap<u8, PerformanceMetrics>, // percentile -> metrics
    pub var_95: f64, // Value at Risk (95th percentile)
    pub cvar_95: f64, // Conditional VaR (expected loss beyond 95th percentile)
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
    // Extract individual trade returns from equity curve
    let mut trade_returns: Vec<f64> = Vec::new();
    if base_result.equity_curve.len() > 1 {
        for i in 1..base_result.equity_curve.len() {
            let ret = base_result.equity_curve[i] - base_result.equity_curve[i - 1];
            trade_returns.push(ret);
        }
    }
    
    // If no trades, return default result
    if trade_returns.is_empty() {
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
        });
    }
    
    // Run Monte Carlo simulations using two methods:
    // 1. Trade order shuffling (traditional Monte Carlo)
    // 2. Bootstrap resampling (sampling with replacement)
    
    let bootstrap_runs = num_runs / 2; // Half bootstrap, half shuffle
    let shuffle_runs = num_runs - bootstrap_runs;
    
    // Method 1: Trade order shuffling (PARALLEL with Rayon)
    let shuffle_results: Vec<_> = (0..shuffle_runs)
        .into_par_iter()
        .map(|_i| {
            let mut rng = rand::thread_rng();
            
            // Shuffle trade returns
            let mut shuffled_returns = trade_returns.clone();
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
        
        // Calculate win rate
        let wins = shuffled_returns.iter().filter(|&&r| r > 0.0).count();
        let win_rate = wins as f64 / shuffled_returns.len() as f64;
        
        (net_profit, sharpe, max_dd, win_rate)
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
    
    // Report progress after shuffle phase
    if let Some(ref callback) = progress_callback {
        callback("monte_carlo", shuffle_runs, num_runs);
    }
    
    // Method 2: Bootstrap resampling (PARALLEL with Rayon)
    let bootstrap_results: Vec<_> = (0..bootstrap_runs)
        .into_par_iter()
        .map(|_i| {
            let mut rng = rand::thread_rng();
            
            // Bootstrap sample: sample with replacement
            let mut bootstrap_returns = Vec::with_capacity(trade_returns.len());
        for _ in 0..trade_returns.len() {
            let idx = rng.gen_range(0..trade_returns.len());
            bootstrap_returns.push(trade_returns[idx]);
        }
        
        // Reconstruct equity curve from bootstrap sample
        let mut equity = base_result.initial_capital;
        let mut equity_curve = vec![equity];
        for ret in &bootstrap_returns {
            equity += ret;
            equity_curve.push(equity);
        }
        
        // Calculate metrics for this run
        let net_profit = equity - base_result.initial_capital;
        
        // Calculate Sharpe ratio
        let sharpe = if !bootstrap_returns.is_empty() {
            let mean_ret = mean(&bootstrap_returns);
            let std_ret = std_dev(&bootstrap_returns);
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
        
        // Calculate win rate
        let wins = bootstrap_returns.iter().filter(|&&r| r > 0.0).count();
        let win_rate = wins as f64 / bootstrap_returns.len() as f64;
        
        (net_profit, sharpe, max_dd, win_rate)
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
    let mut sorted_profits = all_net_profits.clone();
    sorted_profits.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let var_95_index = (0.05 * sorted_profits.len() as f64) as usize;
    let var_95 = sorted_profits[var_95_index];
    
    // CVaR: average of losses below VaR threshold
    let losses_beyond_var: Vec<f64> = sorted_profits.iter()
        .take(var_95_index)
        .copied()
        .collect();
    let cvar_95 = if !losses_beyond_var.is_empty() {
        mean(&losses_beyond_var)
    } else {
        var_95
    };
    
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
    })
}

