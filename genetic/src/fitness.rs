//! Fitness function for Avellaneda-Stoikov strategy chromosome

use crate::chromosome::AvellanedaStoikovChromosome;

/// Fitness function: simplified placeholder that scores based on parameter values
/// In a real implementation, this would run a full backtest simulation
pub async fn avellaneda_stoikov_fitness(
    chromo: &AvellanedaStoikovChromosome,
    _market_data: Vec<dataloader::MarketData>,
    _config: &config::BacktestConfig,
) -> f64 {
    // Simplified fitness calculation based on parameter ranges
    // This is a placeholder - real implementation would run backtest simulation
    
    // Score risk aversion (prefer moderate values)
    let risk_score = if chromo.risk_aversion >= 0.01 && chromo.risk_aversion <= 0.05 {
        10.0
    } else {
        5.0 - (chromo.risk_aversion - 0.03).abs() * 100.0
    };
    
    // Score order size (prefer moderate sizes)
    let size_score = if chromo.order_size >= 0.1 && chromo.order_size <= 0.5 {
        10.0
    } else {
        5.0 - (chromo.order_size - 0.3).abs() * 20.0
    };
    
    // Score inventory target (prefer balanced)
    let inventory_score = 5.0 - chromo.inventory_target.abs() * 10.0;
    
    // Score volatility cap (prefer reasonable caps)
    let vol_score = if chromo.volatility_cap >= 0.5 && chromo.volatility_cap <= 2.0 {
        10.0
    } else {
        5.0 - (chromo.volatility_cap - 1.0).abs() * 5.0
    };
    
    // Combined score
    let fitness = risk_score + size_score + inventory_score + vol_score;
    fitness.max(0.0) // Ensure non-negative
}
