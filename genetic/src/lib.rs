/// Genetic Algorithm optimization for trading strategies
/// 
/// Provides async genetic algorithm optimization for finding optimal
/// trading strategy parameters.

use std::sync::Arc;
use config::GeneticConfig;
use rand::prelude::*;

pub mod chromosome;
pub mod fitness;

/// Represents a set of strategy parameters (chromosome)
pub trait Chromosome: Clone + Send + Sync {
    /// Randomly initialize a chromosome
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self;
    /// Crossover with another chromosome
    fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self;
    /// Mutate the chromosome
    fn mutate(&mut self, rng: &mut impl Rng);
}

/// Async fitness function type alias
pub type AsyncFitnessFn<C> = Arc<dyn Fn(&C) -> std::pin::Pin<Box<dyn std::future::Future<Output = f64> + Send>> + Send + Sync>;

/// Main genetic algorithm optimizer (async fitness)
pub struct GeneticOptimizer<C: Chromosome> {
    pub config: GeneticConfig,
    pub fitness_fn: AsyncFitnessFn<C>,
}

impl<C: Chromosome> GeneticOptimizer<C> {
    pub async fn run(&self) -> (C, f64) {
        let mut rng = rand::thread_rng();
        // 1. Initialize population
        let mut population: Vec<C> = (0..self.config.population_size)
            .map(|_| C::random(&mut rng))
            .collect();
        let mut best = population[0].clone();
        let mut best_fitness = (self.fitness_fn)(&best).await;

        for _ in 0..self.config.generations {
            // 2. Evaluate fitness (async)
            let mut fitnesses = Vec::with_capacity(population.len());
            for c in &population {
                fitnesses.push((self.fitness_fn)(c).await);
            }
            // 3. Selection (tournament)
            let mut new_population = Vec::with_capacity(population.len());
            for _ in 0..population.len() {
                let (a, b) = (rng.gen_range(0..population.len()), rng.gen_range(0..population.len()));
                let winner = if fitnesses[a] > fitnesses[b] { &population[a] } else { &population[b] };
                new_population.push(winner.clone());
            }
            // 4. Crossover
            for i in (0..new_population.len()).step_by(2) {
                if rng.gen_bool(self.config.crossover_rate) && i + 1 < new_population.len() {
                    let child = new_population[i].crossover(&new_population[i + 1], &mut rng);
                    new_population[i] = child;
                }
            }
            // 5. Mutation
            for c in &mut new_population {
                if rng.gen_bool(self.config.mutation_rate) {
                    c.mutate(&mut rng);
                }
            }
            // 6. Update best
            for (c, &fit) in new_population.iter().zip(&fitnesses) {
                if fit > best_fitness {
                    best = c.clone();
                    best_fitness = fit;
                }
            }
            population = new_population;
        }
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
        };
        let (_best, _score) = optimizer.run().await;
        // assert!(_score > 0.0); // Example assertion
    }
}
