//! Trading strategy implementations
//!
//! This module contains concrete implementations of trading strategies that implement
//! the Strategy trait. Each strategy file contains one or more related strategy types.

// Common strategy implementations
pub mod avellaneda_stoikov;

// Re-export all strategy implementations for convenience
pub use avellaneda_stoikov::*;

/// Registry of all available strategy types for dynamic creation
pub fn get_available_strategies() -> Vec<&'static str> {
    vec![
        "MovingAverageCrossover",
        "RSIMeanReversion", 
        "BollingerBandBreakout",
        "BollingerBandMeanReversion",
        "MACDCrossover",
        "MultiIndicatorStrategy",
        "AvellanedaStoikovMarketMaker",
    ]
}

/// Create a strategy by name with default parameters
/// This is useful for dynamic strategy creation
pub fn create_strategy(strategy_name: &str) -> Option<Box<dyn crate::Strategy>> {
    match strategy_name {
        "AvellanedaStoikovMarketMaker" => Some(Box::new(AvellanedaStoikovStrategy::default())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_strategies() {
        let strategies = get_available_strategies();
        assert!(!strategies.is_empty());
        assert!(strategies.contains(&"MovingAverageCrossover"));
    }

    #[test]
    fn test_create_strategy() {
        let strategy = create_strategy("MovingAverageCrossover");
        assert!(strategy.is_some());
        
        let invalid_strategy = create_strategy("NonExistentStrategy");
        assert!(invalid_strategy.is_none());
    }
}
