//! Benchmarks for the genetic algorithm module

use genetic::{GeneticOptimizer, Chromosome, AsyncFitnessFn};
use config::GeneticConfig;
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rand::prelude::*;
use std::sync::Arc;
use tokio::runtime::Runtime;

// Test chromosome for benchmarking
#[derive(Clone, Debug)]
struct BenchChromosome {
    values: Vec<f64>,
}

impl Chromosome for BenchChromosome {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        Self {
            values: (0..10).map(|_| rng.gen_range(-10.0..10.0)).collect(),
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
            if rng.gen_bool(0.1) {
                *value += rng.gen_range(-1.0..1.0);
            }
        }
    }
}

// Fast fitness function for benchmarking
fn create_fast_fitness_fn() -> AsyncFitnessFn<BenchChromosome> {
    Arc::new(|chromosome: &BenchChromosome| {
        let values = chromosome.values.clone();
        Box::pin(async move {
            -values.iter().map(|x| x * x).sum::<f64>()
        })
    })
}

// Slow fitness function for benchmarking I/O bound scenarios
fn create_slow_fitness_fn() -> AsyncFitnessFn<BenchChromosome> {
    Arc::new(|chromosome: &BenchChromosome| {
        let values = chromosome.values.clone();
        Box::pin(async move {
            tokio::time::sleep(tokio::time::Duration::from_micros(100)).await;
            -values.iter().map(|x| x * x).sum::<f64>()
        })
    })
}

fn bench_chromosome_operations(c: &mut Criterion) {
    let mut rng = thread_rng();
    
    c.bench_function("chromosome_creation", |b| {
        b.iter(|| {
            black_box(BenchChromosome::random(&mut rng))
        })
    });
    
    let chromosome1 = BenchChromosome::random(&mut rng);
    let chromosome2 = BenchChromosome::random(&mut rng);
    
    c.bench_function("chromosome_crossover", |b| {
        b.iter(|| {
            black_box(chromosome1.crossover(&chromosome2, &mut rng))
        })
    });
    
    let mut chromosome = BenchChromosome::random(&mut rng);
    c.bench_function("chromosome_mutation", |b| {
        b.iter(|| {
            chromosome.mutate(&mut rng);
            black_box(&chromosome)
        })
    });
}

fn bench_genetic_optimizer_creation(c: &mut Criterion) {
    let fitness_fn = create_fast_fitness_fn();
    
    c.bench_function("optimizer_creation", |b| {
        b.iter(|| {
            let config = GeneticConfig {
                population_size: 50,
                max_generations: 100,
                mutation_rate: 0.01,
                crossover_rate: 0.8,
                elitism_count: 5,
                convergence_threshold: 0.001,
            };
            black_box(GeneticOptimizer::new(config, fitness_fn.clone()))
        })
    });
}

fn bench_fitness_evaluation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("fitness_evaluation");
    
    let fast_fitness = create_fast_fitness_fn();
    let slow_fitness = create_slow_fitness_fn();
    let mut rng = thread_rng();
    let chromosome = BenchChromosome::random(&mut rng);
    
    group.bench_function("fast_fitness", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box((fast_fitness)(&chromosome).await)
            })
        })
    });
    
    group.bench_function("slow_fitness", |b| {
        b.iter(|| {
            rt.block_on(async {
                black_box((slow_fitness)(&chromosome).await)
            })
        })
    });
    
    group.finish();
}

fn bench_population_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("population_operations");
    let mut rng = thread_rng();
    
    for size in [10, 50, 100, 200].iter() {
        group.bench_with_input(BenchmarkId::new("population_generation", size), size, |b, &size| {
            b.iter(|| {
                let population: Vec<BenchChromosome> = (0..size)
                    .map(|_| BenchChromosome::random(&mut rng))
                    .collect();
                black_box(population)
            })
        });
    }
    
    // Benchmark crossover operations on different population sizes
    for size in [10, 50, 100].iter() {
        let population: Vec<BenchChromosome> = (0..*size)
            .map(|_| BenchChromosome::random(&mut rng))
            .collect();
            
        group.bench_with_input(BenchmarkId::new("population_crossover", size), &population, |b, pop| {
            b.iter(|| {
                let mut new_population = Vec::new();
                for _ in 0..pop.len() {
                    let parent1 = &pop[rng.gen_range(0..pop.len())];
                    let parent2 = &pop[rng.gen_range(0..pop.len())];
                    let child = parent1.crossover(parent2, &mut rng);
                    new_population.push(child);
                }
                black_box(new_population)
            })
        });
    }
    
    group.finish();
}

fn bench_genetic_optimization_full(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("genetic_optimization_full");
    group.sample_size(10); // Reduce sample size for long-running benchmarks
    
    let fast_fitness = create_fast_fitness_fn();
    
    // Benchmark with different population sizes
    for pop_size in [20, 50].iter() {
        let config = GeneticConfig {
            population_size: *pop_size,
            max_generations: 10, // Keep generations low for benchmarking
            mutation_rate: 0.01,
            crossover_rate: 0.8,
            elitism_count: 2,
            convergence_threshold: 0.001,
        };
        
        group.bench_with_input(BenchmarkId::new("full_optimization", pop_size), &config, |b, config| {
            b.iter(|| {
                rt.block_on(async {
                    let mut optimizer = GeneticOptimizer::new(config.clone(), fast_fitness.clone());
                    black_box(optimizer.optimize().await.unwrap())
                })
            })
        });
    }
    
    group.finish();
}

fn bench_parameter_optimization_scenarios(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("parameter_optimization_scenarios");
    group.sample_size(10);
    
    // Benchmark different chromosome sizes (parameter counts)
    for param_count in [5, 10, 20].iter() {
        // Create chromosome with specific parameter count
        #[derive(Clone, Debug)]
        struct VariableChromosome {
            values: Vec<f64>,
        }
        
        impl Chromosome for VariableChromosome {
            fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
                Self {
                    values: (0..*param_count).map(|_| rng.gen_range(-10.0..10.0)).collect(),
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
                    if rng.gen_bool(0.1) {
                        *value += rng.gen_range(-1.0..1.0);
                    }
                }
            }
        }
        
        let fitness_fn: AsyncFitnessFn<VariableChromosome> = Arc::new(|chromosome: &VariableChromosome| {
            let values = chromosome.values.clone();
            Box::pin(async move {
                -values.iter().map(|x| x * x).sum::<f64>()
            })
        });
        
        let config = GeneticConfig {
            population_size: 30,
            max_generations: 5,
            mutation_rate: 0.01,
            crossover_rate: 0.8,
            elitism_count: 2,
            convergence_threshold: 0.001,
        };
        
        group.bench_with_input(BenchmarkId::new("param_optimization", param_count), param_count, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let mut optimizer = GeneticOptimizer::new(config.clone(), fitness_fn.clone());
                    black_box(optimizer.optimize().await.unwrap())
                })
            })
        });
    }
    
    group.finish();
}

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    let mut rng = thread_rng();
    
    // Benchmark memory allocation patterns
    group.bench_function("large_population_allocation", |b| {
        b.iter(|| {
            let population: Vec<BenchChromosome> = (0..1000)
                .map(|_| BenchChromosome::random(&mut rng))
                .collect();
            black_box(population)
        })
    });
    
    // Benchmark cloning operations
    let population: Vec<BenchChromosome> = (0..100)
        .map(|_| BenchChromosome::random(&mut rng))
        .collect();
    
    group.bench_function("population_cloning", |b| {
        b.iter(|| {
            black_box(population.clone())
        })
    });
    
    group.finish();
}

criterion_group!(
    genetic_benches,
    bench_chromosome_operations,
    bench_genetic_optimizer_creation,
    bench_fitness_evaluation,
    bench_population_operations,
    bench_genetic_optimization_full,
    bench_parameter_optimization_scenarios,
    bench_memory_usage
);

criterion_main!(genetic_benches);
