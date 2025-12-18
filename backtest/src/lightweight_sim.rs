//! Lightweight Simulation for GA Optimization
//!
//! Memory-efficient backtesting engine designed for genetic algorithm optimization.
//! Uses the `TickSource` trait from `dataloader` to work with memory-mapped files,
//! enabling backtesting on millions of data points without OOM.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    LightweightSim                           │
//! │                                                             │
//! │   data_prep::BinaryFileReader ──┬── dataloader::TickSource │
//! │           (mmap, zero-copy)     │                          │
//! │                                 ▼                          │
//! │                    ┌─────────────────────┐                 │
//! │                    │  Tick Iterator      │                 │
//! │                    │  (streaming access) │                 │
//! │                    └──────────┬──────────┘                 │
//! │                               │                            │
//! │                               ▼                            │
//! │                    ┌─────────────────────┐                 │
//! │                    │  A-S Strategy Core  │                 │
//! │                    │  (simplified logic) │                 │
//! │                    └──────────┬──────────┘                 │
//! │                               │                            │
//! │                               ▼                            │
//! │                    ┌─────────────────────┐                 │
//! │                    │  BacktestResult     │                 │
//! │                    │  (metrics for GA)   │                 │
//! │                    └─────────────────────┘                 │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Memory Usage
//!
//! - Binary file: Memory-mapped (OS handles paging, ~10 MB overhead)
//! - State: ~1 KB (position, P&L, metrics accumulators)
//! - Equity curve samples: ~8 KB (1000 f64 samples)
//! - **Total: ~20 KB constant regardless of dataset size**

use crate::types::BacktestResult;
use bigdecimal::{BigDecimal, FromPrimitive};
use dataloader::{Tick, TickSource};
use data_prep::{BinaryFileReader, SimulationTick};
use genetic::chromosome::AvellanedaStoikovChromosome;
use std::path::Path;

/// Configuration for lightweight simulation
#[derive(Debug, Clone)]
pub struct LightweightSimConfig {
    /// Initial capital in quote currency
    pub initial_capital: f64,
    /// Commission rate (e.g., 0.0004 = 4 bps)
    pub commission_rate: f64,
    /// Initial inventory as fraction of capital (0.5 = 50%)
    pub initial_inventory_pct: f64,
    /// Equity curve sample interval (every N ticks)
    pub equity_sample_interval: usize,
}

impl Default for LightweightSimConfig {
    fn default() -> Self {
        Self {
            initial_capital: 100_000.0,
            commission_rate: 0.0004,
            initial_inventory_pct: 0.5,
            equity_sample_interval: 1000,
        }
    }
}

/// Simulation state (minimal memory footprint)
struct SimState {
    cash: f64,
    inventory: f64,
    last_price: f64,
    realized_pnl: f64,
    trade_count: usize,
    winning_trades: usize,
    high_water_mark: f64,
    max_drawdown: f64,
    volatility: f64,
    last_day_ms: i64,
    day_start_equity: f64,
    daily_returns: Vec<f64>,
    equity_curve: Vec<f64>,
    sample_counter: usize,
}

impl SimState {
    fn new(initial_capital: f64, initial_inventory_pct: f64, first_price: f64) -> Self {
        let inventory_value = initial_capital * initial_inventory_pct;
        let cash = initial_capital - inventory_value;
        let inventory = if first_price > 0.0 { inventory_value / first_price } else { 0.0 };
        
        Self {
            cash,
            inventory,
            last_price: first_price,
            realized_pnl: 0.0,
            trade_count: 0,
            winning_trades: 0,
            high_water_mark: initial_capital,
            max_drawdown: 0.0,
            volatility: 0.2,
            last_day_ms: 0,
            day_start_equity: initial_capital,
            daily_returns: Vec::with_capacity(365),
            equity_curve: Vec::with_capacity(1000),
            sample_counter: 0,
        }
    }
    
    #[inline]
    fn equity(&self) -> f64 {
        self.cash + self.inventory * self.last_price
    }
}

/// Lightweight simulation engine for GA optimization
/// 
/// Uses `TickSource` trait for memory-efficient data access.
pub struct LightweightSim {
    config: LightweightSimConfig,
}

impl LightweightSim {
    /// Create a new lightweight simulation
    pub fn new(config: LightweightSimConfig) -> Self {
        Self { config }
    }
    
    /// Run simulation from a binary file (zero-copy mmap)
    pub fn run_from_file(
        &self,
        path: &Path,
        chromosome: &AvellanedaStoikovChromosome,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let reader = BinaryFileReader::open(path)?;
        self.run(&reader, chromosome)
    }
    
    /// Run simulation with any TickSource implementation
    pub fn run<S: TickSource<TickType = SimulationTick>>(
        &self,
        source: &S,
        chromosome: &AvellanedaStoikovChromosome,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        if source.is_empty() {
            return Err("Empty data source".into());
        }
        
        // Get first price for initialization
        let first_tick = source.get(0).ok_or("Cannot read first tick")?;
        let first_price = first_tick.price();
        
        let mut state = SimState::new(
            self.config.initial_capital,
            self.config.initial_inventory_pct,
            first_price,
        );
        
        // Process all ticks
        for tick in source.iter() {
            self.process_tick(tick, chromosome, &mut state);
        }
        
        // Finalize daily return if in the middle of a day
        if state.last_day_ms > 0 {
            let equity = state.equity();
            if state.day_start_equity > 0.0 {
                let daily_return = (equity - state.day_start_equity) / state.day_start_equity;
                state.daily_returns.push(daily_return);
            }
        }
        
        Ok(self.build_result(&state))
    }
    
    /// Process a single tick with A-S strategy logic
    #[inline]
    fn process_tick(
        &self,
        tick: &SimulationTick,
        chromosome: &AvellanedaStoikovChromosome,
        state: &mut SimState,
    ) {
        let price = tick.price();
        let timestamp_ms = tick.timestamp_ms();
        
        // Update volatility (EWMA)
        if state.last_price > 0.0 && price > 0.0 {
            let log_return = (price / state.last_price).ln().abs();
            let alpha = 0.06;
            state.volatility = alpha * log_return * (252.0 * 6.5 * 60.0_f64).sqrt() 
                             + (1.0 - alpha) * state.volatility;
        }
        state.last_price = price;
        
        // Avellaneda-Stoikov reservation price and spread
        let mid_price = price;
        let inventory_ratio = state.inventory / (self.config.initial_capital / mid_price);
        let reservation_price = mid_price - chromosome.risk_aversion * state.volatility * inventory_ratio;
        
        let base_spread = chromosome.risk_aversion * state.volatility.powi(2) 
                        + (2.0 / chromosome.risk_aversion).ln() / chromosome.market_depth_k;
        let spread = base_spread.max(chromosome.min_spread_bps / 10000.0 * mid_price);
        
        let bid_price = reservation_price - spread / 2.0;
        let ask_price = reservation_price + spread / 2.0;
        
        // Order size
        let order_size = self.config.initial_capital * chromosome.order_size_pct / mid_price;
        
        // Fill logic based on tick side
        let is_sell = tick.is_sell();
        
        if is_sell && price <= bid_price && state.cash >= price * order_size {
            // Sell aggressor hits our bid - we buy
            let fill_value = price * order_size;
            let commission = fill_value * self.config.commission_rate;
            state.cash -= fill_value + commission;
            state.inventory += order_size;
            state.realized_pnl -= commission;
            state.trade_count += 1;
        } else if !is_sell && price >= ask_price && state.inventory >= order_size {
            // Buy aggressor hits our ask - we sell
            let fill_value = price * order_size;
            let commission = fill_value * self.config.commission_rate;
            let cost_basis = state.last_price * order_size;
            let trade_pnl = fill_value - cost_basis - commission;
            
            state.cash += fill_value - commission;
            state.inventory -= order_size;
            state.realized_pnl += trade_pnl;
            state.trade_count += 1;
            
            if trade_pnl > 0.0 {
                state.winning_trades += 1;
            }
        }
        
        // Update metrics
        let equity = state.equity();
        
        if equity > state.high_water_mark {
            state.high_water_mark = equity;
        }
        let drawdown = (state.high_water_mark - equity) / state.high_water_mark;
        if drawdown > state.max_drawdown {
            state.max_drawdown = drawdown;
        }
        
        // Sample equity curve
        state.sample_counter += 1;
        if state.sample_counter % self.config.equity_sample_interval == 0 {
            state.equity_curve.push(equity);
        }
        
        // Daily aggregation for Sharpe
        let day_ms = timestamp_ms / 86_400_000 * 86_400_000;
        if day_ms != state.last_day_ms && state.last_day_ms != 0 {
            let daily_return = (equity - state.day_start_equity) / state.day_start_equity;
            state.daily_returns.push(daily_return);
            state.day_start_equity = equity;
        }
        state.last_day_ms = day_ms;
    }
    
    /// Build BacktestResult from simulation state
    fn build_result(&self, state: &SimState) -> BacktestResult {
        let equity = state.equity();
        let net_profit = equity - self.config.initial_capital;
        
        // Sharpe ratio from daily returns
        let sharpe_ratio = if state.daily_returns.len() > 1 {
            let mean: f64 = state.daily_returns.iter().sum::<f64>() / state.daily_returns.len() as f64;
            let variance: f64 = state.daily_returns.iter()
                .map(|r| (r - mean).powi(2))
                .sum::<f64>() / (state.daily_returns.len() - 1) as f64;
            let std_dev = variance.sqrt();
            if std_dev > 0.0 {
                Some(mean * 252.0_f64.sqrt() / std_dev)
            } else {
                Some(0.0)
            }
        } else {
            None
        };
        
        // Win rate
        let win_rate = if state.trade_count > 0 {
            Some(state.winning_trades as f64 / state.trade_count as f64)
        } else {
            None
        };
        
        // Volatility from daily returns
        let volatility = if state.daily_returns.len() > 1 {
            let mean: f64 = state.daily_returns.iter().sum::<f64>() / state.daily_returns.len() as f64;
            let variance: f64 = state.daily_returns.iter()
                .map(|r| (r - mean).powi(2))
                .sum::<f64>() / (state.daily_returns.len() - 1) as f64;
            Some(variance.sqrt() * 252.0_f64.sqrt())
        } else {
            None
        };
        
        // Calculate unrealized P&L
        let unrealized = state.inventory * state.last_price - 
            (self.config.initial_capital * self.config.initial_inventory_pct);
        
        BacktestResult {
            total_pnl: BigDecimal::from_f64(net_profit).unwrap_or_default(),
            num_trades: state.trade_count,
            closed_trades: state.trade_count,
            open_positions: if state.inventory.abs() > 0.0001 { 1 } else { 0 },
            first_trade_timestamp: None,
            last_trade_timestamp: None,
            first_trade_price: None,
            last_trade_price: Some(state.last_price),
            realized_pnl: Some(state.realized_pnl),
            unrealized_pnl: Some(unrealized),
            max_drawdown: BigDecimal::from_f64(state.max_drawdown * self.config.initial_capital)
                .unwrap_or_default(),
            sharpe_ratio,
            sortino_ratio: None,
            calmar_ratio: if state.max_drawdown > 0.0 {
                Some((net_profit / self.config.initial_capital) / state.max_drawdown)
            } else {
                None
            },
            profit_factor: None,
            win_rate,
            avg_trade_return: if state.trade_count > 0 {
                Some(net_profit / state.trade_count as f64)
            } else {
                None
            },
            median_trade_return: None,
            avg_trade_duration: None,
            volatility,
            gross_profit: if net_profit > 0.0 { Some(net_profit) } else { Some(0.0) },
            gross_loss: if net_profit < 0.0 { Some(net_profit.abs()) } else { Some(0.0) },
            net_profit: Some(net_profit),
            equity_curve: state.equity_curve.clone(),
            trade_returns: Vec::new(),
            initial_capital: self.config.initial_capital,
            data_quality: None,
            transaction_costs: None,
            market_impact: None,
            inventory_metrics: None,
            execution_metrics: None,
            risk_halt_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_config_default() {
        let config = LightweightSimConfig::default();
        assert!((config.initial_capital - 100_000.0).abs() < 0.01);
        assert!((config.commission_rate - 0.0004).abs() < 0.0001);
    }
}
