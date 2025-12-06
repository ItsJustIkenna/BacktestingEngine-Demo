/// Genetic Algorithm optimization for trading strategies
/// 
/// Provides async genetic algorithm optimization for finding optimal
/// trading strategy parameters with parallel fitness evaluation.
/// 
/// Includes robustness-weighted fitness using Monte Carlo consistency checks
/// on top performers to prevent overfitting to lucky parameter combinations.

use std::sync::Arc;
use config::GeneticConfig;
use futures::stream::{FuturesUnordered, StreamExt};
use rand::prelude::*;
use ultra_logger::UltraLogger;
use once_cell::sync::Lazy;

pub mod chromosome;
// Fitness functions moved to backtest crate to avoid circular dependency
// pub mod fitness;

/// Logger for genetic algorithm operations
static GENETIC_LOGGER: Lazy<Arc<UltraLogger>> = Lazy::new(|| {
    Arc::new(UltraLogger::new("BacktestingEngine-genetic".to_string()))
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
#[derive(Clone)]
pub struct FitnessResult {
    pub fitness: f64,
    pub equity_curve: Vec<f64>,
}

impl FitnessResult {
    pub fn new(fitness: f64, equity_curve: Vec<f64>) -> Self {
        Self { fitness, equity_curve }
    }
    
    pub fn failure() -> Self {
        Self { fitness: 0.0, equity_curve: Vec::new() }
    }
}

/// Async fitness function type alias - returns fitness AND equity curve for MC analysis
pub type AsyncFitnessFn<C> = Arc<dyn Fn(&C) -> std::pin::Pin<Box<dyn std::future::Future<Output = FitnessResult> + Send>> + Send + Sync>;

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
        
        // Evaluate initial best
        let mut best = population[0].clone();
        let initial_result = (self.fitness_fn)(&best).await;
        let mut best_fitness = initial_result.fitness;
        let mut best_equity_curve = initial_result.equity_curve;
        let mut generations_without_improvement = 0;
        const CONVERGENCE_THRESHOLD: usize = 10;
        
        let elite_size = (self.config.population_size as f64 * 0.05).ceil() as usize;

        for generation in 0..self.config.generations {
            let generation_start = std::time::Instant::now();
            
            // Progress callback
            if let Some(ref callback) = self.progress_callback {
                callback(generation + 1, self.config.generations).await;
            }
            
            log_info!("{}Gen {}/{}: Starting fitness evaluation for {} chromosomes", 
                job_tag, generation + 1, self.config.generations, population.len());
            
            // Evaluate fitness with controlled parallelism (prevent OOM)
            // Process in batches of 4 to limit memory usage while still parallelizing
            let mut futures = FuturesUnordered::new();
            let mut fitness_results = Vec::with_capacity(population.len());
            let batch_start = std::time::Instant::now();

            for (chunk_idx, chunk) in population.chunks(4).enumerate() {
                // Queue up fitness evaluations for this batch
                for chromo in chunk {
                    futures.push((self.fitness_fn)(chromo));
                }
                // Collect results from this batch before starting next
                while let Some(result) = futures.next().await {
                    fitness_results.push(result);
                }
                
                // Log every 3 batches with timing
                if chunk_idx % 3 == 0 {
                    let elapsed = batch_start.elapsed().as_secs_f64();
                    let rate = fitness_results.len() as f64 / elapsed.max(0.001);
                    log_info!("{}Gen {}: batch {} done, {}/{} chromosomes, {:.1}s elapsed, {:.1}/s", 
                        job_tag, generation + 1, chunk_idx + 1, fitness_results.len(), population.len(), elapsed, rate);
                }
            }
            
            let gen_elapsed = generation_start.elapsed().as_secs_f64();
            log_info!("{}Gen {}/{}: Fitness evaluation complete in {:.1}s", 
                job_tag, generation + 1, self.config.generations, gen_elapsed);
            
            // Extract raw fitness values
            let mut fitnesses: Vec<f64> = fitness_results.iter().map(|r| r.fitness).collect();
            
            // ROBUSTNESS-WEIGHTED FITNESS: Apply Monte Carlo consistency check
            // Only apply in later generations to speed up initial exploration
            // This validates stability and prevents overfitting of top performers
            if self.config.use_monte_carlo_fitness && generation >= self.config.generations / 2 {
                self.apply_robustness_penalty(&mut fitnesses, &fitness_results);
            }
            
            // Track best fitness in this generation
            let mut generation_best_fitness = best_fitness;
            
            // 3. Sort population by fitness for elitism
            let mut indexed_population: Vec<(usize, &C, f64, &FitnessResult)> = population.iter()
                .enumerate()
                .zip(fitnesses.iter())
                .zip(fitness_results.iter())
                .map(|(((idx, c), &fit), result)| (idx, c, fit, result))
                .collect();
            indexed_population.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            
            // 4. Preserve elite individuals (top 5%)
            let mut new_population = Vec::with_capacity(population.len());
            for i in 0..elite_size {
                new_population.push(indexed_population[i].1.clone());
                if indexed_population[i].2 > generation_best_fitness {
                    generation_best_fitness = indexed_population[i].2;
                    best = indexed_population[i].1.clone();
                    best_fitness = generation_best_fitness;
                    best_equity_curve = indexed_population[i].3.equity_curve.clone();
                }
            }
            
            // Check for improvement
            if generation_best_fitness > best_fitness {
                generations_without_improvement = 0;
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
            let improvement_rate = if generation > 0 && best_fitness > 0.0 {
                (best_fitness - generation_best_fitness).abs() / best_fitness.abs()
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
