/// Genetic Algorithm optimization for trading strategies
/// 
/// Provides async genetic algorithm optimization for finding optimal
/// trading strategy parameters with parallel fitness evaluation.
/// 
/// Includes robustness-weighted fitness using Monte Carlo consistency checks
/// on top performers to prevent overfitting to lucky parameter combinations.
///
/// Performance optimizations:
/// - Adaptive sampling: Coarser data in early generations for faster exploration
/// - Dynamic concurrency: Memory-aware parallel evaluation limits
/// - SIMD utilities: Vectorized calculations for metrics

use std::sync::Arc;
use std::collections::HashMap;
use config::GeneticConfig;
use rand::prelude::*;
use ultra_logger::{UltraLogger, LoggerConfig, TransportConfig, ConnectionConfig};
use once_cell::sync::Lazy;

pub mod chromosome;

// Re-export chromosome types for external use
pub use chromosome::{AvellanedaStoikovChromosome, MomentumChromosome};

/// Portfolio-level genetic algorithm optimization
pub mod portfolio;

/// Regime-aware Monte Carlo simulation for robust backtesting
pub mod regime_mc;

/// Hidden Markov Model for probabilistic regime detection
pub mod hmm_regime;

/// Adaptive sampling and optimization utilities
pub mod adaptive_sampling;

pub use portfolio::{
    PortfolioGAOptimizer, PortfolioFitnessConfig, PortfolioOptimizationResult,
    SymbolOptimizationResult, SharedParams, PerSymbolParams,
    AllocationChromosome, AllocationChromosomeFactory,
};

pub use regime_mc::{
    RegimeMCConfig, RegimeMCResult, RegimeAwareMC, RegimeInfo,
    MarketRegime, run_regime_aware_monte_carlo,
};

pub use hmm_regime::{
    HMMConfig, HiddenMarkovModel, GaussianEmission, FitResult,
    HMMRegimeTracker, RegimeCharacteristics, RegimeUpdate,
};

pub use adaptive_sampling::{
    AdaptiveSamplingConfig, SampledData, sample_market_data,
    StableRegionDetector, DynamicConcurrency,
};

/// Create Elasticsearch configuration for genetic module
fn create_elasticsearch_config() -> LoggerConfig {
    // Check if we should use Elasticsearch (environment variable or default)
    let use_elasticsearch = std::env::var("USE_ELASTICSEARCH_LOGGING")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(true); // Default to true for production

    if use_elasticsearch {
        // Get credentials from environment variables set by Kubernetes
        let endpoint = std::env::var("ELASTICSEARCH_ENDPOINT")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_ENDPOINT"))
            .unwrap_or_else(|_| "http://localhost:9200".to_string());
        let username = std::env::var("ELASTICSEARCH_USERNAME")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_USERNAME"))
            .unwrap_or_else(|_| "elastic".to_string());
        let password = std::env::var("ELASTICSEARCH_PASSWORD")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_PASSWORD"))
            .unwrap_or_else(|_| "mN9CNxqYU9J0nw4aywXVxTAw".to_string());

        let mut options = HashMap::new();
        options.insert("index".to_string(), "backtestingengine-logs".to_string());

        LoggerConfig {
            level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            transport: TransportConfig {
                transport_type: "elasticsearch".to_string(),
                connection: ConnectionConfig {
                    host: endpoint,
                    port: 443,
                    username: Some(username),
                    password: Some(password),
                    options,
                },
            },
        }
    } else {
        // Fallback to stdout for development
        LoggerConfig::default()
    }
}

/// Logger for genetic algorithm operations with Elasticsearch support
static GENETIC_LOGGER: Lazy<Arc<UltraLogger>> = Lazy::new(|| {
    Arc::new(UltraLogger::with_config(
        "BacktestingEngine-genetic".to_string(),
        create_elasticsearch_config()
    ))
});

/// Log info message using tokio::spawn for async logging (matches worker's pattern)
macro_rules! log_info {
    ($($arg:tt)*) => {{
        let logger = GENETIC_LOGGER.clone();
        let msg = format!($($arg)*);
        tokio::spawn(async move {
            let _ = logger.info(msg).await;
        });
    }};
}

/// Represents a set of strategy parameters (chromosome)
pub trait Chromosome: Clone + Send + Sync {
    /// Randomly initialize a chromosome
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self;
    /// Crossover with another chromosome
    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self;
    /// Mutate the chromosome with adaptive mutation rate
    fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64);
    /// Calculate distance to another chromosome for diversity metrics
    fn distance(&self, other: &Self) -> f64;
}

/// Fitness result containing both the raw fitness and equity curve for robustness analysis
#[derive(Clone, Default)]
pub struct FitnessResult {
    pub fitness: f64,
    pub equity_curve: Vec<f64>,
    /// Additional backtest metrics for reporting
    pub net_pnl: f64,
    pub sharpe_ratio: f64,
    pub num_trades: i32,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    /// Total volume traded in USD
    pub total_volume: f64,
    /// Total fees paid in USD
    pub total_fees: f64,
    /// Best trade P&L in USD
    pub best_trade_pnl: f64,
    /// Worst trade P&L in USD
    pub worst_trade_pnl: f64,
    /// Average trade P&L in USD
    pub avg_trade_pnl: f64,
    /// Average time in trade in milliseconds
    pub avg_time_in_trade_ms: f64,
    /// Average inventory over backtest
    pub avg_inventory: f64,
    /// Maximum inventory held
    pub max_inventory: f64,
    /// Number of inventory zero crossings
    pub zero_crossings: i32,
    /// Average fill rate (0.0-1.0)
    pub avg_fill_rate: f64,
    /// Average queue position
    pub avg_queue_position: f64,
    /// Partial fill percentage (0.0-100.0)
    pub partial_fill_pct: f64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f64,
}

impl FitnessResult {
    pub fn new(fitness: f64, equity_curve: Vec<f64>) -> Self {
        Self { 
            fitness, 
            equity_curve,
            net_pnl: 0.0,
            sharpe_ratio: 0.0,
            num_trades: 0,
            max_drawdown: 0.0,
            win_rate: 0.0,
            profit_factor: 0.0,
            total_volume: 0.0,
            total_fees: 0.0,
            best_trade_pnl: 0.0,
            worst_trade_pnl: 0.0,
            avg_trade_pnl: 0.0,
            avg_time_in_trade_ms: 0.0,
            avg_inventory: 0.0,
            max_inventory: 0.0,
            zero_crossings: 0,
            avg_fill_rate: 0.0,
            avg_queue_position: 0.0,
            partial_fill_pct: 0.0,
            avg_latency_ms: 0.0,
        }
    }
    
    /// Create a FitnessResult with full metrics from a backtest
    pub fn with_metrics(
        fitness: f64,
        equity_curve: Vec<f64>,
        net_pnl: f64,
        sharpe_ratio: f64,
        num_trades: i32,
        max_drawdown: f64,
        win_rate: f64,
        profit_factor: f64,
    ) -> Self {
        Self {
            fitness,
            equity_curve,
            net_pnl,
            sharpe_ratio,
            num_trades,
            max_drawdown,
            win_rate,
            profit_factor,
            total_volume: 0.0,
            total_fees: 0.0,
            best_trade_pnl: 0.0,
            worst_trade_pnl: 0.0,
            avg_trade_pnl: 0.0,
            avg_time_in_trade_ms: 0.0,
            avg_inventory: 0.0,
            max_inventory: 0.0,
            zero_crossings: 0,
            avg_fill_rate: 0.0,
            avg_queue_position: 0.0,
            partial_fill_pct: 0.0,
            avg_latency_ms: 0.0,
        }
    }
    
    /// Create a FitnessResult with ALL metrics from a realistic simulation
    pub fn with_full_metrics(
        fitness: f64,
        equity_curve: Vec<f64>,
        net_pnl: f64,
        sharpe_ratio: f64,
        num_trades: i32,
        max_drawdown: f64,
        win_rate: f64,
        profit_factor: f64,
        total_volume: f64,
        total_fees: f64,
        best_trade_pnl: f64,
        worst_trade_pnl: f64,
        avg_trade_pnl: f64,
        avg_time_in_trade_ms: f64,
        avg_inventory: f64,
        max_inventory: f64,
        zero_crossings: i32,
        avg_fill_rate: f64,
        avg_queue_position: f64,
        partial_fill_pct: f64,
        avg_latency_ms: f64,
    ) -> Self {
        Self {
            fitness,
            equity_curve,
            net_pnl,
            sharpe_ratio,
            num_trades,
            max_drawdown,
            win_rate,
            profit_factor,
            total_volume,
            total_fees,
            best_trade_pnl,
            worst_trade_pnl,
            avg_trade_pnl,
            avg_time_in_trade_ms,
            avg_inventory,
            max_inventory,
            zero_crossings,
            avg_fill_rate,
            avg_queue_position,
            partial_fill_pct,
            avg_latency_ms,
        }
    }
    
    pub fn failure() -> Self {
        Self::default()
    }
}

/// Context passed to fitness function for adaptive optimization
#[derive(Debug, Clone)]
pub struct FitnessContext {
    /// Current generation (0-indexed)
    pub generation: usize,
    /// Total number of generations
    pub total_generations: usize,
    /// Sample rate for this generation (1 = every tick, N = every Nth tick)
    pub sample_rate: usize,
    /// Whether this is full resolution (final generations)
    pub is_full_resolution: bool,
}

impl FitnessContext {
    /// Create a new context for a given generation
    pub fn new(generation: usize, total_generations: usize, config: &AdaptiveSamplingConfig) -> Self {
        let sample_rate = config.sample_rate_for_generation(generation, total_generations);
        let is_full_resolution = sample_rate == 1;
        Self {
            generation,
            total_generations,
            sample_rate,
            is_full_resolution,
        }
    }
    
    /// Create a context that always uses full resolution
    pub fn full_resolution(generation: usize, total_generations: usize) -> Self {
        Self {
            generation,
            total_generations,
            sample_rate: 1,
            is_full_resolution: true,
        }
    }
}

/// Async fitness function type alias - returns fitness AND equity curve for MC analysis
pub type AsyncFitnessFn<C> = Arc<dyn Fn(&C) -> std::pin::Pin<Box<dyn std::future::Future<Output = FitnessResult> + Send>> + Send + Sync>;

/// Async fitness function with generation context for adaptive sampling
pub type AsyncContextFitnessFn<C> = Arc<dyn Fn(&C, FitnessContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = FitnessResult> + Send>> + Send + Sync>;

/// Progress callback type for reporting generation progress
pub type ProgressCallback = Arc<dyn Fn(usize, usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Main genetic algorithm optimizer (async fitness)
pub struct GeneticOptimizer<C: Chromosome> {
    pub config: GeneticConfig,
    pub fitness_fn: AsyncFitnessFn<C>,
    pub progress_callback: Option<ProgressCallback>,
    /// Optional job ID for logging correlation
    pub job_id: Option<String>,
}

impl<C: Chromosome + 'static> GeneticOptimizer<C> {
    pub async fn run(&self) -> (C, f64) {
        let job_tag = self.job_id.as_ref().map(|id| format!("[Job {}] ", id)).unwrap_or_default();
        let optimization_start = std::time::Instant::now();
        log_info!("{}Starting genetic optimization: pop={} gen={}", 
            job_tag, self.config.population_size, self.config.generations);
        
        let mut rng = rand::rngs::StdRng::from_entropy();
        
        // Initialize population
        let mut population: Vec<C> = (0..self.config.population_size)
            .map(|_| C::random(&mut rng))
            .collect();
        
        // Evaluate initial best (this can take several minutes with full data)
        let mut best = population[0].clone();
        log_info!("{}Evaluating initial chromosome (this may take several minutes)...", job_tag);
        let initial_result = (self.fitness_fn)(&best).await;
        log_info!("{}Initial chromosome evaluated: fitness={:.6}", job_tag, initial_result.fitness);
        let mut best_fitness = initial_result.fitness;
        let mut best_equity_curve = initial_result.equity_curve;
        let mut generations_without_improvement = 0;
        const CONVERGENCE_THRESHOLD: usize = 10; // Stop after 10 gens without meaningful improvement
        
        let elite_size = (self.config.population_size as f64 * 0.05).ceil() as usize;
        
        // Dynamic concurrent evaluations based on available memory
        // Uses DynamicConcurrency to determine optimal parallelism:
        // - Checks MAX_CONCURRENT_EVALS env var first
        // - Then POD_MEMORY_LIMIT_GB for Kubernetes
        // - Falls back to 6 for standard 32GB pods
        let concurrency_calc = DynamicConcurrency::default();
        let max_concurrent_evals = concurrency_calc.get_concurrent_evals();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent_evals));
        log_info!("{}Using {} concurrent evaluations (dynamic memory-aware)", job_tag, max_concurrent_evals);

        for generation in 0..self.config.generations {
            let generation_start = std::time::Instant::now();
            
            log_info!("{}Gen {}/{}: Starting PARALLEL fitness evaluation for {} chromosomes (max {} concurrent)", 
                job_tag, generation + 1, self.config.generations, population.len(), max_concurrent_evals);
            
            // PARALLEL FITNESS EVALUATION with bounded concurrency
            // Uses semaphore to limit memory usage while still parallelizing
            let fitness_fn = Arc::clone(&self.fitness_fn);
            let mut handles = Vec::with_capacity(population.len());
            
            for chromo in population.iter() {
                let fitness_fn = Arc::clone(&fitness_fn);
                let chromo = chromo.clone();
                let permit = Arc::clone(&semaphore);
                
                // Spawn each fitness evaluation as a separate tokio task
                // Semaphore limits how many run concurrently
                let handle = tokio::spawn(async move {
                    let _permit = permit.acquire().await.expect("semaphore closed");
                    fitness_fn(&chromo).await
                });
                handles.push(handle);
            }
            
            // Collect all results in parallel
            let mut fitness_results = Vec::with_capacity(population.len());
            let mut completed = 0;
            let batch_start = std::time::Instant::now();
            
            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        fitness_results.push(result);
                        completed += 1;
                        
                        // Log progress every 5 completions
                        if completed % 5 == 0 || completed == population.len() {
                            let elapsed = batch_start.elapsed().as_secs_f64();
                            let rate = completed as f64 / elapsed.max(0.001);
                            log_info!("{}Gen {}: {}/{} chromosomes evaluated, {:.1}s elapsed, {:.2}/s", 
                                job_tag, generation + 1, completed, population.len(), elapsed, rate);
                        }
                    }
                    Err(e) => {
                        log_info!("{}Gen {}: chromosome evaluation panicked: {}", job_tag, generation + 1, e);
                        fitness_results.push(FitnessResult::failure());
                    }
                }
            }
            
            let gen_elapsed = generation_start.elapsed().as_secs_f64();
            log_info!("{}Gen {}/{}: Fitness evaluation complete in {:.1}s ({:.2} chromosomes/sec)", 
                job_tag, generation + 1, self.config.generations, gen_elapsed, population.len() as f64 / gen_elapsed);
            
            // Extract raw fitness values
            let mut fitnesses: Vec<f64> = fitness_results.iter().map(|r| r.fitness).collect();
            // Only apply in later generations to speed up initial exploration
            // This validates stability and prevents overfitting of top performers
            if self.config.use_monte_carlo_fitness && generation >= self.config.generations / 2 {
                self.apply_robustness_penalty(&mut fitnesses, &fitness_results);
            }
            
            // 3. Sort population by fitness for elitism
            let mut indexed_population: Vec<(usize, &C, f64, &FitnessResult)> = population.iter()
                .enumerate()
                .zip(fitnesses.iter())
                .zip(fitness_results.iter())
                .map(|(((idx, c), &fit), result)| (idx, c, fit, result))
                .collect();
            indexed_population.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            
            // Track best fitness BEFORE updating (for convergence check)
            let previous_best_fitness = best_fitness;
            
            // 4. Preserve elite individuals (top 5%)
            let mut new_population = Vec::with_capacity(population.len());
            for i in 0..elite_size {
                new_population.push(indexed_population[i].1.clone());
                if indexed_population[i].2 > best_fitness {
                    best = indexed_population[i].1.clone();
                    best_fitness = indexed_population[i].2;
                    best_equity_curve = indexed_population[i].3.equity_curve.clone();
                }
            }
            
            // Check for improvement (compare against PREVIOUS best, not current)
            const IMPROVEMENT_THRESHOLD: f64 = 0.001; // 0.1% improvement required
            let relative_improvement = if previous_best_fitness > 0.0 {
                (best_fitness - previous_best_fitness) / previous_best_fitness
            } else {
                best_fitness - previous_best_fitness
            };
            
            if relative_improvement > IMPROVEMENT_THRESHOLD {
                generations_without_improvement = 0;
                log_info!("{}Gen {}: Improvement! fitness {:.6} → {:.6} (+{:.2}%)", 
                    job_tag, generation + 1, previous_best_fitness, best_fitness, relative_improvement * 100.0);
            } else {
                generations_without_improvement += 1;
            }
            
            // Calculate population diversity for monitoring
            let diversity = Self::calculate_diversity(&population);
            
            // Early stopping if converged OR if diversity is too low
            if generations_without_improvement >= CONVERGENCE_THRESHOLD {
                log_info!("{}Converged after {} generations (no improvement for {} generations)", 
                    job_tag, generation + 1, CONVERGENCE_THRESHOLD);
                break;
            }
            
            // If diversity is very low, inject random individuals to maintain exploration
            const MIN_DIVERSITY_THRESHOLD: f64 = 0.01;
            let diversity_injection_count = if diversity < MIN_DIVERSITY_THRESHOLD {
                (self.config.population_size as f64 * 0.1) as usize // Inject 10% new random individuals
            } else {
                0
            };
            
            // 5. Selection (tournament) for remaining population
            while new_population.len() < population.len() - diversity_injection_count {
                let (a, b) = (rng.gen_range(0..population.len()), rng.gen_range(0..population.len()));
                let winner = if fitnesses[a] > fitnesses[b] { &population[a] } else { &population[b] };
                new_population.push(winner.clone());
            }
            
            // 5b. Inject random individuals if diversity is too low
            for _ in 0..diversity_injection_count {
                new_population.push(C::random(&mut rng));
            }
            
            // 6. Crossover (skip elites)
            for i in (elite_size..new_population.len()).step_by(2) {
                if rng.gen_bool(self.config.crossover_rate) && i + 1 < new_population.len() {
                    let child = new_population[i].crossover(&new_population[i + 1], &mut rng);
                    new_population[i] = child;
                }
            }
            
            // 7. Adaptive mutation (skip elites)
            // Adaptive step sizing: larger steps early, smaller as we converge
            // Also increase mutation if improvement rate is slow
            let improvement_rate = if generation > 0 && previous_best_fitness > 0.0 {
                (best_fitness - previous_best_fitness).abs() / previous_best_fitness.abs()
            } else {
                0.1 // Default high improvement rate at start
            };
            
            // If improvement is slow, increase mutation to explore more
            let improvement_factor = if improvement_rate < 0.01 { 1.5 } else { 1.0 };
            
            let adaptive_mutation_rate = (self.config.mutation_rate * 
                (1.0 - (generation as f64 / self.config.generations as f64))) * improvement_factor;
            
            for i in elite_size..new_population.len() {
                if rng.gen_bool(adaptive_mutation_rate) {
                    new_population[i].mutate(&mut rng, adaptive_mutation_rate);
                }
            }
            
            population = new_population;
            
            // Progress callback - called AFTER generation work is complete
            // Reports completed generations accurately (generation 1 complete = 1/total progress)
            if let Some(ref callback) = self.progress_callback {
                callback(generation + 1, self.config.generations).await;
            }
            
            // Progress logging every 5 generations for better visibility
            if (generation + 1) % 5 == 0 || generation == 0 {
                log_info!("{}Generation {}/{}: Best fitness = {:.4}, Diversity = {:.4}, Mutation rate = {:.4}", 
                    job_tag, generation + 1, self.config.generations, best_fitness, diversity, adaptive_mutation_rate);
            }
        }
        
        // Run final robustness check on the best chromosome
        if self.config.use_monte_carlo_fitness && !best_equity_curve.is_empty() {
            let (mean_sharpe, std_sharpe) = Self::run_lightweight_monte_carlo(&best_equity_curve, self.config.mc_sample_size);
            log_info!("{}Best chromosome robustness: mean_sharpe={:.3}, std_sharpe={:.3}", job_tag, mean_sharpe, std_sharpe);
        }
        
        let total_time = optimization_start.elapsed().as_secs_f64();
        log_info!("{}Genetic optimization complete in {:.1}s | best_fitness={:.4}", job_tag, total_time, best_fitness);
        
        (best, best_fitness)
    }
    
    /// Apply robustness penalty to top performers using Monte Carlo consistency check
    /// 
    /// This prevents overfitting by penalizing parameter sets that got lucky.
    /// Only applies to top N% to save computation.
    fn apply_robustness_penalty(&self, fitnesses: &mut [f64], results: &[FitnessResult]) {
        // Find fitness threshold for top N%
        let mut sorted = fitnesses.to_vec();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let threshold_idx = ((1.0 - self.config.top_percentile_threshold) * sorted.len() as f64) as usize;
        let fitness_threshold = sorted[threshold_idx.min(sorted.len().saturating_sub(1))];
        
        // Use a smaller sample size for per-generation checks (faster)
        // Final robustness check uses the full mc_sample_size
        let quick_sample_size = 10.min(self.config.mc_sample_size);
        
        // Apply MC consistency check only to top performers
        for (i, result) in results.iter().enumerate() {
            if fitnesses[i] >= fitness_threshold && !result.equity_curve.is_empty() {
                let (mean_sharpe, std_sharpe) = Self::run_lightweight_monte_carlo(
                    &result.equity_curve, 
                    quick_sample_size
                );
                
                // Calculate robustness penalty based on coefficient of variation
                // High std relative to mean = unstable = penalty
                let cv = if mean_sharpe.abs() > 0.001 {
                    std_sharpe / mean_sharpe.abs()
                } else {
                    1.0 // Max penalty if mean is near zero
                };
                
                // Penalty ranges from 0 (perfect consistency) to 0.5 (very unstable)
                // CV of 0.5 or higher = 50% penalty
                let penalty = (cv * 0.5).min(0.5);
                
                // Apply penalty
                fitnesses[i] *= 1.0 - penalty;
            }
        }
    }
    
    /// Run lightweight Monte Carlo by shuffling trade returns
    /// Returns (mean_sharpe, std_sharpe)
    fn run_lightweight_monte_carlo(equity_curve: &[f64], num_runs: usize) -> (f64, f64) {
        use rayon::prelude::*;
        
        if equity_curve.len() < 2 {
            return (0.0, 0.0);
        }
        
        // Extract trade returns from equity curve
        let trade_returns: Vec<f64> = equity_curve.windows(2)
            .map(|w| w[1] - w[0])
            .collect();
        
        if trade_returns.is_empty() {
            return (0.0, 0.0);
        }
        
        // Run Monte Carlo in parallel
        let sharpe_ratios: Vec<f64> = (0..num_runs)
            .into_par_iter()
            .map(|_| {
                let mut rng = rand::thread_rng();
                let mut shuffled = trade_returns.clone();
                shuffled.shuffle(&mut rng);
                
                // Calculate Sharpe ratio
                let mean_ret: f64 = shuffled.iter().sum::<f64>() / shuffled.len() as f64;
                let variance: f64 = shuffled.iter()
                    .map(|r| (r - mean_ret).powi(2))
                    .sum::<f64>() / (shuffled.len() - 1).max(1) as f64;
                let std_ret = variance.sqrt();
                
                if std_ret > 0.0 {
                    mean_ret / std_ret * 252.0_f64.sqrt() // Annualized
                } else {
                    0.0
                }
            })
            .collect();
        
        // Calculate mean and std of Sharpe ratios
        let mean_sharpe: f64 = sharpe_ratios.iter().sum::<f64>() / sharpe_ratios.len() as f64;
        let variance: f64 = sharpe_ratios.iter()
            .map(|s| (s - mean_sharpe).powi(2))
            .sum::<f64>() / (sharpe_ratios.len() - 1).max(1) as f64;
        let std_sharpe = variance.sqrt();
        
        (mean_sharpe, std_sharpe)
    }
    
    /// Calculate population diversity as average pairwise distance
    fn calculate_diversity(population: &[C]) -> f64 {
        if population.len() < 2 {
            return 1.0;
        }
        
        let mut total_distance = 0.0;
        let mut count = 0;
        
        for i in 0..population.len() {
            for j in (i + 1)..population.len() {
                total_distance += population[i].distance(&population[j]);
                count += 1;
            }
        }
        
        if count > 0 {
            total_distance / count as f64
        } else {
            0.0
        }
    }
}

/// Adaptive Genetic Algorithm optimizer with generation-aware sampling
/// 
/// Uses coarser data sampling in early generations for faster exploration,
/// then switches to full resolution for precise fine-tuning. Intended to
/// reduce run time; the effect is not benchmarked in this repo.
/// 
/// Also supports dynamic concurrency based on available memory.
pub struct AdaptiveGeneticOptimizer<C: Chromosome> {
    pub config: GeneticConfig,
    pub fitness_fn: AsyncContextFitnessFn<C>,
    pub progress_callback: Option<ProgressCallback>,
    pub job_id: Option<String>,
    pub sampling_config: AdaptiveSamplingConfig,
}

impl<C: Chromosome + 'static> AdaptiveGeneticOptimizer<C> {
    /// Create a new adaptive optimizer with default sampling config
    pub fn new(
        config: GeneticConfig,
        fitness_fn: AsyncContextFitnessFn<C>,
    ) -> Self {
        Self {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
            sampling_config: AdaptiveSamplingConfig::default(),
        }
    }

    /// Run optimization with adaptive sampling
    pub async fn run(&self) -> (C, f64) {
        let job_tag = self.job_id.as_ref().map(|id| format!("[Job {}] ", id)).unwrap_or_default();
        let optimization_start = std::time::Instant::now();
        
        log_info!("{}Starting ADAPTIVE genetic optimization: pop={} gen={} sampling={:?}", 
            job_tag, self.config.population_size, self.config.generations,
            if self.sampling_config.enabled { "adaptive" } else { "full" });
        
        let mut rng = rand::rngs::StdRng::from_entropy();
        let mut population: Vec<C> = (0..self.config.population_size)
            .map(|_| C::random(&mut rng))
            .collect();
        
        // Evaluate initial best with full resolution
        let mut best = population[0].clone();
        let initial_ctx = FitnessContext::full_resolution(0, self.config.generations);
        log_info!("{}Evaluating initial chromosome (full resolution)...", job_tag);
        let initial_result = (self.fitness_fn)(&best, initial_ctx).await;
        log_info!("{}Initial chromosome evaluated: fitness={:.6}", job_tag, initial_result.fitness);
        
        let mut best_fitness = initial_result.fitness;
        let mut _best_equity_curve = initial_result.equity_curve;
        let mut generations_without_improvement = 0;
        const CONVERGENCE_THRESHOLD: usize = 5;
        
        let elite_size = (self.config.population_size as f64 * 0.05).ceil() as usize;
        
        // Dynamic concurrency
        let concurrency_calc = DynamicConcurrency::default();
        let max_concurrent_evals = concurrency_calc.get_concurrent_evals();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent_evals));
        log_info!("{}Using {} concurrent evaluations", job_tag, max_concurrent_evals);

        for generation in 0..self.config.generations {
            let generation_start = std::time::Instant::now();
            
            // Get sampling context for this generation
            let sample_rate = self.sampling_config.sample_rate_for_generation(
                generation, 
                self.config.generations
            );
            let is_full = sample_rate == 1;
            
            log_info!("{}Gen {}/{}: {} fitness evaluation for {} chromosomes (sample_rate={}, {} concurrent)", 
                job_tag, generation + 1, self.config.generations, 
                if is_full { "FULL" } else { "SAMPLED" },
                population.len(), sample_rate, max_concurrent_evals);
            
            let fitness_fn = Arc::clone(&self.fitness_fn);
            let mut handles = Vec::with_capacity(population.len());
            
            for chromo in population.iter() {
                let fitness_fn = Arc::clone(&fitness_fn);
                let chromo = chromo.clone();
                let permit = Arc::clone(&semaphore);
                let ctx = FitnessContext::new(generation, self.config.generations, &self.sampling_config);
                
                let handle = tokio::spawn(async move {
                    let _permit = permit.acquire().await.expect("semaphore closed");
                    fitness_fn(&chromo, ctx).await
                });
                handles.push(handle);
            }
            
            let mut fitness_results = Vec::with_capacity(population.len());
            let mut completed = 0;
            let batch_start = std::time::Instant::now();
            
            for handle in handles {
                match handle.await {
                    Ok(result) => {
                        fitness_results.push(result);
                        completed += 1;
                        
                        if completed % 5 == 0 || completed == population.len() {
                            let elapsed = batch_start.elapsed().as_secs_f64();
                            let rate = completed as f64 / elapsed.max(0.001);
                            log_info!("{}Gen {}: {}/{} chromosomes evaluated, {:.1}s elapsed, {:.2}/s", 
                                job_tag, generation + 1, completed, population.len(), elapsed, rate);
                        }
                    }
                    Err(e) => {
                        log_info!("{}Gen {}: chromosome evaluation panicked: {}", job_tag, generation + 1, e);
                        fitness_results.push(FitnessResult::failure());
                    }
                }
            }
            
            let gen_elapsed = generation_start.elapsed().as_secs_f64();
            log_info!("{}Gen {}/{}: Fitness evaluation complete in {:.1}s ({:.2} chromosomes/sec)", 
                job_tag, generation + 1, self.config.generations, gen_elapsed, population.len() as f64 / gen_elapsed);
            
            let fitnesses: Vec<f64> = fitness_results.iter().map(|r| r.fitness).collect();
            
            // Sort population by fitness
            let mut indexed_population: Vec<(usize, &C, f64, &FitnessResult)> = population.iter()
                .enumerate()
                .zip(fitnesses.iter())
                .zip(fitness_results.iter())
                .map(|(((idx, c), &fit), result)| (idx, c, fit, result))
                .collect();
            indexed_population.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            
            let previous_best_fitness = best_fitness;
            
            // Preserve elites
            let mut new_population = Vec::with_capacity(population.len());
            for i in 0..elite_size {
                new_population.push(indexed_population[i].1.clone());
                if indexed_population[i].2 > best_fitness {
                    best = indexed_population[i].1.clone();
                    best_fitness = indexed_population[i].2;
                    _best_equity_curve = indexed_population[i].3.equity_curve.clone();
                }
            }
            
            // Check improvement
            const IMPROVEMENT_THRESHOLD: f64 = 0.001;
            let relative_improvement = if previous_best_fitness > 0.0 {
                (best_fitness - previous_best_fitness) / previous_best_fitness
            } else {
                best_fitness - previous_best_fitness
            };
            
            if relative_improvement > IMPROVEMENT_THRESHOLD {
                generations_without_improvement = 0;
                log_info!("{}Gen {}: Improvement! fitness {:.6} → {:.6} (+{:.2}%)", 
                    job_tag, generation + 1, previous_best_fitness, best_fitness, relative_improvement * 100.0);
            } else {
                generations_without_improvement += 1;
            }
            
            // Early stopping
            if generations_without_improvement >= CONVERGENCE_THRESHOLD {
                log_info!("{}Converged at generation {} (no improvement for {} gens)", 
                    job_tag, generation + 1, CONVERGENCE_THRESHOLD);
                break;
            }
            
            // Generate new population (crossover + mutation)
            while new_population.len() < population.len() {
                let parent1_idx = rng.gen_range(0..elite_size.max(1));
                let parent2_idx = rng.gen_range(0..population.len().min(10));
                
                let mut child = indexed_population[parent1_idx].1.clone();
                if rng.gen::<f64>() < self.config.crossover_rate {
                    child = child.crossover(indexed_population[parent2_idx].1, &mut rng);
                }
                child.mutate(&mut rng, self.config.mutation_rate);
                new_population.push(child);
            }
            
            population = new_population;
            
            if let Some(ref callback) = self.progress_callback {
                callback(generation + 1, self.config.generations).await;
            }
        }
        
        let total_elapsed = optimization_start.elapsed().as_secs_f64();
        log_info!("{}Adaptive optimization complete in {:.1}s ({:.1} min): best_fitness={:.6}", 
            job_tag, total_elapsed, total_elapsed / 60.0, best_fitness);
        
        (best, best_fitness)
    }
}

// --- Sample usage for Avellaneda-Stoikov optimization ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chromosome::AvellanedaStoikovChromosome;
    use std::pin::Pin;
    use std::future::Future;

    #[tokio::test]
    async fn test_genetic_optimizer_avellaneda_stoikov() {
        let config = GeneticConfig {
            population_size: 10,
            generations: 3,
            crossover_rate: 0.7,
            mutation_rate: 0.3,
            use_monte_carlo_fitness: false,
            mc_sample_size: 50,
            top_percentile_threshold: 0.10,
        };

        let fitness_fn = Arc::new(|chromo: &AvellanedaStoikovChromosome| {
            // For now, return a simple fitness score
            // In a real implementation, this would run a full backtest
            let score = chromo.risk_aversion * 10.0 + chromo.inventory_target * 5.0;
            let equity_curve = vec![100.0, 101.0, 102.0, 101.5, 103.0]; // Mock equity curve
            Box::pin(async move { FitnessResult::new(score, equity_curve) }) as Pin<Box<dyn Future<Output = FitnessResult> + Send + 'static>>
        });
        
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: Some("test-job".to_string()),
        };
        let (_best, _score) = optimizer.run().await;
        // assert!(_score > 0.0); // Example assertion
    }
}
