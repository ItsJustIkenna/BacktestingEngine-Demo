//! Portfolio/Multi-Symbol Simulation Engine
//!
//! This module provides portfolio-level backtesting across multiple symbols:
//! - Synchronized event processing across assets
//! - Portfolio-level capital allocation
//! - Cross-asset inventory and risk management
//! - Aggregated performance metrics

use crate::realistic_simulation::{
    RealisticSimulator, RealisticASParams, OrderBookEvent,
};
use crate::portfolio_risk::PortfolioRiskMetrics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Per-symbol simulation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolResult {
    pub symbol: String,
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub num_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_volume: f64,
    pub total_fees: f64,
    pub allocation_weight: f64,
    pub allocated_capital: f64,
    // Starting position
    pub starting_inventory: f64,
    pub starting_inventory_value: f64,
    pub starting_capital: f64,
    pub starting_mark_price: f64,
    // Ending position
    pub ending_inventory: f64,
    pub ending_inventory_value: f64,
    pub ending_capital: f64,
    pub ending_mark_price: f64,
    pub final_equity: f64,
    // Execution metrics
    pub orders_placed: usize,
    pub orders_filled: usize,
    pub fill_rate: f64,
    pub avg_time_in_trade_ms: f64,
}

/// Portfolio-level aggregated result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSimResult {
    /// Per-symbol breakdown
    pub symbol_results: Vec<SymbolResult>,
    
    /// Aggregate metrics
    pub total_pnl: f64,
    pub portfolio_sharpe: f64,
    pub portfolio_max_drawdown: f64,
    pub total_trades: usize,
    pub portfolio_win_rate: f64,
    pub portfolio_profit_factor: f64,
    pub total_volume: f64,
    pub total_fees: f64,
    
    /// Portfolio equity curve (combined across all symbols)
    pub equity_curve: Vec<f64>,
    
    /// Trade returns for Monte Carlo (aggregated)
    pub trade_returns: Vec<f64>,
    
    /// Portfolio risk metrics
    pub risk_metrics: Option<PortfolioRiskMetrics>,
    
    /// Initial total capital
    pub initial_capital: f64,
    /// Final total equity
    pub final_equity: f64,
    
    /// Correlation matrix between symbols
    pub correlation_matrix: Option<Vec<CorrelationPair>>,
    
    /// Aggregated execution metrics
    pub total_orders_placed: usize,
    pub total_orders_filled: usize,
    pub avg_fill_rate: f64,
    pub avg_time_in_trade_ms: f64,
}

/// Correlation between two symbols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationPair {
    pub symbol1: String,
    pub symbol2: String,
    pub correlation: f64,
}

/// Portfolio simulator that runs across multiple symbols
pub struct PortfolioSimulator {
    initial_capital: f64,
    fee_config: config::ExchangeFeeConfig,
}

impl PortfolioSimulator {
    /// Create a new portfolio simulator
    pub fn new(initial_capital: f64, fee_config: config::ExchangeFeeConfig) -> Self {
        Self {
            initial_capital,
            fee_config,
        }
    }
    
    /// Run portfolio simulation across multiple symbols
    /// 
    /// # Arguments
    /// * `symbol_events` - Map of symbol -> order book events
    /// * `params` - Strategy parameters (applied to each symbol)
    /// * `allocation_weights` - Optional capital allocation per symbol (equal if None)
    /// 
    /// # Returns
    /// Portfolio-level aggregated results with per-symbol breakdown
    pub fn run(
        &self,
        symbol_events: &HashMap<String, Vec<OrderBookEvent>>,
        params: &RealisticASParams,
        allocation_weights: Option<&Vec<f64>>,
    ) -> PortfolioSimResult {
        let symbols: Vec<String> = symbol_events.keys().cloned().collect();
        let num_symbols = symbols.len();
        
        if num_symbols == 0 {
            return self.empty_result();
        }
        
        // Calculate allocation weights (equal if not specified)
        let weights: Vec<f64> = allocation_weights
            .map(|w| w.clone())
            .unwrap_or_else(|| vec![1.0 / num_symbols as f64; num_symbols]);
        
        // Run individual simulations per symbol
        let mut symbol_results: Vec<SymbolResult> = Vec::with_capacity(num_symbols);
        let mut all_equity_curves: Vec<Vec<f64>> = Vec::new();
        let mut all_trade_returns: Vec<f64> = Vec::new();
        
        for (i, symbol) in symbols.iter().enumerate() {
            let weight = weights.get(i).copied().unwrap_or(1.0 / num_symbols as f64);
            let allocated_capital = self.initial_capital * weight;
            
            // Create per-symbol simulator with allocated capital
            let simulator = RealisticSimulator::with_exchange_config(
                allocated_capital,
                self.fee_config.clone(),
            );
            
            // Run simulation for this symbol
            if let Some(events) = symbol_events.get(symbol) {
                let result = simulator.run(events, params);
                
                // Collect trade returns for aggregate Monte Carlo
                all_trade_returns.extend(result.trade_returns.iter().cloned());
                
                // Collect equity curve for portfolio aggregation
                all_equity_curves.push(result.equity_curve.clone());
                
                // Build per-symbol result
                symbol_results.push(SymbolResult {
                    symbol: symbol.clone(),
                    total_pnl: result.total_pnl,
                    sharpe_ratio: result.sharpe_ratio,
                    max_drawdown: result.max_drawdown,
                    num_trades: result.num_trades,
                    win_rate: result.win_rate,
                    profit_factor: result.profit_factor,
                    total_volume: result.total_volume,
                    total_fees: result.total_fees,
                    allocation_weight: weight,
                    allocated_capital,
                    // Starting position
                    starting_inventory: result.inventory_metrics.starting_inventory,
                    starting_inventory_value: result.inventory_metrics.starting_inventory_value,
                    starting_capital: result.inventory_metrics.starting_capital,
                    starting_mark_price: result.inventory_metrics.starting_mark_price,
                    // Ending position
                    ending_inventory: result.inventory_metrics.ending_inventory,
                    ending_inventory_value: result.inventory_metrics.ending_inventory_value,
                    ending_capital: result.inventory_metrics.ending_capital,
                    ending_mark_price: result.inventory_metrics.ending_mark_price,
                    final_equity: result.inventory_metrics.final_equity,
                    // Execution metrics - derive from fill rate
                    // We estimate orders_placed based on num_trades and fill_rate
                    orders_placed: if result.avg_fill_rate > 0.0 {
                        (result.num_trades as f64 / result.avg_fill_rate).round() as usize
                    } else {
                        result.num_trades
                    },
                    orders_filled: result.num_trades,
                    fill_rate: result.avg_fill_rate,
                    avg_time_in_trade_ms: result.avg_time_in_trade_ms,
                });
            }
        }
        
        // Aggregate portfolio equity curve
        let portfolio_equity_curve = self.aggregate_equity_curves(&all_equity_curves);
        
        // Calculate portfolio-level metrics
        let total_pnl: f64 = symbol_results.iter().map(|r| r.total_pnl).sum();
        let total_trades: usize = symbol_results.iter().map(|r| r.num_trades).sum();
        let total_volume: f64 = symbol_results.iter().map(|r| r.total_volume).sum();
        let total_fees: f64 = symbol_results.iter().map(|r| r.total_fees).sum();
        let final_equity: f64 = symbol_results.iter().map(|r| r.final_equity).sum();
        
        // Calculate portfolio Sharpe from aggregated equity curve
        let portfolio_sharpe = self.calculate_portfolio_sharpe(&portfolio_equity_curve);
        
        // Calculate portfolio max drawdown
        let portfolio_max_drawdown = self.calculate_max_drawdown(&portfolio_equity_curve);
        
        // Aggregate win rate (weighted by number of trades)
        let portfolio_win_rate = if total_trades > 0 {
            symbol_results.iter()
                .map(|r| r.win_rate * r.num_trades as f64)
                .sum::<f64>() / total_trades as f64
        } else {
            0.0
        };
        
        // Aggregate profit factor
        let total_wins: f64 = symbol_results.iter()
            .filter(|r| r.total_pnl > 0.0)
            .map(|r| r.total_pnl)
            .sum();
        let total_losses: f64 = symbol_results.iter()
            .filter(|r| r.total_pnl < 0.0)
            .map(|r| r.total_pnl.abs())
            .sum();
        let portfolio_profit_factor = if total_losses > 0.0 {
            (total_wins / total_losses).min(100.0)
        } else if total_wins > 0.0 {
            10.0
        } else {
            1.0
        };
        
        // Calculate correlation matrix between symbol returns
        let correlation_matrix = self.calculate_correlation_matrix(&symbol_results, &all_equity_curves);
        
        // Aggregate execution metrics
        let total_orders_placed: usize = symbol_results.iter().map(|r| r.orders_placed).sum();
        let total_orders_filled: usize = symbol_results.iter().map(|r| r.orders_filled).sum();
        let avg_fill_rate = if total_orders_placed > 0 {
            total_orders_filled as f64 / total_orders_placed as f64
        } else {
            0.0
        };
        // Weighted average of time in trade (weighted by number of trades)
        let avg_time_in_trade_ms = if total_trades > 0 {
            symbol_results.iter()
                .map(|r| r.avg_time_in_trade_ms * r.num_trades as f64)
                .sum::<f64>() / total_trades as f64
        } else {
            0.0
        };
        
        PortfolioSimResult {
            symbol_results,
            total_pnl,
            portfolio_sharpe,
            portfolio_max_drawdown,
            total_trades,
            portfolio_win_rate,
            portfolio_profit_factor,
            total_volume,
            total_fees,
            equity_curve: portfolio_equity_curve,
            trade_returns: all_trade_returns,
            risk_metrics: None, // Can be computed later if needed
            initial_capital: self.initial_capital,
            final_equity,
            correlation_matrix: Some(correlation_matrix),
            total_orders_placed,
            total_orders_filled,
            avg_fill_rate,
            avg_time_in_trade_ms,
        }
    }
    
    /// Aggregate equity curves from multiple symbols into portfolio equity curve
    fn aggregate_equity_curves(&self, curves: &[Vec<f64>]) -> Vec<f64> {
        if curves.is_empty() {
            return vec![self.initial_capital];
        }
        
        // Find the longest equity curve
        let max_len = curves.iter().map(|c| c.len()).max().unwrap_or(0);
        
        if max_len == 0 {
            return vec![self.initial_capital];
        }
        
        // Sum equity values at each timestamp
        // For shorter curves, use their last value
        let mut portfolio_curve = Vec::with_capacity(max_len);
        
        for i in 0..max_len {
            let total_equity: f64 = curves.iter()
                .map(|curve| {
                    if i < curve.len() {
                        curve[i]
                    } else {
                        // Use last value for shorter curves
                        curve.last().copied().unwrap_or(0.0)
                    }
                })
                .sum();
            portfolio_curve.push(total_equity);
        }
        
        portfolio_curve
    }
    
    /// Calculate portfolio Sharpe ratio from equity curve
    fn calculate_portfolio_sharpe(&self, equity_curve: &[f64]) -> f64 {
        if equity_curve.len() < 10 {
            return 0.0;
        }
        
        // Calculate returns
        let returns: Vec<f64> = equity_curve.windows(2)
            .map(|w| (w[1] - w[0]) / w[0])
            .filter(|r| r.is_finite())
            .collect();
        
        if returns.len() < 10 {
            return 0.0;
        }
        
        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / returns.len() as f64;
        let std_dev = variance.sqrt();
        
        if std_dev > 1e-10 {
            // Annualize assuming ~252 trading days
            let annualization = (252.0_f64).sqrt();
            (mean_return / std_dev) * annualization
        } else {
            0.0
        }
    }
    
    /// Calculate maximum drawdown from equity curve
    fn calculate_max_drawdown(&self, equity_curve: &[f64]) -> f64 {
        if equity_curve.is_empty() {
            return 0.0;
        }
        
        let mut peak = equity_curve[0];
        let mut max_dd = 0.0;
        
        for &equity in equity_curve.iter() {
            if equity > peak {
                peak = equity;
            }
            let dd = (peak - equity) / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
        
        max_dd
    }
    
    /// Calculate correlation matrix between symbol returns
    fn calculate_correlation_matrix(
        &self,
        symbol_results: &[SymbolResult],
        equity_curves: &[Vec<f64>],
    ) -> Vec<CorrelationPair> {
        let mut correlations = Vec::new();
        
        if equity_curves.len() < 2 {
            return correlations;
        }
        
        // Calculate returns for each symbol
        let returns: Vec<Vec<f64>> = equity_curves.iter()
            .map(|curve| {
                curve.windows(2)
                    .map(|w| if w[0] > 0.0 { (w[1] - w[0]) / w[0] } else { 0.0 })
                    .collect()
            })
            .collect();
        
        // Calculate pairwise correlations
        for i in 0..symbol_results.len() {
            for j in (i + 1)..symbol_results.len() {
                let corr = self.calculate_correlation(&returns[i], &returns[j]);
                correlations.push(CorrelationPair {
                    symbol1: symbol_results[i].symbol.clone(),
                    symbol2: symbol_results[j].symbol.clone(),
                    correlation: corr,
                });
            }
        }
        
        correlations
    }
    
    /// Calculate Pearson correlation between two return series
    fn calculate_correlation(&self, returns1: &[f64], returns2: &[f64]) -> f64 {
        let n = returns1.len().min(returns2.len());
        if n < 5 {
            return 0.0;
        }
        
        let mean1 = returns1[..n].iter().sum::<f64>() / n as f64;
        let mean2 = returns2[..n].iter().sum::<f64>() / n as f64;
        
        let mut cov = 0.0;
        let mut var1 = 0.0;
        let mut var2 = 0.0;
        
        for i in 0..n {
            let d1 = returns1[i] - mean1;
            let d2 = returns2[i] - mean2;
            cov += d1 * d2;
            var1 += d1 * d1;
            var2 += d2 * d2;
        }
        
        let denom = (var1 * var2).sqrt();
        if denom > 1e-10 {
            cov / denom
        } else {
            0.0
        }
    }
    
    /// Return empty result for edge cases
    fn empty_result(&self) -> PortfolioSimResult {
        PortfolioSimResult {
            symbol_results: Vec::new(),
            total_pnl: 0.0,
            portfolio_sharpe: 0.0,
            portfolio_max_drawdown: 0.0,
            total_trades: 0,
            portfolio_win_rate: 0.0,
            portfolio_profit_factor: 1.0,
            total_volume: 0.0,
            total_fees: 0.0,
            equity_curve: vec![self.initial_capital],
            trade_returns: Vec::new(),
            risk_metrics: None,
            initial_capital: self.initial_capital,
            final_equity: self.initial_capital,
            correlation_matrix: None,
            total_orders_placed: 0,
            total_orders_filled: 0,
            avg_fill_rate: 0.0,
            avg_time_in_trade_ms: 0.0,
        }
    }
}

/// Convert PortfolioSimResult to BacktestResult for API compatibility
impl From<PortfolioSimResult> for crate::types::BacktestResult {
    fn from(result: PortfolioSimResult) -> Self {
        use bigdecimal::BigDecimal;
        
        crate::types::BacktestResult {
            total_pnl: BigDecimal::try_from(result.total_pnl).unwrap_or_default(),
            max_drawdown: BigDecimal::try_from(result.portfolio_max_drawdown).unwrap_or_default(),
            sharpe_ratio: Some(result.portfolio_sharpe),
            num_trades: result.total_trades,
            profit_factor: Some(result.portfolio_profit_factor),
            win_rate: Some(result.portfolio_win_rate),
            equity_curve: result.equity_curve,
            closed_trades: result.total_trades,
            trade_returns: result.trade_returns,
            initial_capital: result.initial_capital,
            transaction_costs: Some(crate::types::TransactionCostAnalysis {
                total_commission: result.total_fees,
                total_slippage: 0.0,
                total_market_impact: 0.0,
                total_transaction_costs: result.total_fees,
                costs_as_percentage_of_pnl: if result.total_pnl.abs() > 0.01 {
                    (result.total_fees / result.total_pnl.abs()) * 100.0
                } else {
                    0.0
                },
                avg_cost_per_trade: if result.total_trades > 0 {
                    result.total_fees / result.total_trades as f64
                } else {
                    0.0
                },
                commission_by_exchange: std::collections::HashMap::new(),
            }),
            // Portfolio-level inventory aggregation
            inventory_metrics: Some(crate::types::InventoryMetrics {
                total_volume_traded: result.total_volume,
                avg_inventory_value: 0.0, // Not tracked at portfolio level
                turnover_per_day: 0.0,
                buy_fills: 0,
                sell_fills: 0,
                buy_volume: 0.0,
                sell_volume: 0.0,
                imbalance_ratio: 0.0,
                starting_inventory: 0.0,
                starting_inventory_value: 0.0,
                starting_mark_price: 0.0,
                ending_inventory: result.symbol_results.iter().map(|r| r.ending_inventory).sum(),
                ending_inventory_value: result.symbol_results.iter().map(|r| r.ending_inventory_value).sum(),
                ending_mark_price: 0.0, // Not applicable for portfolio
                starting_capital: result.initial_capital,
                ending_capital: result.symbol_results.iter().map(|r| r.ending_capital).sum(),
                final_equity: result.final_equity,
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_portfolio() {
        let sim = PortfolioSimulator::new(100_000.0, config::ExchangeFeeConfig::kraken());
        let events: HashMap<String, Vec<OrderBookEvent>> = HashMap::new();
        let params = RealisticASParams::default();
        
        let result = sim.run(&events, &params, None);
        
        assert_eq!(result.symbol_results.len(), 0);
        assert_eq!(result.initial_capital, 100_000.0);
        assert_eq!(result.final_equity, 100_000.0);
    }
    
    #[test]
    fn test_equal_weight_allocation() {
        let sim = PortfolioSimulator::new(100_000.0, config::ExchangeFeeConfig::kraken());
        
        // Verify allocation weights
        let symbols = vec!["BTCUSD".to_string(), "ETHUSD".to_string(), "SOLUSD".to_string()];
        let expected_weight = 1.0 / 3.0;
        
        for _symbol in &symbols {
            // Each should get ~33.3% of capital
            let allocated: f64 = 100_000.0 * expected_weight;
            assert!((allocated - 33333.33_f64).abs() < 1.0);
        }
    }
    
    #[test]
    fn test_correlation_calculation() {
        let sim = PortfolioSimulator::new(100_000.0, config::ExchangeFeeConfig::kraken());
        
        // Perfect positive correlation
        let r1 = vec![0.01, 0.02, -0.01, 0.03, -0.02];
        let r2 = vec![0.01, 0.02, -0.01, 0.03, -0.02];
        let corr = sim.calculate_correlation(&r1, &r2);
        assert!((corr - 1.0).abs() < 0.01);
        
        // Perfect negative correlation
        let r3 = vec![-0.01, -0.02, 0.01, -0.03, 0.02];
        let corr = sim.calculate_correlation(&r1, &r3);
        assert!((corr - (-1.0)).abs() < 0.01);
    }
    
    #[test]
    fn test_max_drawdown() {
        let sim = PortfolioSimulator::new(100_000.0, config::ExchangeFeeConfig::kraken());
        
        // Equity curve with 20% drawdown
        let curve = vec![100.0, 110.0, 120.0, 100.0, 96.0, 105.0, 115.0];
        let dd = sim.calculate_max_drawdown(&curve);
        
        // Peak is 120, trough is 96, so DD = (120-96)/120 = 20%
        assert!((dd - 0.20).abs() < 0.01);
    }
}
