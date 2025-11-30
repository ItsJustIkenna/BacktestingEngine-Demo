/// Genetic Algorithm optimization for trading strategies
/// 
/// Provides async genetic algorithm optimization for finding optimal
/// trading strategy parameters with parallel fitness evaluation.

use std::sync::Arc;
use config::GeneticConfig;
use rand::prelude::*;
use rayon::prelude::*;

pub mod chromosome;
// Fitness functions moved to backtest crate to avoid circular dependency
// pub mod fitness;

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

/// Async fitness function type alias
pub type AsyncFitnessFn<C> = Arc<dyn Fn(&C) -> std::pin::Pin<Box<dyn std::future::Future<Output = f64> + Send>> + Send + Sync>;

/// Progress callback type for reporting generation progress
pub type ProgressCallback = Arc<dyn Fn(usize, usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Main genetic algorithm optimizer (async fitness)
pub struct GeneticOptimizer<C: Chromosome> {
    pub config: GeneticConfig,
    pub fitness_fn: AsyncFitnessFn<C>,
    pub progress_callback: Option<ProgressCallback>,
}

impl<C: Chromosome> GeneticOptimizer<C> {
    pub async fn run(&self) -> (C, f64) {
        // Use StdRng which is Send-safe (unlike thread_rng which uses Rc)
        let mut rng = rand::rngs::StdRng::from_entropy();
        
        // 1. Initialize population
        let mut population: Vec<C> = (0..self.config.population_size)
            .map(|_| C::random(&mut rng))
            .collect();
        
        let mut best = population[0].clone();
        let mut best_fitness = (self.fitness_fn)(&best).await;
        let mut generations_without_improvement = 0;
        const CONVERGENCE_THRESHOLD: usize = 10;
        
        // Elite size (top 5% of population)
        let elite_size = (self.config.population_size as f64 * 0.05).ceil() as usize;

        // Capture Tokio runtime handle for use in parallel threads
        let handle = tokio::runtime::Handle::current();

        for generation in 0..self.config.generations {
            // Call progress callback if provided
            if let Some(ref callback) = self.progress_callback {
                callback(generation + 1, self.config.generations).await;
            }
            
            // 2. Evaluate fitness in parallel (OPTIMIZED)
            // Use Tokio runtime handle to execute async fitness functions in Rayon threads
            let fitnesses: Vec<f64> = population.par_iter()
                .map(|chromo| {
                    // Use Tokio runtime handle for async in Rayon parallel context
                    handle.block_on((self.fitness_fn)(chromo))
                })
                .collect();
            
            // Track best fitness in this generation
            let mut generation_best_fitness = best_fitness;
            
            // 3. Sort population by fitness for elitism
            let mut indexed_population: Vec<(usize, &C, f64)> = population.iter()
                .enumerate()
                .zip(fitnesses.iter())
                .map(|((idx, c), &fit)| (idx, c, fit))
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
                eprintln!("✅ Converged after {} generations (no improvement for {} generations)", 
                    generation + 1, CONVERGENCE_THRESHOLD);
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
                eprintln!("🧬 Generation {}/{}: Best fitness = {:.4}, Diversity = {:.4}, Mutation rate = {:.4}", 
                    generation + 1, self.config.generations, best_fitness, diversity, adaptive_mutation_rate);
            }
        }
        
        (best, best_fitness)
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
        };

        let fitness_fn = Arc::new(|chromo: &AvellanedaStoikovChromosome| {
            // For now, return a simple fitness score
            // In a real implementation, this would run a full backtest
            let score = chromo.risk_aversion * 10.0 + chromo.inventory_target * 5.0;
            Box::pin(async move { score }) as Pin<Box<dyn Future<Output = f64> + Send + 'static>>
        });
        
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
        };
        let (_best, _score) = optimizer.run().await;
        // assert!(_score > 0.0); // Example assertion
    }
}
