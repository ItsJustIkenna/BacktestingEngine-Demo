//! Portfolio Genetic Algorithm Optimization
//!
//! Multi-phase optimization for portfolio backtests:
//! - Phase 1: Per-symbol GA optimization (parallel)
//! - Phase 2: Allocation weight optimization
//! - Final: Combined portfolio simulation
//!
//! Supports shared vs per-symbol parameters and customizable fitness functions.

use crate::chromosome::AvellanedaStoikovChromosome;
use crate::{Chromosome, FitnessResult, GeneticOptimizer, AsyncFitnessFn, ProgressCallback};
use config::GeneticConfig;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Parameters that should be shared across all symbols in a portfolio
/// These represent portfolio-level risk preferences
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedParams {
    /// Risk aversion (gamma) - portfolio-level risk tolerance
    pub risk_aversion: f64,
    /// Volatility adjustment factor - how much to respond to vol changes
    pub volatility_adjustment_factor: f64,
    /// Inventory penalty factor - how aggressively to rebalance
    pub inventory_penalty_factor: f64,
    /// Take profit percentage (0.0 = disabled)
    pub take_profit_pct: f64,
    /// Stop loss percentage (0.0 = disabled)
    pub stop_loss_pct: f64,
    /// Maximum trades per minute across portfolio
    pub max_trades_per_minute: f64,
}

impl Default for SharedParams {
    fn default() -> Self {
        Self {
            risk_aversion: 0.1,
            volatility_adjustment_factor: 0.5,
            inventory_penalty_factor: 0.5,
            take_profit_pct: 0.0,
            stop_loss_pct: 0.0,
            max_trades_per_minute: 30.0,
        }
    }
}

impl SharedParams {
    /// Extract shared params from a chromosome
    pub fn from_chromosome(chr: &AvellanedaStoikovChromosome) -> Self {
        Self {
            risk_aversion: chr.risk_aversion,
            volatility_adjustment_factor: chr.volatility_adjustment_factor,
            inventory_penalty_factor: chr.inventory_penalty_factor,
            take_profit_pct: chr.take_profit_pct,
            stop_loss_pct: chr.stop_loss_pct,
            max_trades_per_minute: chr.max_trades_per_minute,
        }
    }
    
    /// Apply shared params to a chromosome
    pub fn apply_to_chromosome(&self, chr: &mut AvellanedaStoikovChromosome) {
        chr.risk_aversion = self.risk_aversion;
        chr.volatility_adjustment_factor = self.volatility_adjustment_factor;
        chr.inventory_penalty_factor = self.inventory_penalty_factor;
        chr.take_profit_pct = self.take_profit_pct;
        chr.stop_loss_pct = self.stop_loss_pct;
        chr.max_trades_per_minute = self.max_trades_per_minute;
    }
}

/// Parameters that differ per symbol based on market characteristics
/// These are optimized independently for each symbol
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerSymbolParams {
    pub order_size_pct: f64,
    pub window_ms: u64,
    pub min_quote_lifetime_ms: u64,
    pub inventory_target: f64,
    pub volatility_cap: f64,
    pub time_horizon_minutes: f64,
    pub market_depth_k: f64,
    pub trend_sensitivity: f64,
    pub flow_sensitivity: f64,
    pub min_spread_bps: f64,
    pub adverse_selection_sensitivity: f64,
    pub fee_spread_multiplier: f64,
}

impl PerSymbolParams {
    /// Extract per-symbol params from a chromosome
    pub fn from_chromosome(chr: &AvellanedaStoikovChromosome) -> Self {
        Self {
            order_size_pct: chr.order_size_pct,
            window_ms: chr.window_ms,
            min_quote_lifetime_ms: chr.min_quote_lifetime_ms,
            inventory_target: chr.inventory_target,
            volatility_cap: chr.volatility_cap,
            time_horizon_minutes: chr.time_horizon_minutes,
            market_depth_k: chr.market_depth_k,
            trend_sensitivity: chr.trend_sensitivity,
            flow_sensitivity: chr.flow_sensitivity,
            min_spread_bps: chr.min_spread_bps,
            adverse_selection_sensitivity: chr.adverse_selection_sensitivity,
            fee_spread_multiplier: chr.fee_spread_multiplier,
        }
    }
    
    /// Apply per-symbol params to a chromosome
    pub fn apply_to_chromosome(&self, chr: &mut AvellanedaStoikovChromosome) {
        chr.order_size_pct = self.order_size_pct;
        chr.window_ms = self.window_ms;
        chr.min_quote_lifetime_ms = self.min_quote_lifetime_ms;
        chr.inventory_target = self.inventory_target;
        chr.volatility_cap = self.volatility_cap;
        chr.time_horizon_minutes = self.time_horizon_minutes;
        chr.market_depth_k = self.market_depth_k;
        chr.trend_sensitivity = self.trend_sensitivity;
        chr.flow_sensitivity = self.flow_sensitivity;
        chr.min_spread_bps = self.min_spread_bps;
        chr.adverse_selection_sensitivity = self.adverse_selection_sensitivity;
        chr.fee_spread_multiplier = self.fee_spread_multiplier;
    }
}

/// Chromosome for allocation weight optimization (Phase 2)
/// Weights are normalized to sum to 1.0
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllocationChromosome {
    /// Raw weights (will be normalized)
    pub weights: Vec<f64>,
    /// Number of symbols
    pub num_symbols: usize,
}

impl AllocationChromosome {
    pub fn new(num_symbols: usize) -> Self {
        let weights = vec![1.0 / num_symbols as f64; num_symbols];
        Self { weights, num_symbols }
    }
    
    /// Get normalized weights that sum to 1.0
    pub fn normalized_weights(&self) -> Vec<f64> {
        let sum: f64 = self.weights.iter().sum();
        if sum <= 0.0 {
            return vec![1.0 / self.num_symbols as f64; self.num_symbols];
        }
        self.weights.iter().map(|w| w / sum).collect()
    }
    
    /// Enforce minimum weight per asset (e.g., 5% minimum)
    pub fn enforce_min_weight(&mut self, min_weight: f64) {
        let min_raw = min_weight * self.num_symbols as f64;
        for w in &mut self.weights {
            if *w < min_raw {
                *w = min_raw;
            }
        }
    }
}

impl Chromosome for AllocationChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        // This will be overridden with proper num_symbols
        let num_symbols = 2; // Default, will be set properly
        let weights: Vec<f64> = (0..num_symbols)
            .map(|_| rng.gen_range(0.1..1.0))
            .collect();
        Self { weights, num_symbols }
    }
    
    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
        let weights: Vec<f64> = self.weights.iter()
            .zip(other.weights.iter())
            .map(|(a, b)| if rng.gen_bool(0.5) { *a } else { *b })
            .collect();
        Self { weights, num_symbols: self.num_symbols }
    }
    
    fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64) {
        for w in &mut self.weights {
            if rng.gen::<f64>() < mutation_rate {
                // Mutate by +/- 20%
                let factor = rng.gen_range(0.8..1.2);
                *w = (*w * factor).clamp(0.05, 2.0);
            }
        }
    }
    
    fn distance(&self, other: &Self) -> f64 {
        self.weights.iter()
            .zip(other.weights.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}

/// Factory for creating AllocationChromosome with proper num_symbols
pub struct AllocationChromosomeFactory {
    pub num_symbols: usize,
    pub min_weight: f64,
}

impl AllocationChromosomeFactory {
    pub fn new(num_symbols: usize) -> Self {
        Self { 
            num_symbols, 
            min_weight: 0.05, // 5% minimum per symbol
        }
    }
    
    pub fn random(&self, rng: &mut impl Rng) -> AllocationChromosome {
        let weights: Vec<f64> = (0..self.num_symbols)
            .map(|_| rng.gen_range(0.1..1.0))
            .collect();
        let mut chr = AllocationChromosome { 
            weights, 
            num_symbols: self.num_symbols 
        };
        chr.enforce_min_weight(self.min_weight);
        chr
    }
}

/// Customizable portfolio fitness configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioFitnessConfig {
    /// Weight for portfolio Sharpe ratio
    pub sharpe_weight: f64,
    /// Weight for portfolio Sortino ratio
    pub sortino_weight: f64,
    /// Weight for total net profit
    pub net_profit_weight: f64,
    /// Weight for portfolio profit factor
    pub profit_factor_weight: f64,
    /// Weight for portfolio win rate
    pub win_rate_weight: f64,
    /// Weight for max drawdown (negative correlation - lower is better)
    pub max_drawdown_weight: f64,
    /// Weight for diversification bonus (based on correlation)
    pub diversification_weight: f64,
    /// Weight for Calmar ratio
    pub calmar_weight: f64,
    /// Weight for number of trades (penalizes low-frequency strategies)
    pub num_trades_weight: f64,
    /// Weight for fill rate (penalizes wide spreads that don't get filled)
    pub fill_rate_weight: f64,
    /// Minimum number of trades required (per symbol)
    pub min_trades_per_symbol: usize,
    /// Target trades per day for scoring (used to normalize num_trades score)
    pub target_trades_per_day: f64,
    /// Maximum acceptable drawdown (absolute value)
    pub max_acceptable_drawdown: f64,
}

impl Default for PortfolioFitnessConfig {
    fn default() -> Self {
        Self {
            sharpe_weight: 0.10,
            sortino_weight: 0.05,
            net_profit_weight: 0.20,
            profit_factor_weight: 0.10,
            win_rate_weight: 0.05,  // Reduced - high win_rate weight causes wide spread exploit
            max_drawdown_weight: 0.10,
            diversification_weight: 0.05,
            calmar_weight: 0.10,
            num_trades_weight: 0.10,  // Important for market making
            fill_rate_weight: 0.15,   // Critical - penalizes wide spreads
            min_trades_per_symbol: 1000,  // ~33 trades/symbol/day over 30 days - prevents "do nothing" exploit
            target_trades_per_day: 33.0,  // Target ~33 trades/day for active market making
            max_acceptable_drawdown: 0.25,
        }
    }
}

impl PortfolioFitnessConfig {
    /// Create from job params fitness weights
    pub fn from_weights(weights: &crate::chromosome::AvellanedaStoikovChromosome) -> Self {
        Self::default() // Use defaults, can be extended later
    }
    
    /// Calculate fitness from portfolio metrics
    pub fn calculate_fitness(
        &self,
        sharpe: f64,
        sortino: Option<f64>,
        net_profit: f64,
        profit_factor: f64,
        win_rate: f64,
        max_drawdown: f64,
        diversification_ratio: Option<f64>,
        calmar: Option<f64>,
        total_trades: usize,
        num_symbols: usize,
        fill_rate: Option<f64>,
    ) -> f64 {
        // Check minimum trades constraint - HARD cutoff to prevent "do nothing" exploit
        let avg_trades_per_symbol = total_trades as f64 / num_symbols.max(1) as f64;
        
        // Hard rejection for strategies that don't trade enough
        // This prevents the GA from exploiting wide spreads to avoid losses
        if total_trades < self.min_trades_per_symbol {
            return f64::NEG_INFINITY; // Reject solutions with too few trades
        }
        
        // Check maximum drawdown constraint
        if max_drawdown > self.max_acceptable_drawdown {
            return -0.5; // Penalty for excessive drawdown
        }
        
        // Normalize metrics
        let sharpe_score = sharpe.clamp(-2.0, 5.0) / 5.0;
        let sortino_score = sortino.unwrap_or(0.0).clamp(-2.0, 5.0) / 5.0;
        let profit_score = (net_profit / 10000.0).clamp(-1.0, 2.0); // Relative to 10k capital
        let pf_score = (profit_factor - 1.0).clamp(-0.5, 1.0); // 1.0 is breakeven
        let wr_score = (win_rate - 0.5).clamp(-0.2, 0.3) * 2.0; // Normalize around 50%
        let dd_score = (0.25 - max_drawdown).clamp(-0.25, 0.25); // Lower DD is better
        let div_score = diversification_ratio.unwrap_or(1.0).clamp(0.5, 2.0) - 1.0;
        let calmar_score = calmar.unwrap_or(0.0).clamp(-2.0, 5.0) / 5.0;
        
        // Trade frequency score: reward strategies that trade more
        // Score 0.0 at 0 trades/symbol/day, 1.0 at target_trades_per_day, caps at 1.5
        // Assumes 30-day backtest period for normalization
        let trades_per_symbol_per_day = avg_trades_per_symbol / 30.0;
        let trade_freq_score = (trades_per_symbol_per_day / self.target_trades_per_day)
            .clamp(0.0, 1.5);
        
        // Fill rate score: reward strategies with higher fill rates
        // Score 0.0 at 0% fill rate, 1.0 at 20% fill rate, caps at 1.5
        // Market making typically has 5-30% fill rates
        let fill_rate_val = fill_rate.unwrap_or(0.0);
        let fill_rate_score = (fill_rate_val / 0.20).clamp(0.0, 1.5);
        
        // Severe penalty for extremely low fill rates (< 1%)
        let fill_rate_penalty = if fill_rate_val < 0.01 {
            -0.3 * (1.0 - fill_rate_val / 0.01)
        } else {
            0.0
        };
        
        // Additional penalty if below minimum trades per symbol
        let trade_penalty = if avg_trades_per_symbol < self.min_trades_per_symbol as f64 {
            -0.2 * (1.0 - avg_trades_per_symbol / self.min_trades_per_symbol as f64)
        } else {
            0.0
        };
        
        // Weighted sum
        let fitness = 
            self.sharpe_weight * sharpe_score +
            self.sortino_weight * sortino_score +
            self.net_profit_weight * profit_score +
            self.profit_factor_weight * pf_score +
            self.win_rate_weight * wr_score +
            self.max_drawdown_weight * dd_score +
            self.diversification_weight * div_score +
            self.calmar_weight * calmar_score +
            self.num_trades_weight * trade_freq_score +
            self.fill_rate_weight * fill_rate_score +
            trade_penalty +
            fill_rate_penalty;
        
        fitness
    }
}

/// Results from per-symbol optimization phase
#[derive(Clone, Debug)]
pub struct SymbolOptimizationResult {
    /// Asset key (symbol@exchange)
    pub asset_key: String,
    /// Best chromosome found for this symbol
    pub chromosome: AvellanedaStoikovChromosome,
    /// Best fitness achieved
    pub fitness: f64,
    /// Key metrics
    pub sharpe_ratio: f64,
    pub net_pnl: f64,
    pub num_trades: i32,
    pub max_drawdown: f64,
}

/// Results from the full portfolio optimization
#[derive(Clone, Debug)]
pub struct PortfolioOptimizationResult {
    /// Shared parameters (applied to all symbols)
    pub shared_params: SharedParams,
    /// Per-symbol optimized parameters
    pub symbol_params: HashMap<String, PerSymbolParams>,
    /// Per-symbol chromosomes (for detailed backtesting)
    pub symbol_chromosomes: HashMap<String, AvellanedaStoikovChromosome>,
    /// Optimized allocation weights (normalized, sum to 1.0)
    pub allocation_weights: Vec<f64>,
    /// Symbol order matching weights
    pub symbols: Vec<String>,
    /// Portfolio-level fitness
    pub portfolio_fitness: f64,
    /// Per-symbol optimization results
    pub symbol_results: Vec<SymbolOptimizationResult>,
}

impl PortfolioOptimizationResult {
    /// Get the chromosome for a specific symbol
    pub fn get_chromosome(&self, asset_key: &str) -> Option<&AvellanedaStoikovChromosome> {
        self.symbol_chromosomes.get(asset_key)
    }
    
    /// Get allocation weight for a symbol
    pub fn get_weight(&self, asset_key: &str) -> f64 {
        self.symbols.iter()
            .position(|s| s == asset_key)
            .map(|idx| self.allocation_weights.get(idx).copied().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
}

/// Portfolio GA optimizer that orchestrates multi-phase optimization
pub struct PortfolioGAOptimizer {
    pub config: GeneticConfig,
    pub fitness_config: PortfolioFitnessConfig,
    pub job_id: String,
}

impl PortfolioGAOptimizer {
    pub fn new(config: GeneticConfig, fitness_config: PortfolioFitnessConfig, job_id: String) -> Self {
        Self { config, fitness_config, job_id }
    }
    
    /// Phase 1: Run per-symbol GA optimization in parallel
    /// Returns optimized chromosomes for each symbol
    pub async fn optimize_symbols_parallel<F>(
        &self,
        symbols: &[String],
        fitness_fn_factory: F,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<Vec<SymbolOptimizationResult>, Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn(&str) -> AsyncFitnessFn<AvellanedaStoikovChromosome> + Send + Sync + 'static,
    {
        use tokio::task::JoinSet;
        
        let num_symbols = symbols.len();
        let config = self.config.clone();
        let job_id = self.job_id.clone();
        
        // Create join set for parallel symbol optimization
        let mut join_set: JoinSet<Result<SymbolOptimizationResult, Box<dyn std::error::Error + Send + Sync>>> = JoinSet::new();
        
        for (idx, symbol) in symbols.iter().enumerate() {
            let symbol = symbol.clone();
            let config = config.clone();
            let job_id = job_id.clone();
            let fitness_fn = fitness_fn_factory(&symbol);
            
            join_set.spawn(async move {
                eprintln!("📊 [{}] Starting GA for symbol {}/{}: {}", job_id, idx + 1, num_symbols, symbol);
                
                let optimizer = GeneticOptimizer::<AvellanedaStoikovChromosome> {
                    config,
                    fitness_fn,
                    progress_callback: None, // Per-symbol progress not needed
                    job_id: Some(format!("{}-{}", job_id, symbol)),
                };
                
                let (best_chromosome, best_fitness) = optimizer.run().await;
                
                // Get final metrics by evaluating best chromosome
                let final_result = (optimizer.fitness_fn)(&best_chromosome).await;
                
                eprintln!("✅ [{}] Symbol {} complete: fitness={:.4}, sharpe={:.3}, pnl=${:.2}", 
                    job_id, symbol, best_fitness, final_result.sharpe_ratio, final_result.net_pnl);
                
                Ok(SymbolOptimizationResult {
                    asset_key: symbol,
                    chromosome: best_chromosome,
                    fitness: best_fitness,
                    sharpe_ratio: final_result.sharpe_ratio,
                    net_pnl: final_result.net_pnl,
                    num_trades: final_result.num_trades,
                    max_drawdown: final_result.max_drawdown,
                })
            });
        }
        
        // Collect results
        let mut results = Vec::with_capacity(num_symbols);
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(opt_result)) => results.push(opt_result),
                Ok(Err(e)) => {
                    eprintln!("❌ [{}] Symbol optimization error: {}", self.job_id, e);
                    return Err(e);
                }
                Err(e) => {
                    eprintln!("❌ [{}] Join error: {}", self.job_id, e);
                    return Err(format!("Task join error: {}", e).into());
                }
            }
        }
        
        // Sort by original symbol order
        results.sort_by(|a, b| {
            let idx_a = symbols.iter().position(|s| s == &a.asset_key).unwrap_or(usize::MAX);
            let idx_b = symbols.iter().position(|s| s == &b.asset_key).unwrap_or(usize::MAX);
            idx_a.cmp(&idx_b)
        });
        
        Ok(results)
    }
    
    /// Phase 2: Optimize allocation weights given fixed per-symbol params
    /// Uses a separate GA to find optimal capital allocation
    pub async fn optimize_allocation_weights<F>(
        &self,
        symbol_chromosomes: &HashMap<String, AvellanedaStoikovChromosome>,
        symbols: &[String],
        portfolio_fitness_fn: F,
    ) -> Result<Vec<f64>, Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn(&[f64]) -> std::pin::Pin<Box<dyn std::future::Future<Output = FitnessResult> + Send>> + Send + Sync + 'static,
    {
        let num_symbols = symbols.len();
        let factory = AllocationChromosomeFactory::new(num_symbols);
        let portfolio_fitness_fn = Arc::new(portfolio_fitness_fn);
        
        // Smaller config for weight optimization (faster)
        let weight_config = GeneticConfig {
            population_size: 30,
            generations: 15,
            mutation_rate: 0.15,
            crossover_rate: 0.8,
            ..self.config.clone()
        };
        
        eprintln!("📊 [{}] Starting allocation weight optimization for {} symbols", self.job_id, num_symbols);
        
        // Create fitness function for allocation optimization
        let fitness_fn: AsyncFitnessFn<AllocationChromosome> = {
            let portfolio_fitness_fn = Arc::clone(&portfolio_fitness_fn);
            Arc::new(move |chr: &AllocationChromosome| {
                let weights = chr.normalized_weights();
                let fitness_fn = Arc::clone(&portfolio_fitness_fn);
                Box::pin(async move {
                    fitness_fn(&weights).await
                })
            })
        };
        
        // Run GA for weight optimization
        let mut rng = rand::rngs::StdRng::from_entropy();
        let mut population: Vec<AllocationChromosome> = (0..weight_config.population_size)
            .map(|_| factory.random(&mut rng))
            .collect();
        
        let mut best_weights = vec![1.0 / num_symbols as f64; num_symbols];
        let mut best_fitness = f64::NEG_INFINITY;
        
        for gen in 0..weight_config.generations {
            // Evaluate population
            let mut fitness_results = Vec::with_capacity(population.len());
            for chr in &population {
                let result = fitness_fn(chr).await;
                fitness_results.push((chr.clone(), result.fitness));
            }
            
            // Sort by fitness
            fitness_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            
            // Update best
            if let Some((best_chr, fitness)) = fitness_results.first() {
                if *fitness > best_fitness {
                    best_fitness = *fitness;
                    best_weights = best_chr.normalized_weights();
                    eprintln!("📊 [{}] Weight Gen {}: fitness={:.4}, weights={:?}", 
                        self.job_id, gen + 1, best_fitness, best_weights);
                }
            }
            
            // Selection + crossover + mutation for next generation
            let elite_size = (weight_config.population_size as f64 * 0.1).ceil() as usize;
            let mut next_pop: Vec<AllocationChromosome> = fitness_results.iter()
                .take(elite_size)
                .map(|(chr, _)| chr.clone())
                .collect();
            
            while next_pop.len() < weight_config.population_size {
                let idx1 = rng.gen_range(0..fitness_results.len().min(10));
                let idx2 = rng.gen_range(0..fitness_results.len().min(10));
                let mut child = fitness_results[idx1].0.crossover(&fitness_results[idx2].0, &mut rng);
                child.mutate(&mut rng, weight_config.mutation_rate);
                child.enforce_min_weight(0.05);
                next_pop.push(child);
            }
            
            population = next_pop;
        }
        
        eprintln!("✅ [{}] Weight optimization complete: weights={:?}", self.job_id, best_weights);
        Ok(best_weights)
    }
    
    /// Extract shared params from the best-performing symbol's chromosome
    /// or average across all chromosomes
    pub fn extract_shared_params(
        &self,
        symbol_results: &[SymbolOptimizationResult],
    ) -> SharedParams {
        if symbol_results.is_empty() {
            return SharedParams::default();
        }
        
        // Use the best-performing symbol's shared params
        let best = symbol_results.iter()
            .max_by(|a, b| a.fitness.partial_cmp(&b.fitness).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        
        SharedParams::from_chromosome(&best.chromosome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_allocation_chromosome_normalization() {
        let chr = AllocationChromosome {
            weights: vec![0.3, 0.5, 0.2],
            num_symbols: 3,
        };
        let normalized = chr.normalized_weights();
        let sum: f64 = normalized.iter().sum();
        assert!((sum - 1.0).abs() < 0.0001);
    }
    
    #[test]
    fn test_allocation_min_weight() {
        let mut chr = AllocationChromosome {
            weights: vec![0.01, 0.99, 0.0],
            num_symbols: 3,
        };
        chr.enforce_min_weight(0.1);
        let normalized = chr.normalized_weights();
        assert!(normalized.iter().all(|w| *w >= 0.05)); // After normalization
    }
    
    #[test]
    fn test_fitness_config_default_weights_sum() {
        let config = PortfolioFitnessConfig::default();
        let sum = config.sharpe_weight + config.sortino_weight + config.net_profit_weight +
            config.profit_factor_weight + config.win_rate_weight + config.max_drawdown_weight +
            config.diversification_weight + config.calmar_weight + config.num_trades_weight +
            config.fill_rate_weight;
        assert!((sum - 1.0).abs() < 0.01);
    }
}
