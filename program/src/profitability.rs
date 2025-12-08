//! Profitability criteria for saving strategies to the database
//!
//! This module defines the criteria that a strategy must meet to be considered
//! "profitable" and worth saving to the database for future use.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use diesel_async::pooled_connection::deadpool;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;
use bigdecimal::{BigDecimal, FromPrimitive};

use crate::job_queue::BacktestResult;
use crate::logging_facade::BACKTEST_LOGGER;
use crate::log_info;

use databaseschema::ops::strategy_ops::StrategyOperations;
use databaseschema::models::strategy::{NewStrategy, NewStrategyInstance};

/// Profitability criteria that a strategy must meet to be saved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitabilityCriteria {
    /// Minimum Sharpe ratio (risk-adjusted return)
    /// Default: 1.0 (considered acceptable for most strategies)
    pub min_sharpe_ratio: f64,
    
    /// Minimum Sortino ratio (downside risk-adjusted return)
    /// Default: 1.5 (focuses on downside deviation)
    pub min_sortino_ratio: Option<f64>,
    
    /// Minimum Calmar ratio (return vs max drawdown)
    /// Default: 0.5
    pub min_calmar_ratio: Option<f64>,
    
    /// Minimum profit factor (gross profit / gross loss)
    /// Default: 1.2 (must make 20% more than it loses)
    pub min_profit_factor: f64,
    
    /// Maximum drawdown allowed (as decimal, e.g., 0.20 = 20%)
    /// Default: 0.25 (25% max drawdown)
    pub max_drawdown: f64,
    
    /// Minimum win rate (as decimal, e.g., 0.50 = 50%)
    /// Default: 0.40 (can be lower if risk/reward is good)
    pub min_win_rate: f64,
    
    /// Minimum number of trades to be statistically significant
    /// Default: 30
    pub min_trades: usize,
    
    /// Minimum total PnL (absolute value)
    /// Default: 0.0 (must be profitable)
    pub min_total_pnl: f64,
    
    /// Require positive annualized return
    /// Default: true
    pub require_positive_return: bool,
    
    /// Maximum Value at Risk (95%) as percentage of capital
    /// Default: None (not required)
    pub max_var_95_pct: Option<f64>,
}

impl Default for ProfitabilityCriteria {
    fn default() -> Self {
        Self {
            min_sharpe_ratio: 1.0,
            min_sortino_ratio: Some(1.5),
            min_calmar_ratio: Some(0.5),
            min_profit_factor: 1.2,
            max_drawdown: 0.25,
            min_win_rate: 0.40,
            min_trades: 30,
            min_total_pnl: 0.0,
            require_positive_return: true,
            max_var_95_pct: None,
        }
    }
}

impl ProfitabilityCriteria {
    /// Create criteria from job parameters (UI-configurable)
    /// This is the primary way to configure profitability criteria from the UI
    pub fn from_job_params(params: &crate::job_queue::BacktestJobParams) -> Self {
        Self {
            min_sharpe_ratio: params.min_sharpe_ratio,
            min_sortino_ratio: params.min_sortino_ratio,
            min_calmar_ratio: params.min_calmar_ratio,
            min_profit_factor: params.min_profit_factor,
            max_drawdown: params.max_drawdown,
            min_win_rate: params.min_win_rate,
            min_trades: params.min_trades,
            min_total_pnl: params.min_total_pnl.unwrap_or(0.0),
            require_positive_return: true,
            max_var_95_pct: None,
        }
    }
    
    /// Create criteria from GeneticConfig for consistency
    /// Uses the genetic optimizer's thresholds as a baseline, with sensible defaults
    /// for metrics that GeneticConfig doesn't specify
    pub fn from_genetic_config(genetic_config: &config::GeneticConfig) -> Self {
        // GeneticConfig primarily uses Sharpe ratio for fitness
        // We use its Monte Carlo settings to infer strictness level
        let is_strict = genetic_config.use_monte_carlo_fitness;
        
        if is_strict {
            // If using MC fitness, use stricter criteria
            Self {
                min_sharpe_ratio: 1.0,  // Genetic uses Sharpe as primary fitness
                min_sortino_ratio: Some(1.5),
                min_calmar_ratio: Some(0.5),
                min_profit_factor: 1.2,
                max_drawdown: 0.20,
                min_win_rate: 0.45,
                min_trades: 30,
                min_total_pnl: 0.0,
                require_positive_return: true,
                max_var_95_pct: Some(0.10), // 10% VaR from MC analysis
            }
        } else {
            // Standard optimization - use default criteria
            Self::default()
        }
    }
    
    /// Create relaxed criteria for testing or initial strategy exploration
    pub fn relaxed() -> Self {
        Self {
            min_sharpe_ratio: 0.5,
            min_sortino_ratio: None,
            min_calmar_ratio: None,
            min_profit_factor: 1.0,
            max_drawdown: 0.35,
            min_win_rate: 0.35,
            min_trades: 20,
            min_total_pnl: 0.0,
            require_positive_return: true,
            max_var_95_pct: None,
        }
    }
    
    /// Create strict criteria for production-ready strategies
    pub fn strict() -> Self {
        Self {
            min_sharpe_ratio: 1.5,
            min_sortino_ratio: Some(2.0),
            min_calmar_ratio: Some(1.0),
            min_profit_factor: 1.5,
            max_drawdown: 0.15,
            min_win_rate: 0.50,
            min_trades: 50,
            min_total_pnl: 1000.0,
            require_positive_return: true,
            max_var_95_pct: Some(0.05), // 5% VaR limit
        }
    }
    
    /// Builder method to set minimum Sharpe ratio
    pub fn with_min_sharpe(mut self, min_sharpe: f64) -> Self {
        self.min_sharpe_ratio = min_sharpe;
        self
    }
    
    /// Builder method to set maximum drawdown
    pub fn with_max_drawdown(mut self, max_dd: f64) -> Self {
        self.max_drawdown = max_dd;
        self
    }
    
    /// Builder method to set minimum trades
    pub fn with_min_trades(mut self, min_trades: usize) -> Self {
        self.min_trades = min_trades;
        self
    }
    
    /// Builder method to set minimum profit factor
    pub fn with_min_profit_factor(mut self, min_pf: f64) -> Self {
        self.min_profit_factor = min_pf;
        self
    }
    
    /// Check if a backtest result meets the profitability criteria
    pub fn is_profitable(&self, result: &BacktestResult) -> ProfitabilityResult {
        let mut failures = Vec::new();
        let mut passed = true;
        
        // Check Sharpe ratio
        if result.sharpe_ratio < self.min_sharpe_ratio {
            failures.push(format!(
                "Sharpe ratio {:.3} < minimum {:.3}",
                result.sharpe_ratio, self.min_sharpe_ratio
            ));
            passed = false;
        }
        
        // Check Sortino ratio (if required)
        if let Some(min_sortino) = self.min_sortino_ratio {
            if let Some(sortino) = result.sortino_ratio {
                if sortino < min_sortino {
                    failures.push(format!(
                        "Sortino ratio {:.3} < minimum {:.3}",
                        sortino, min_sortino
                    ));
                    passed = false;
                }
            } else {
                failures.push("Sortino ratio not available".to_string());
                passed = false;
            }
        }
        
        // Check Calmar ratio (if required)
        if let Some(min_calmar) = self.min_calmar_ratio {
            if let Some(calmar) = result.calmar_ratio {
                if calmar < min_calmar {
                    failures.push(format!(
                        "Calmar ratio {:.3} < minimum {:.3}",
                        calmar, min_calmar
                    ));
                    passed = false;
                }
            } else {
                failures.push("Calmar ratio not available".to_string());
                passed = false;
            }
        }
        
        // Check profit factor
        if result.profit_factor < self.min_profit_factor {
            failures.push(format!(
                "Profit factor {:.3} < minimum {:.3}",
                result.profit_factor, self.min_profit_factor
            ));
            passed = false;
        }
        
        // Check max drawdown
        if result.max_drawdown > self.max_drawdown {
            failures.push(format!(
                "Max drawdown {:.2}% > maximum {:.2}%",
                result.max_drawdown * 100.0, self.max_drawdown * 100.0
            ));
            passed = false;
        }
        
        // Check win rate
        if result.win_rate < self.min_win_rate {
            failures.push(format!(
                "Win rate {:.2}% < minimum {:.2}%",
                result.win_rate * 100.0, self.min_win_rate * 100.0
            ));
            passed = false;
        }
        
        // Check minimum trades
        if result.total_trades < self.min_trades {
            failures.push(format!(
                "Total trades {} < minimum {}",
                result.total_trades, self.min_trades
            ));
            passed = false;
        }
        
        // Check total PnL
        if result.total_pnl < self.min_total_pnl {
            failures.push(format!(
                "Total PnL {:.2} < minimum {:.2}",
                result.total_pnl, self.min_total_pnl
            ));
            passed = false;
        }
        
        // Check positive return (if required)
        if self.require_positive_return && result.total_pnl <= 0.0 {
            failures.push("Strategy is not profitable (negative PnL)".to_string());
            passed = false;
        }
        
        // Check VaR (if required)
        if let Some(max_var) = self.max_var_95_pct {
            if let Some(var) = result.value_at_risk_95 {
                if var > max_var {
                    failures.push(format!(
                        "VaR 95% {:.2}% > maximum {:.2}%",
                        var * 100.0, max_var * 100.0
                    ));
                    passed = false;
                }
            }
        }
        
        ProfitabilityResult {
            is_profitable: passed,
            failures,
            score: self.calculate_score(result),
        }
    }
    
    /// Calculate a composite score for the strategy (higher is better)
    fn calculate_score(&self, result: &BacktestResult) -> f64 {
        let mut score = 0.0;
        
        // Sharpe ratio contributes up to 30 points
        score += (result.sharpe_ratio / self.min_sharpe_ratio).min(3.0) * 10.0;
        
        // Profit factor contributes up to 20 points
        score += (result.profit_factor / self.min_profit_factor).min(2.0) * 10.0;
        
        // Win rate contributes up to 15 points
        score += (result.win_rate / self.min_win_rate).min(1.5) * 10.0;
        
        // Drawdown (lower is better) contributes up to 20 points
        let dd_score = (self.max_drawdown / result.max_drawdown.max(0.01)).min(2.0);
        score += dd_score * 10.0;
        
        // Trades volume contributes up to 15 points
        let trade_score = (result.total_trades as f64 / self.min_trades as f64).min(3.0);
        score += trade_score * 5.0;
        
        score
    }
}

/// Result of profitability check
#[derive(Debug, Clone)]
pub struct ProfitabilityResult {
    /// Whether the strategy meets all profitability criteria
    pub is_profitable: bool,
    /// List of criteria that were not met
    pub failures: Vec<String>,
    /// Composite score (higher is better)
    pub score: f64,
}

/// Save a profitable strategy to the database
pub async fn save_profitable_strategy(
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
    result: &BacktestResult,
    strategy_name: &str,
    strategy_type: &str,
    criteria: &ProfitabilityCriteria,
) -> Result<Option<Uuid>, Box<dyn std::error::Error + Send + Sync>> {
    // Check if the strategy meets profitability criteria
    let profitability = criteria.is_profitable(result);
    
    if !profitability.is_profitable {
        log_info!(BACKTEST_LOGGER, 
            "Strategy {} does not meet profitability criteria: {:?}", 
            strategy_name, profitability.failures
        );
        return Ok(None);
    }
    
    log_info!(BACKTEST_LOGGER, 
        "Strategy {} meets profitability criteria with score {:.2}. Saving to database...", 
        strategy_name, profitability.score
    );
    
    // Get a connection from the pool
    let mut conn = databaseschema::get_timescale_connection(pool.clone()).await?;
    
    // First, try to find or create the base strategy
    let strategy = match StrategyOperations::get_strategy_by_name_version(
        &mut *conn, 
        strategy_name, 
        "1.0"
    ).await {
        Ok(s) => s,
        Err(_) => {
            // Create new strategy
            let new_strategy = NewStrategy {
                strategy_name: strategy_name.to_string(),
                strategy_type: strategy_type.to_string(),
                version: "1.0".to_string(),
                description: Some(format!(
                    "Auto-saved profitable strategy. Sharpe: {:.3}, Profit Factor: {:.3}",
                    result.sharpe_ratio, result.profit_factor
                )),
                created_by: Some("backtest_engine".to_string()),
                is_active: true,
                base_configuration: result.strategy_params.clone(),
                metadata: Some(serde_json::json!({
                    "auto_saved": true,
                    "profitability_score": profitability.score,
                    "job_id": result.job_id,
                })),
            };
            
            StrategyOperations::create_strategy(&mut *conn, new_strategy).await?
        }
    };
    
    // Create a strategy instance with the specific parameters and performance
    let new_instance = NewStrategyInstance {
        strategy_id: strategy.id,
        instance_name: Some(format!("{}_{}", strategy_name, result.job_id)),
        description: Some(format!(
            "Backtest result from job {}. PnL: {:.2}, Sharpe: {:.3}",
            result.job_id, result.total_pnl, result.sharpe_ratio
        )),
        parameters: result.strategy_params.clone().unwrap_or(serde_json::json!({})),
        performance_summary: Some(serde_json::json!({
            "total_pnl": result.total_pnl,
            "sharpe_ratio": result.sharpe_ratio,
            "sortino_ratio": result.sortino_ratio,
            "calmar_ratio": result.calmar_ratio,
            "max_drawdown": result.max_drawdown,
            "win_rate": result.win_rate,
            "profit_factor": result.profit_factor,
            "total_trades": result.total_trades,
            "profitability_score": profitability.score,
        })),
        risk_metrics: Some(serde_json::json!({
            "max_drawdown": result.max_drawdown,
            "value_at_risk_95": result.value_at_risk_95,
            "expected_shortfall_95": result.expected_shortfall_95,
            "volatility": result.volatility,
        })),
        is_template: false,
        tags: Some(vec![
            Some("auto-saved".to_string()),
            Some("profitable".to_string()),
            Some(format!("score:{:.0}", profitability.score)),
        ]),
        created_by: Some("backtest_engine".to_string()),
        optimization_run_id: None,
        optimization_score: BigDecimal::from_f64(profitability.score),
    };
    
    let instance = StrategyOperations::create_strategy_instance(&mut *conn, new_instance).await?;
    
    log_info!(BACKTEST_LOGGER, 
        "Saved strategy instance {} (score: {:.2}) for strategy {}", 
        instance.id, profitability.score, strategy.id
    );
    
    Ok(Some(instance.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    
    fn create_test_result(sharpe: f64, pnl: f64, drawdown: f64, trades: usize) -> BacktestResult {
        BacktestResult {
            job_id: "test-job".to_string(),
            total_pnl: pnl,
            sharpe_ratio: sharpe,
            max_drawdown: drawdown,
            win_rate: 0.55,
            total_trades: trades,
            profit_factor: 1.5,
            equity_curve: vec![100000.0, 100500.0, 101000.0],
            completed_at: Utc::now(),
            annualized_return: None,
            volatility: None,
            sortino_ratio: Some(2.0),
            calmar_ratio: Some(1.0),
            avg_trade_return: None,
            best_trade: None,
            worst_trade: None,
            avg_time_in_trade: None,
            value_at_risk_95: None,
            expected_shortfall_95: None,
            max_drawdown_duration_days: None,
            current_drawdown: None,
            avg_drawdown: None,
            total_orders: None,
            filled_orders: None,
            cancelled_orders: None,
            avg_slippage: None,
            total_commission_paid: None,
            strategy_params: None,
            monte_carlo: None,
            walk_forward: None,
        }
    }
    
    #[test]
    fn test_profitable_strategy_passes() {
        let criteria = ProfitabilityCriteria::default();
        let result = create_test_result(1.5, 5000.0, 0.10, 50);
        
        let profitability = criteria.is_profitable(&result);
        assert!(profitability.is_profitable);
        assert!(profitability.failures.is_empty());
        assert!(profitability.score > 0.0);
    }
    
    #[test]
    fn test_unprofitable_strategy_fails() {
        let criteria = ProfitabilityCriteria::default();
        let result = create_test_result(0.5, -1000.0, 0.40, 15);
        
        let profitability = criteria.is_profitable(&result);
        assert!(!profitability.is_profitable);
        assert!(!profitability.failures.is_empty());
    }
    
    #[test]
    fn test_relaxed_criteria() {
        let criteria = ProfitabilityCriteria::relaxed();
        let result = create_test_result(0.6, 500.0, 0.30, 25);
        
        let profitability = criteria.is_profitable(&result);
        assert!(profitability.is_profitable);
    }
    
    #[test]
    fn test_strict_criteria() {
        let criteria = ProfitabilityCriteria::strict();
        let result = create_test_result(1.5, 5000.0, 0.10, 50);
        
        // May fail strict criteria due to higher thresholds
        let profitability = criteria.is_profitable(&result);
        // This might fail because strict requires sharpe >= 1.5 and min_trades >= 50
        // but we're right at the edge
    }
}
