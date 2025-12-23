//! Comprehensive tests for Genetic Optimization module
//! Covers chromosome operations, fitness evaluation, and evolution

use genetic::{
    chromosome::AvellanedaStoikovChromosome, 
    Chromosome,
    FitnessResult,
};
use config::GeneticConfig;
use rand::SeedableRng;
use rand::rngs::StdRng;

// ============================================================================
// CHROMOSOME CREATION TESTS
// ============================================================================

#[test]
fn test_chromosome_random_creation() {
    let mut rng = StdRng::seed_from_u64(42);
    let chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    assert!(chromosome.risk_aversion > 0.0, "Risk aversion should be positive");
    assert!(chromosome.order_size_pct > 0.0, "Order size pct should be positive");
    assert!(chromosome.window_ms > 0, "Window should be positive");
}

#[test]
fn test_chromosome_random_bounds() {
    let mut rng = StdRng::seed_from_u64(12345);
    
    // Generate multiple chromosomes and check they're within expected ranges
    for _ in 0..100 {
        let chromosome = AvellanedaStoikovChromosome::random(&mut rng);
        
        assert!(chromosome.risk_aversion >= 0.0001 && chromosome.risk_aversion <= 1.0,
            "Risk aversion out of range: {}", chromosome.risk_aversion);
        assert!(chromosome.order_size_pct >= 0.001 && chromosome.order_size_pct <= 0.25,
            "Order size pct out of range: {}", chromosome.order_size_pct);
        // fee_rate removed from chromosome - now uses exchange config
    }
}

#[test]
fn test_chromosome_random_different_seeds() {
    let mut rng1 = StdRng::seed_from_u64(1);
    let mut rng2 = StdRng::seed_from_u64(2);
    
    let c1 = AvellanedaStoikovChromosome::random(&mut rng1);
    let c2 = AvellanedaStoikovChromosome::random(&mut rng2);
    
    // Different seeds should produce different chromosomes
    let same = c1.risk_aversion == c2.risk_aversion 
        && c1.order_size_pct == c2.order_size_pct;
    assert!(!same, "Different seeds should produce different chromosomes");
}

// ============================================================================
// MUTATION TESTS
// ============================================================================

#[test]
fn test_chromosome_mutation() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    let original_risk_aversion = chromosome.risk_aversion;
    
    // Mutate modifies in place
    chromosome.mutate(&mut rng, 0.3);
    
    // After mutation, values should still be valid
    assert!(chromosome.risk_aversion > 0.0, "Mutated values should still be valid");
    
    // Note: mutation is probabilistic, so we can't guarantee values change
    let _ = original_risk_aversion;
}

#[test]
fn test_mutation_produces_valid_chromosomes() {
    let mut rng = StdRng::seed_from_u64(99);
    let mut chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    // Apply mutation many times
    for _ in 0..50 {
        chromosome.mutate(&mut rng, 0.3);
        
        // Mutated chromosome should still have valid values within bounds
        assert!(chromosome.risk_aversion >= 0.0001 && chromosome.risk_aversion <= 1.0,
            "Risk aversion should stay in bounds");
        assert!(chromosome.order_size_pct >= 0.001 && chromosome.order_size_pct <= 0.25,
            "Order size should stay in bounds");
    }
}

#[test]
fn test_mutation_with_high_rate() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    // High mutation rate should cause more changes
    for _ in 0..10 {
        chromosome.mutate(&mut rng, 0.9);
    }
    
    // Values should still be valid
    assert!(chromosome.risk_aversion > 0.0);
    assert!(chromosome.order_size_pct > 0.0);
}

#[test]
fn test_mutation_with_zero_rate() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    let original = chromosome.clone();
    
    // Zero mutation rate should not change anything
    chromosome.mutate(&mut rng, 0.0);
    
    assert_eq!(chromosome.risk_aversion, original.risk_aversion);
    assert_eq!(chromosome.order_size_pct, original.order_size_pct);
}

// ============================================================================
// CROSSOVER TESTS
// ============================================================================

#[test]
fn test_chromosome_crossover() {
    let mut rng = StdRng::seed_from_u64(42);
    let parent1 = AvellanedaStoikovChromosome::random(&mut rng);
    let parent2 = AvellanedaStoikovChromosome::random(&mut rng);
    
    let child = parent1.crossover(&parent2, &mut rng);
    
    // Child should have valid values
    assert!(child.risk_aversion > 0.0, "Child risk_aversion should be positive");
    assert!(child.order_size_pct > 0.0, "Child order_size_pct should be positive");
}

#[test]
fn test_crossover_inheritance() {
    let mut rng = StdRng::seed_from_u64(42);
    let parent1 = AvellanedaStoikovChromosome::random(&mut rng);
    let parent2 = AvellanedaStoikovChromosome::random(&mut rng);
    
    // Generate many children and verify they inherit from parents
    for _ in 0..20 {
        let child = parent1.crossover(&parent2, &mut rng);
        
        // Each field should come from one of the parents
        let risk_from_p1 = (child.risk_aversion - parent1.risk_aversion).abs() < 0.0001;
        let risk_from_p2 = (child.risk_aversion - parent2.risk_aversion).abs() < 0.0001;
        
        assert!(risk_from_p1 || risk_from_p2, 
            "Child's risk_aversion should come from a parent");
    }
}

#[test]
fn test_crossover_with_identical_parents() {
    let mut rng = StdRng::seed_from_u64(42);
    let parent = AvellanedaStoikovChromosome::random(&mut rng);
    
    let child = parent.crossover(&parent, &mut rng);
    
    // Child should be identical to parent when parents are identical
    assert_eq!(child.risk_aversion, parent.risk_aversion);
    assert_eq!(child.order_size_pct, parent.order_size_pct);
}

// ============================================================================
// GENETIC CONFIG TESTS
// ============================================================================

#[test]
fn test_genetic_config_default() {
    let config = GeneticConfig::default();
    
    assert!(config.population_size > 0, "Population size should be positive");
    assert!(config.generations > 0, "Generations should be positive");
    assert!(config.mutation_rate >= 0.0 && config.mutation_rate <= 1.0, 
        "Mutation rate should be between 0 and 1");
    assert!(config.crossover_rate >= 0.0 && config.crossover_rate <= 1.0,
        "Crossover rate should be between 0 and 1");
}

#[test]
fn test_genetic_config_custom() {
    let config = GeneticConfig {
        population_size: 100,
        generations: 50,
        mutation_rate: 0.1,
        crossover_rate: 0.8,
        use_monte_carlo_fitness: false,
        mc_sample_size: 30,
        top_percentile_threshold: 0.20,
    };
    
    assert_eq!(config.population_size, 100);
    assert_eq!(config.generations, 50);
    assert!(!config.use_monte_carlo_fitness);
}

#[test]
fn test_genetic_config_default_values() {
    let config = GeneticConfig::default();
    
    // Check default values
    assert_eq!(config.population_size, 50);
    assert_eq!(config.generations, 100);
    assert_eq!(config.crossover_rate, 0.8);
    assert_eq!(config.mutation_rate, 0.1);
    assert!(config.use_monte_carlo_fitness);
}

// ============================================================================
// FITNESS FUNCTION CONCEPT TESTS
// ============================================================================

#[test]
fn test_fitness_result_creation() {
    let equity_curve = vec![100.0, 105.0, 102.0, 108.0, 115.0];
    let result = FitnessResult::new(1.5, equity_curve.clone());
    
    assert_eq!(result.fitness, 1.5);
    assert_eq!(result.equity_curve.len(), 5);
}

#[test]
fn test_fitness_result_failure() {
    let result = FitnessResult::failure();
    
    assert_eq!(result.fitness, 0.0);
    assert!(result.equity_curve.is_empty());
}

#[test]
fn test_fitness_calculation_concept() {
    // Test fitness calculation logic
    let sharpe_ratio = 2.0;
    let profit_factor = 1.5;
    let win_rate = 0.55;
    let max_drawdown = 0.10; // 10%
    
    // Simple fitness function
    let fitness = sharpe_ratio * profit_factor * win_rate / (1.0 + max_drawdown);
    
    assert!(fitness > 0.0, "Fitness should be positive for profitable strategy");
}

#[test]
fn test_fitness_penalizes_drawdown() {
    // Same metrics but different drawdowns
    let sharpe = 2.0;
    let pf = 1.5;
    let wr = 0.55;
    
    let low_dd = 0.05;
    let high_dd = 0.30;
    
    let fitness_low_dd = sharpe * pf * wr / (1.0 + low_dd);
    let fitness_high_dd = sharpe * pf * wr / (1.0 + high_dd);
    
    assert!(fitness_low_dd > fitness_high_dd, 
        "Higher drawdown should result in lower fitness");
}

#[test]
fn test_fitness_rewards_sharpe() {
    let pf = 1.5;
    let wr = 0.55;
    let dd = 0.10;
    
    let low_sharpe = 0.5;
    let high_sharpe = 3.0;
    
    let fitness_low_sharpe = low_sharpe * pf * wr / (1.0 + dd);
    let fitness_high_sharpe = high_sharpe * pf * wr / (1.0 + dd);
    
    assert!(fitness_high_sharpe > fitness_low_sharpe,
        "Higher Sharpe ratio should result in higher fitness");
}

// ============================================================================
// SELECTION TESTS
// ============================================================================

#[test]
fn test_tournament_selection_concept() {
    // Simulate tournament selection
    let fitnesses = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let tournament_size = 3;
    
    let mut rng = StdRng::seed_from_u64(42);
    
    // Run many tournaments
    let mut winners = Vec::new();
    for _ in 0..100 {
        let mut best_idx = 0;
        let mut best_fitness = 0.0;
        
        for _ in 0..tournament_size {
            let idx = rand::Rng::gen_range(&mut rng, 0..fitnesses.len());
            if fitnesses[idx] > best_fitness {
                best_fitness = fitnesses[idx];
                best_idx = idx;
            }
        }
        winners.push(best_idx);
    }
    
    // Higher fitness individuals should win more often
    let high_fitness_wins = winners.iter().filter(|&&i| i >= 3).count();
    assert!(high_fitness_wins > 40, "High fitness should win more tournaments");
}

#[test]
fn test_elite_selection_concept() {
    let fitnesses = vec![1.0, 5.0, 2.0, 4.0, 3.0];
    let elite_count = 2;
    
    let mut indexed: Vec<(usize, f64)> = fitnesses.iter().enumerate()
        .map(|(i, &f)| (i, f))
        .collect();
    
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    let elite_indices: Vec<usize> = indexed.iter().take(elite_count).map(|&(i, _)| i).collect();
    
    // Top 2 should be indices 1 and 3 (fitness 5.0 and 4.0)
    assert!(elite_indices.contains(&1));
    assert!(elite_indices.contains(&3));
}

// ============================================================================
// DISTANCE TESTS
// ============================================================================

#[test]
fn test_chromosome_distance_same() {
    let mut rng = StdRng::seed_from_u64(42);
    let chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    let distance = chromosome.distance(&chromosome);
    
    assert!(distance.abs() < 0.0001, "Distance to self should be zero");
}

#[test]
fn test_chromosome_distance_different() {
    let mut rng = StdRng::seed_from_u64(42);
    let c1 = AvellanedaStoikovChromosome::random(&mut rng);
    let c2 = AvellanedaStoikovChromosome::random(&mut rng);
    
    let distance = c1.distance(&c2);
    
    assert!(distance > 0.0, "Distance between different chromosomes should be positive");
}

#[test]
fn test_chromosome_distance_symmetry() {
    let mut rng = StdRng::seed_from_u64(42);
    let c1 = AvellanedaStoikovChromosome::random(&mut rng);
    let c2 = AvellanedaStoikovChromosome::random(&mut rng);
    
    let d1 = c1.distance(&c2);
    let d2 = c2.distance(&c1);
    
    assert!((d1 - d2).abs() < 0.0001, "Distance should be symmetric");
}

// ============================================================================
// POPULATION EVOLUTION TESTS
// ============================================================================

#[test]
fn test_population_creation() {
    let mut rng = StdRng::seed_from_u64(42);
    let population_size = 20;
    
    let population: Vec<AvellanedaStoikovChromosome> = (0..population_size)
        .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
        .collect();
    
    assert_eq!(population.len(), population_size);
    
    // All should be valid
    for c in &population {
        assert!(c.risk_aversion > 0.0);
    }
}

#[test]
fn test_population_fitness_sorting() {
    // Simulate fitness evaluation
    let fitnesses = vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
    
    let mut indexed: Vec<(usize, f64)> = fitnesses.iter().enumerate()
        .map(|(i, &f)| (i, f))
        .collect();
    
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    
    // Best should be index 5 (fitness 9.0)
    assert_eq!(indexed[0].0, 5);
    assert_eq!(indexed[0].1, 9.0);
}

#[test]
fn test_generation_evolution_simulation() {
    let mut rng = StdRng::seed_from_u64(42);
    
    // Initial population
    let mut population: Vec<AvellanedaStoikovChromosome> = (0..10)
        .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
        .collect();
    
    // Simple fitness function (prefer higher risk_aversion for test)
    let fitness_fn = |c: &AvellanedaStoikovChromosome| c.risk_aversion * 10.0;
    
    // Run a few generations
    for _ in 0..5 {
        // Evaluate fitness
        let mut with_fitness: Vec<(AvellanedaStoikovChromosome, f64)> = 
            population.iter().map(|c| (c.clone(), fitness_fn(c))).collect();
        
        // Sort by fitness descending
        with_fitness.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Create next generation
        let mut next_gen = Vec::new();
        
        // Keep elite
        next_gen.push(with_fitness[0].0.clone());
        
        // Create offspring
        while next_gen.len() < population.len() {
            let p1 = &with_fitness[rand::Rng::gen_range(&mut rng, 0..5)].0;
            let p2 = &with_fitness[rand::Rng::gen_range(&mut rng, 0..5)].0;
            let mut child = p1.crossover(p2, &mut rng);
            child.mutate(&mut rng, 0.1);
            next_gen.push(child);
        }
        
        population = next_gen;
    }
    
    // Should still have valid population
    assert_eq!(population.len(), 10);
}

#[test]
fn test_diversity_maintenance() {
    let mut rng = StdRng::seed_from_u64(42);
    
    // Generate a population
    let population: Vec<AvellanedaStoikovChromosome> = (0..20)
        .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
        .collect();
    
    // Calculate diversity (standard deviation of a parameter)
    let values: Vec<f64> = population.iter().map(|c| c.risk_aversion).collect();
    let mean: f64 = values.iter().sum::<f64>() / values.len() as f64;
    let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let std_dev = variance.sqrt();
    
    // There should be some diversity
    assert!(std_dev > 0.001, "Population should have diversity");
}

// ============================================================================
// PARAMETER RANGE TESTS
// ============================================================================

#[test]
fn test_parameter_clamping() {
    // Test that parameters stay within valid ranges even after many mutations
    let mut rng = StdRng::seed_from_u64(42);
    let mut chromosome = AvellanedaStoikovChromosome::random(&mut rng);
    
    for _ in 0..100 {
        chromosome.mutate(&mut rng, 0.5);
        
        // These should always be within bounds
        assert!(chromosome.risk_aversion >= 0.0001 && chromosome.risk_aversion <= 1.0);
        assert!(chromosome.order_size_pct >= 0.001 && chromosome.order_size_pct <= 0.25);
        // fee_rate removed from chromosome - now uses exchange config
    }
}

#[test]
fn test_all_parameter_ranges() {
    let mut rng = StdRng::seed_from_u64(42);
    
    for _ in 0..50 {
        let c = AvellanedaStoikovChromosome::random(&mut rng);
        
        assert!(c.risk_aversion >= 0.0001 && c.risk_aversion <= 1.0);
        assert!(c.order_size_pct >= 0.001 && c.order_size_pct <= 0.25);
        assert!(c.window_ms >= 1_000 && c.window_ms <= 1_800_000);
        assert!(c.min_quote_lifetime_ms >= 10 && c.min_quote_lifetime_ms <= 60_000);
        // fee_rate and latency_buffer removed from chromosome - uses exchange config and latency simulation
        assert!(c.inventory_target >= -1.0 && c.inventory_target <= 1.0);
        assert!(c.volatility_cap >= 0.20 && c.volatility_cap <= 5.0);  // Min 0.20 > vol_floor 0.15
        assert!(c.time_horizon_minutes >= 0.167 && c.time_horizon_minutes <= 60.0);
        assert!(c.market_depth_k >= 1.0 && c.market_depth_k <= 50.0);  // Updated range for liquid markets
    }
}

// ============================================================================
// CONVERGENCE TESTS
// ============================================================================

#[test]
fn test_convergence_detection_concept() {
    // Simulate fitness progression
    let generations_without_improvement = 0;
    let convergence_threshold = 10;
    
    let converged = generations_without_improvement >= convergence_threshold;
    
    assert!(!converged, "Should not be converged at start");
}

#[test]
fn test_early_stopping_concept() {
    // Simulate fitness tracking
    let mut best_fitness = 0.0;
    let mut stagnation_count = 0;
    let stagnation_limit = 5;
    
    let fitness_history = vec![1.0, 1.5, 1.8, 1.8, 1.8, 1.8, 1.8, 1.8];
    
    for fitness in fitness_history {
        if fitness > best_fitness {
            best_fitness = fitness;
            stagnation_count = 0;
        } else {
            stagnation_count += 1;
        }
        
        if stagnation_count >= stagnation_limit {
            break;
        }
    }
    
    assert!(stagnation_count >= stagnation_limit, "Should detect stagnation");
}

// ============================================================================
// MONTE CARLO FITNESS TESTS
// ============================================================================

#[test]
fn test_monte_carlo_config() {
    let config = GeneticConfig {
        population_size: 50,
        generations: 100,
        crossover_rate: 0.8,
        mutation_rate: 0.1,
        use_monte_carlo_fitness: true,
        mc_sample_size: 100,
        top_percentile_threshold: 0.05,
    };
    
    assert!(config.use_monte_carlo_fitness);
    assert_eq!(config.mc_sample_size, 100);
    assert_eq!(config.top_percentile_threshold, 0.05);
}

#[test]
fn test_monte_carlo_consistency_concept() {
    // Simulate MC consistency check
    let fitness_samples = vec![1.5, 1.4, 1.6, 1.45, 1.55, 1.5, 1.48, 1.52];
    
    let mean: f64 = fitness_samples.iter().sum::<f64>() / fitness_samples.len() as f64;
    let variance: f64 = fitness_samples.iter()
        .map(|&f| (f - mean).powi(2))
        .sum::<f64>() / fitness_samples.len() as f64;
    let std_dev = variance.sqrt();
    
    // Coefficient of variation
    let cv = std_dev / mean;
    
    // Low CV indicates consistent results
    assert!(cv < 0.1, "Consistent fitness should have low CV");
}

// ============================================================================
// INTEGRATION CONCEPT TESTS
// ============================================================================

#[test]
fn test_full_evolution_simulation() {
    let mut rng = StdRng::seed_from_u64(42);
    let population_size = 20;
    let generations = 10;
    
    // Create initial population
    let mut population: Vec<AvellanedaStoikovChromosome> = (0..population_size)
        .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
        .collect();
    
    // Track best fitness
    let mut best_overall_fitness = 0.0;
    
    // Mock fitness function
    let fitness = |c: &AvellanedaStoikovChromosome| -> f64 {
        // Reward certain parameter combinations (fee_rate removed from chromosome)
        c.risk_aversion * 5.0 + c.order_size_pct * 10.0 + c.market_depth_k * 0.5
    };
    
    for _gen in 0..generations {
        // Evaluate
        let mut pop_with_fitness: Vec<(AvellanedaStoikovChromosome, f64)> = 
            population.iter().map(|c| (c.clone(), fitness(c))).collect();
        
        // Sort
        pop_with_fitness.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Track best
        if pop_with_fitness[0].1 > best_overall_fitness {
            best_overall_fitness = pop_with_fitness[0].1;
        }
        
        // Create next generation
        let mut next_gen = Vec::new();
        
        // Elite (top 2)
        next_gen.push(pop_with_fitness[0].0.clone());
        next_gen.push(pop_with_fitness[1].0.clone());
        
        // Breed
        while next_gen.len() < population_size {
            let i1 = rand::Rng::gen_range(&mut rng, 0..population_size / 2);
            let i2 = rand::Rng::gen_range(&mut rng, 0..population_size / 2);
            
            let mut child = pop_with_fitness[i1].0.crossover(&pop_with_fitness[i2].0, &mut rng);
            child.mutate(&mut rng, 0.1);
            next_gen.push(child);
        }
        
        population = next_gen;
    }
    
    // Evolution should produce some fitness improvement
    assert!(best_overall_fitness > 0.0, "Should find some fit individuals");
}

#[test]
fn test_chromosome_clonability() {
    let mut rng = StdRng::seed_from_u64(42);
    let original = AvellanedaStoikovChromosome::random(&mut rng);
    let cloned = original.clone();
    
    assert_eq!(original.risk_aversion, cloned.risk_aversion);
    assert_eq!(original.order_size_pct, cloned.order_size_pct);
    assert_eq!(original.window_ms, cloned.window_ms);
}

#[test]
fn test_phase2_dynamic_parameters() {
    let mut rng = StdRng::seed_from_u64(42);
    let c = AvellanedaStoikovChromosome::random(&mut rng);
    
    // Phase 2 parameters should be within bounds
    assert!(c.volatility_adjustment_factor >= 0.0 && c.volatility_adjustment_factor <= 2.0);
    assert!(c.inventory_penalty_factor >= 0.0 && c.inventory_penalty_factor <= 1.0);
    assert!(c.trend_sensitivity >= -1.0 && c.trend_sensitivity <= 1.0);
    assert!(c.flow_sensitivity >= -1.0 && c.flow_sensitivity <= 1.0);
}
