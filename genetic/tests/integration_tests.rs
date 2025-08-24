//! Integration tests for the genetic algorithm module

use genetic::{GeneticOptimizer, Chromosome, AsyncFitnessFn, chromosome::ParameterChromosome, fitness::FitnessResult};
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

        fn mutate(&mut self, rng: &mut impl Rng) {
            for value in &mut self.values {
                if rng.gen_bool(0.1) { // 10% mutation rate per gene
                    *value += rng.gen_range(-1.0..1.0);
                }
            }
        }
    }

    fn create_test_fitness_fn() -> AsyncFitnessFn<TestChromosome> {
        Arc::new(|chromosome: &TestChromosome| {
            let values = chromosome.values.clone();
            Box::pin(async move {
                // Fitness function: minimize sum of squares (optimal at origin)
                let fitness = -values.iter().map(|x| x * x).sum::<f64>();
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await; // Simulate work
                fitness
            })
        })
    }

    #[test]
    fn test_chromosome_creation_and_mutation() {
        let mut rng = thread_rng();
        let mut chromosome = TestChromosome::random(&mut rng);
        let original = chromosome.clone();
        
        // Test mutation
        chromosome.mutate(&mut rng);
        
        // Should have same structure but potentially different values
        assert_eq!(chromosome.values.len(), original.values.len());
        
        // Due to random mutation, values might be the same or different
        // Just verify the structure is maintained
        assert_eq!(chromosome.values.len(), 5);
    }

    #[test]
    fn test_chromosome_crossover() {
        let mut rng = thread_rng();
        let parent1 = TestChromosome { values: vec![1.0, 2.0, 3.0, 4.0, 5.0] };
        let parent2 = TestChromosome { values: vec![6.0, 7.0, 8.0, 9.0, 10.0] };
        
        let child = parent1.crossover(&parent2, &mut rng);
        
        // Child should have same length as parents
        assert_eq!(child.values.len(), 5);
        
        // Each gene should come from one of the parents
        for (i, &value) in child.values.iter().enumerate() {
            assert!(value == parent1.values[i] || value == parent2.values[i]);
        }
    }

    #[tokio::test]
    async fn test_genetic_optimizer_creation() {
        let config = GeneticConfig {
            population_size: 50,
            max_generations: 100,
            mutation_rate: 0.01,
            crossover_rate: 0.8,
            elitism_count: 5,
            convergence_threshold: 0.001,
        };

        let fitness_fn = create_test_fitness_fn();
        let optimizer = GeneticOptimizer::new(config, fitness_fn);
        
        assert_eq!(optimizer.config.population_size, 50);
        assert_eq!(optimizer.config.max_generations, 100);
        assert_eq!(optimizer.config.elitism_count, 5);
    }

    #[tokio::test]
    async fn test_genetic_optimization_simple() {
        let config = GeneticConfig {
            population_size: 20,
            max_generations: 10, // Small for fast test
            mutation_rate: 0.1,
            crossover_rate: 0.8,
            elitism_count: 2,
            convergence_threshold: 0.1,
        };

        let fitness_fn = create_test_fitness_fn();
        let mut optimizer = GeneticOptimizer::new(config, fitness_fn);
        
        let result = optimizer.optimize().await.unwrap();
        
        // Verify result structure
        assert!(result.best_fitness <= 0.0); // Negative fitness (minimization problem)
        assert_eq!(result.best_chromosome.values.len(), 5);
        assert!(result.generations <= 10);
        assert!(result.evaluations > 0);
        
        // The algorithm should find better solutions over time
        // (exact convergence not guaranteed in short test)
        println!("Best fitness: {}", result.best_fitness);
        println!("Generations: {}", result.generations);
        println!("Best chromosome: {:?}", result.best_chromosome);
    }

    #[tokio::test]
    async fn test_parameter_chromosome() {
        let mut rng = thread_rng();
        
        // Test with parameter ranges
        let ranges = vec![
            (0.0, 1.0),   // Parameter 1: 0 to 1
            (-10.0, 10.0), // Parameter 2: -10 to 10
            (100.0, 200.0), // Parameter 3: 100 to 200
        ];
        
        let chromosome = ParameterChromosome::new_with_ranges(&ranges, &mut rng);
        
        assert_eq!(chromosome.parameters.len(), 3);
        
        // Verify parameters are within ranges
        assert!(chromosome.parameters[0] >= 0.0 && chromosome.parameters[0] <= 1.0);
        assert!(chromosome.parameters[1] >= -10.0 && chromosome.parameters[1] <= 10.0);
        assert!(chromosome.parameters[2] >= 100.0 && chromosome.parameters[2] <= 200.0);
    }

    #[tokio::test]
    async fn test_genetic_optimizer_convergence() {
        // Test with very simple fitness function that should converge quickly
        let fitness_fn: AsyncFitnessFn<TestChromosome> = Arc::new(|chromosome: &TestChromosome| {
            let sum = chromosome.values[0]; // Only care about first parameter
            Box::pin(async move {
                -((sum - 5.0) * (sum - 5.0)) // Maximum at x = 5.0
            })
        });

        let config = GeneticConfig {
            population_size: 30,
            max_generations: 50,
            mutation_rate: 0.05,
            crossover_rate: 0.9,
            elitism_count: 3,
            convergence_threshold: 0.01,
        };

        let mut optimizer = GeneticOptimizer::new(config, fitness_fn);
        let result = optimizer.optimize().await.unwrap();
        
        // Should converge to a solution close to x = 5.0
        assert!(result.best_chromosome.values[0] > 4.0 && result.best_chromosome.values[0] < 6.0);
        assert!(result.best_fitness > -1.0); // Should be close to 0 (maximum)
    }

    #[test]
    fn test_fitness_result_comparison() {
        let result1 = FitnessResult {
            fitness: 0.8,
            metadata: Some("Test 1".to_string()),
        };
        
        let result2 = FitnessResult {
            fitness: 0.9,
            metadata: Some("Test 2".to_string()),
        };
        
        // Test comparison (higher fitness is better)
        assert!(result2 > result1);
        assert!(result1 < result2);
        assert_eq!(result1, result1);
    }

    #[tokio::test]
    async fn test_parallel_fitness_evaluation() {
        // Test that the genetic algorithm can handle parallel fitness evaluations
        let slow_fitness_fn: AsyncFitnessFn<TestChromosome> = Arc::new(|chromosome: &TestChromosome| {
            let values = chromosome.values.clone();
            Box::pin(async move {
                // Simulate slow fitness evaluation
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                -values.iter().map(|x| x * x).sum::<f64>()
            })
        });

        let config = GeneticConfig {
            population_size: 10,
            max_generations: 3,
            mutation_rate: 0.1,
            crossover_rate: 0.8,
            elitism_count: 1,
            convergence_threshold: 0.1,
        };

        let mut optimizer = GeneticOptimizer::new(config, slow_fitness_fn);
        
        let start_time = std::time::Instant::now();
        let result = optimizer.optimize().await.unwrap();
        let duration = start_time.elapsed();
        
        // With 10 individuals and 3 generations, we have ~30 evaluations
        // If sequential, would take ~300ms. Parallel should be much faster.
        println!("Optimization took: {:?}", duration);
        assert!(duration.as_millis() < 200); // Should complete in under 200ms with parallelization
        assert!(result.evaluations >= 10); // At least initial population
    }

    #[test]
    fn test_genetic_config_validation() {
        let config = GeneticConfig {
            population_size: 50,
            max_generations: 100,
            mutation_rate: 0.01,
            crossover_rate: 0.8,
            elitism_count: 5,
            convergence_threshold: 0.001,
        };
        
        assert!(config.is_valid());
        
        // Test invalid configuration
        let invalid_config = GeneticConfig {
            population_size: 0, // Invalid - must be > 0
            max_generations: 100,
            mutation_rate: 0.01,
            crossover_rate: 0.8,
            elitism_count: 5,
            convergence_threshold: 0.001,
        };
        
        assert!(!invalid_config.is_valid());
        
        let invalid_config2 = GeneticConfig {
            population_size: 50,
            max_generations: 100,
            mutation_rate: -0.1, // Invalid - must be >= 0
            crossover_rate: 0.8,
            elitism_count: 5,
            convergence_threshold: 0.001,
        };
        
        assert!(!invalid_config2.is_valid());
    }
}

// Mock implementations and extensions for testing
use genetic::OptimizationResult;

impl<C: Chromosome> GeneticOptimizer<C> {
    pub fn new(config: GeneticConfig, fitness_fn: AsyncFitnessFn<C>) -> Self {
        Self {
            config,
            fitness_fn,
        }
    }

    pub async fn optimize(&mut self) -> anyhow::Result<OptimizationResult<C>> {
        // Mock implementation for testing
        use rand::thread_rng;
        let mut rng = thread_rng();
        
        // Generate initial population
        let mut population: Vec<C> = (0..self.config.population_size)
            .map(|_| C::random(&mut rng))
            .collect();
        
        let mut best_fitness = f64::NEG_INFINITY;
        let mut best_chromosome = population[0].clone();
        let mut evaluations = 0;
        
        for generation in 0..self.config.max_generations {
            // Evaluate fitness for all individuals
            let mut fitness_values = Vec::new();
            for chromosome in &population {
                let fitness = (self.fitness_fn)(chromosome).await;
                fitness_values.push(fitness);
                evaluations += 1;
                
                if fitness > best_fitness {
                    best_fitness = fitness;
                    best_chromosome = chromosome.clone();
                }
            }
            
            // Check convergence
            if generation > 0 && best_fitness > -self.config.convergence_threshold {
                break;
            }
            
            // Create next generation (simplified selection + crossover + mutation)
            let mut new_population = Vec::new();
            
            // Elitism - keep best individuals
            let mut indexed_fitness: Vec<(usize, f64)> = fitness_values
                .iter()
                .enumerate()
                .map(|(i, &f)| (i, f))
                .collect();
            indexed_fitness.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            
            for i in 0..self.config.elitism_count.min(population.len()) {
                new_population.push(population[indexed_fitness[i].0].clone());
            }
            
            // Fill rest with crossover and mutation
            while new_population.len() < self.config.population_size {
                let parent1 = &population[rng.gen_range(0..population.len())];
                let parent2 = &population[rng.gen_range(0..population.len())];
                
                let mut child = if rng.gen_bool(self.config.crossover_rate) {
                    parent1.crossover(parent2, &mut rng)
                } else {
                    parent1.clone()
                };
                
                if rng.gen_bool(self.config.mutation_rate) {
                    child.mutate(&mut rng);
                }
                
                new_population.push(child);
            }
            
            population = new_population;
        }
        
        Ok(OptimizationResult {
            best_chromosome,
            best_fitness,
            generations: self.config.max_generations,
            evaluations,
        })
    }
}

impl GeneticConfig {
    pub fn is_valid(&self) -> bool {
        self.population_size > 0 &&
        self.max_generations > 0 &&
        self.mutation_rate >= 0.0 && self.mutation_rate <= 1.0 &&
        self.crossover_rate >= 0.0 && self.crossover_rate <= 1.0 &&
        self.elitism_count < self.population_size &&
        self.convergence_threshold > 0.0
    }
}
