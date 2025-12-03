use serde::{Deserialize, Serialize};

/// Configuration for CLI visualization display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationConfig {
    /// Terminal display width
    pub terminal_width: usize,
    /// Terminal display height  
    pub terminal_height: usize,
    /// Default initial capital assumption when not provided
    pub default_initial_capital: f64,
    /// Chart scaling parameters
    pub chart: ChartConfig,
    /// Performance rating thresholds
    pub thresholds: ThresholdConfig,
}

/// Chart visualization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartConfig {
    /// Maximum value for chart scaling
    pub max_value: f64,
    /// Chart width in characters
    pub width: usize,
}

/// Performance rating thresholds for intelligent analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Sharpe ratio thresholds for star ratings
    pub sharpe_ratio: SharpeThresholds,
    /// Volatility thresholds for risk classification
    pub volatility: VolatilityThresholds,
    /// Profit factor thresholds for performance rating
    pub profit_factor: ProfitFactorThresholds,
    /// Win rate thresholds for classification
    pub win_rate: WinRateThresholds,
    /// Other financial thresholds
    pub financial: FinancialThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharpeThresholds {
    pub excellent: f64,     // 5 stars
    pub very_good: f64,     // 4 stars  
    pub good: f64,          // 3 stars
    pub fair: f64,          // 2 stars
    // Below fair = 1 star
    pub display_green: f64, // Threshold for green vs yellow display
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilityThresholds {
    pub high_risk: f64,     // Above this = high risk (red)
    pub medium_risk: f64,   // Above this = medium risk (yellow)
    // Below medium = low risk (green)
}

#[derive(Debug, Clone, Serialize, Deserialize)]  
pub struct ProfitFactorThresholds {
    pub excellent: f64,     // Above this = excellent
    pub good: f64,          // Above this = good
    pub marginal: f64,      // Above this = marginal
    // Below marginal = poor
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WinRateThresholds {
    pub strong: f64,        // Above this = strong
    pub average: f64,       // Above this = average
    // Below average = weak
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialThresholds {
    pub significant_drawdown: f64,      // Drawdown amount for warning
    pub minimum_return_positive: f64,   // Minimum return to be considered positive
    pub recommendation_profit_check: f64, // Min profit for positive recommendation
}

/// Genetic Algorithm configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneticConfig {
    pub population_size: usize,
    pub generations: usize,
    pub mutation_rate: f64,
    pub crossover_rate: f64,
    pub elite_ratio: f64,
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub visualization: VisualizationConfig,
    pub genetic_algorithm: GeneticConfig,
}

impl Default for VisualizationConfig {
    fn default() -> Self {
        Self {
            terminal_width: 80,
            terminal_height: 24,
            default_initial_capital: 100000.0,
            chart: ChartConfig::default(),
            thresholds: ThresholdConfig::default(),
        }
    }
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            max_value: 20000.0,
            width: 50,
        }
    }
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            sharpe_ratio: SharpeThresholds {
                excellent: 2.0,
                very_good: 1.5,
                good: 1.0,
                fair: 0.5,
                display_green: 1.0,
            },
            volatility: VolatilityThresholds {
                high_risk: 25.0,
                medium_risk: 15.0,
            },
            profit_factor: ProfitFactorThresholds {
                excellent: 2.0,
                good: 1.5,
                marginal: 1.0,
            },
            win_rate: WinRateThresholds {
                strong: 60.0,
                average: 45.0,
            },
            financial: FinancialThresholds {
                significant_drawdown: 10000.0,
                minimum_return_positive: 1.0,
                recommendation_profit_check: 0.0,
            },
        }
    }
}

impl Default for GeneticConfig {
    fn default() -> Self {
        Self {
            population_size: 25,        // Reduced from 50 to prevent OOMKill with large datasets
            generations: 50,            // Reduced from 100 for memory efficiency
            mutation_rate: 0.1,
            crossover_rate: 0.8,
            elite_ratio: 0.2,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            visualization: VisualizationConfig::default(),
            genetic_algorithm: GeneticConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load configuration from file, falling back to defaults if file doesn't exist
    pub fn load_or_default() -> Self {
        // Try to load from config file, fall back to defaults
        if let Ok(config_str) = std::fs::read_to_string("config/app-config.yaml") {
            if let Ok(config) = serde_yaml::from_str::<AppConfig>(&config_str) {
                return config;
            }
        }
        
        // Fall back to defaults and optionally save them
        let default_config = Self::default();
        
        // Create config directory if it doesn't exist
        if let Err(_) = std::fs::create_dir_all("config") {
            // If we can't create the directory, just use defaults
            return default_config;
        }
        
        // Try to save default config
        if let Ok(yaml) = serde_yaml::to_string(&default_config) {
            let _ = std::fs::write("config/app-config.yaml", yaml);
        }
        
        default_config
    }
}


