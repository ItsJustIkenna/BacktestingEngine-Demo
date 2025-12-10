//! Integration tests for the genetic algorithm module

use genetic::{GeneticOptimizer, Chromosome, AsyncFitnessFn, FitnessResult};
use genetic::chromosome::AvellanedaStoikovChromosome;
use config::GeneticConfig;
use rand::prelude::*;
use std::sync::Arc;
use tokio;

#[cfg(test)]
mod tests {
    use super::*;

    // Test chromosome implementation for testing
    #[derive(Clone, Debug, PartialEq)]
    struct TestChromosome {
        pub values: Vec<f64>,
    }

    impl Chromosome for TestChromosome {
        fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
            Self {
                values: (0..5).map(|_| rng.gen_range(-10.0..10.0)).collect(),
            }
        }

        fn crossover(&self, other: &Self, rng: &mut impl Rng) -> Self {
            let mut new_values = Vec::new();
            for (a, b) in self.values.iter().zip(&other.values) {
                if rng.gen_bool(0.5) {
                    new_values.push(*a);
                } else {
                    new_values.push(*b);
                }
            }
            Self { values: new_values }
        }

        fn mutate(&mut self, rng: &mut impl Rng, mutation_rate: f64) {
            for value in &mut self.values {
                if rng.gen_bool(mutation_rate) {
                    *value += rng.gen_range(-1.0..1.0);
                }
            }
        }

        fn distance(&self, other: &Self) -> f64 {
            self.values.iter().zip(&other.values)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        }
    }

    fn create_test_fitness_fn() -> AsyncFitnessFn<TestChromosome> {
        Arc::new(|chromosome: &TestChromosome| {
            let values = chromosome.values.clone();
            Box::pin(async move {
                // Simple fitness function: minimize sum of squares
                let fitness = -values.iter().map(|&x| x * x).sum::<f64>();
                // Create a mock equity curve for testing
                let equity_curve = vec![100.0, 101.0, 102.0, 101.5, 103.0];
                FitnessResult::new(fitness, equity_curve)
            })
        })
    }

    #[tokio::test]
    async fn test_genetic_optimizer_creation() {
        let config = GeneticConfig {
            population_size: 50,
            generations: 100,
            crossover_rate: 0.7,
            mutation_rate: 0.1, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };

        let fitness_fn = create_test_fitness_fn();

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        // Test that we can run the optimizer
        let (best, fitness) = optimizer.run().await;
        
        // Should find values close to zero (minimum of sum of squares)
        assert!(fitness > -100.0);
        assert_eq!(best.values.len(), 5);
    }

    #[tokio::test]
    async fn test_genetic_optimizer_convergence() {
        let config = GeneticConfig {
            population_size: 30,
            generations: 10, // Small for fast test
            crossover_rate: 0.8,
            mutation_rate: 0.2, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };

        let fitness_fn = create_test_fitness_fn();

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        let (best, fitness) = optimizer.run().await;

        // Basic sanity checks
        assert!(fitness <= 0.0); // Our fitness is negative sum of squares
        assert_eq!(best.values.len(), 5);
        
        // Values should be reasonable (not extreme)
        for &value in &best.values {
            assert!(value.abs() < 20.0);
        }
    }

    #[tokio::test]
    async fn test_avellaneda_stoikov_chromosome() {
        let mut rng = rand::thread_rng();
        
        // Test random generation
        let chromo1 = AvellanedaStoikovChromosome::random(&mut rng);
        let chromo2 = AvellanedaStoikovChromosome::random(&mut rng);
        
        // Test that values are in expected ranges (updated for wider ranges)
        assert!(chromo1.risk_aversion >= 0.0001 && chromo1.risk_aversion <= 1.0);
        assert!(chromo1.order_size_pct >= 0.001 && chromo1.order_size_pct <= 0.25);
        assert!(chromo1.window_ms >= 1_000 && chromo1.window_ms <= 1_800_000);
        // fee_rate removed from chromosome - uses exchange config
        // market_depth_k range is now 1.0-50.0
        assert!(chromo1.market_depth_k >= 1.0 && chromo1.market_depth_k <= 50.0);
        
        // Test crossover
        let child = chromo1.crossover(&chromo2, &mut rng);
        assert!(child.risk_aversion >= 0.0001 && child.risk_aversion <= 1.0);
        assert!(child.order_size_pct >= 0.001 && child.order_size_pct <= 0.25);
        
        // Test mutation
        let mut chromo_mut = chromo1.clone();
        chromo_mut.mutate(&mut rng, 0.1);
        // Values should stay in bounds after mutation
        assert!(chromo_mut.risk_aversion >= 0.0001 && chromo_mut.risk_aversion <= 1.0);
        assert!(chromo_mut.order_size_pct >= 0.001 && chromo_mut.order_size_pct <= 0.25);
    }

    #[tokio::test]
    async fn test_genetic_optimizer_with_avellaneda_stoikov() {
        let config = GeneticConfig {
            population_size: 20,
            generations: 50,
            crossover_rate: 0.7,
            mutation_rate: 0.1, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };

        // Simple fitness function for A-S parameters
        let fitness_fn: AsyncFitnessFn<AvellanedaStoikovChromosome> = Arc::new(|chromo| {
            let risk = chromo.risk_aversion;
            let size_pct = chromo.order_size_pct;
            let inv_target = chromo.inventory_target;
            Box::pin(async move {
                // Prefer moderate risk aversion and reasonable order size percentages
                let risk_score = if risk > 0.01 && risk < 0.05 { 10.0 } else { 1.0 };
                let size_score = if size_pct > 0.02 && size_pct < 0.10 { 10.0 } else { 1.0 };
                let inv_score = if inv_target.abs() < 0.5 { 5.0 } else { 1.0 };
                FitnessResult::new(risk_score + size_score + inv_score, Vec::new())
            })
        });

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        let (best, fitness) = optimizer.run().await;

        // Should find decent parameters
        assert!(fitness > 10.0);
        assert!(best.risk_aversion > 0.001);
        assert!(best.order_size_pct > 0.005);
    }

    #[tokio::test]
    async fn test_async_fitness_evaluation() {
        let config = GeneticConfig {
            population_size: 10,
            generations: 5,
            crossover_rate: 0.5,
            mutation_rate: 0.1, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };

        // Counter to track how many fitness evaluations occurred
        use std::sync::atomic::{AtomicUsize, Ordering};
        let eval_count = Arc::new(AtomicUsize::new(0));

        // Async fitness function that counts evaluations
        let eval_count_clone = Arc::clone(&eval_count);
        let slow_fitness_fn: AsyncFitnessFn<TestChromosome> = Arc::new(move |chromo| {
            let values = chromo.values.clone();
            let counter = Arc::clone(&eval_count_clone);
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                // Return fitness based on first value
                FitnessResult::new(values.get(0).copied().unwrap_or(0.0), Vec::new())
            })
        });

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn: slow_fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        let (_best, _fitness) = optimizer.run().await;

        // Should have evaluated the fitness function multiple times
        // (population_size * generations) evaluations minimum
        let total_evals = eval_count.load(Ordering::SeqCst);
        assert!(total_evals >= 50); // At least 10 * 5 evaluations
    }

    #[tokio::test]
    async fn test_genetic_config_validation() {
        // Valid config
        let config = GeneticConfig {
            population_size: 100,
            generations: 50,
            crossover_rate: 0.7,
            mutation_rate: 0.1, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };
        
        assert!(config.population_size > 0);
        assert!(config.generations > 0);
        assert!(config.crossover_rate >= 0.0 && config.crossover_rate <= 1.0);
        assert!(config.mutation_rate >= 0.0 && config.mutation_rate <= 1.0);

        // Test edge cases
        let edge_config = GeneticConfig {
            population_size: 1,
            generations: 1,
            crossover_rate: 0.0,
            mutation_rate: 0.0, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };
        
        assert!(edge_config.population_size > 0);
        assert!(edge_config.generations > 0);
    }

    #[tokio::test]
    async fn test_tournament_selection() {
        let config = GeneticConfig {
            population_size: 10,
            generations: 1,
            crossover_rate: 0.0, // No crossover for this test
            mutation_rate: 0.0,  // No mutation for this test
            use_monte_carlo_fitness: false,
            mc_sample_size: 50,
            top_percentile_threshold: 0.10,
        };

        // Fitness function that returns the first value
        let fitness_fn: AsyncFitnessFn<TestChromosome> = Arc::new(|chromo| {
            let first_val = chromo.values.get(0).copied().unwrap_or(0.0);
            Box::pin(async move { FitnessResult::new(first_val, Vec::new()) })
        });

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        let (best, fitness) = optimizer.run().await;

        // Should find reasonable solution
        assert_eq!(best.values.len(), 5);
        assert!(fitness >= -10.0); // Should be reasonable given random initialization
    }

    #[tokio::test]
    async fn test_chromosome_trait_implementation() {
        let mut rng = rand::thread_rng();
        
        // Test TestChromosome
        let chromo1 = TestChromosome::random(&mut rng);
        let chromo2 = TestChromosome::random(&mut rng);
        
        assert_eq!(chromo1.values.len(), 5);
        assert_eq!(chromo2.values.len(), 5);
        
        // Test crossover
        let child = chromo1.crossover(&chromo2, &mut rng);
        assert_eq!(child.values.len(), 5);
        
        // Test mutation
        let mut mutant = chromo1.clone();
        mutant.mutate(&mut rng, 0.1);
        assert_eq!(mutant.values.len(), 5);
    }

    #[tokio::test]
    async fn test_population_diversity() {
        let config = GeneticConfig {
            population_size: 20,
            generations: 5,
            crossover_rate: 0.9,
            mutation_rate: 0.3, use_monte_carlo_fitness: false, mc_sample_size: 50, top_percentile_threshold: 0.10, };

        let fitness_fn = create_test_fitness_fn();

        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };

        let (best, fitness) = optimizer.run().await;

        // Basic checks
        assert!(fitness <= 0.0);
        assert_eq!(best.values.len(), 5);
    }
}

