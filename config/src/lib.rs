use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

/// Configuration validation errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing required field: {field}")]
    MissingField { field: String },
    
    #[error("Invalid value for {field}: {value} (reason: {reason})")]
    InvalidValue {
        field: String,
        value: String,
        reason: String,
    },
    
    #[error("File not found: {path}")]
    FileNotFound { path: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Main backtesting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub database: DatabaseConfig,
    pub trading: TradingConfig,
    pub data: DataConfig,
    pub output: OutputConfig,
}

/// Database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: Option<u32>,
    pub connection_timeout: Option<u64>, // seconds
}

/// Trading-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub commission_rate: f64,      // e.g., 0.001 for 0.1%
    pub slippage_bps: f64,         // basis points
    pub initial_capital: f64,
    pub max_position_size: Option<f64>, // as percentage of portfolio
    pub risk_free_rate: f64,       // for Sharpe ratio calculation
}

/// Data source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    pub default_timeframe: String, // e.g., "1d", "1h", "5m"
    pub data_sources: Vec<String>, // e.g., ["yahoo", "alpha_vantage"]
    pub cache_enabled: bool,
    pub cache_duration_hours: u32,
}

/// Output and reporting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub results_dir: String,
    pub reports_dir: String,
    pub charts_enabled: bool,
    pub export_formats: Vec<String>, // e.g., ["json", "csv", "html"]
}

/// Monte Carlo simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub num_simulations: usize,
    pub confidence_level: f64,
    // Add more config options as needed
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            trading: TradingConfig::default(),
            data: DataConfig::default(),
            output: OutputConfig::default(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "trading_platform".to_string(),
            username: "postgres".to_string(),
            password: "password".to_string(),
            max_connections: Some(10),
            connection_timeout: Some(30),
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            commission_rate: 0.001,    // 0.1%
            slippage_bps: 5.0,         // 5 basis points
            initial_capital: 100000.0, // $100,000
            max_position_size: Some(0.1), // 10%
            risk_free_rate: 0.02,      // 2%
        }
    }
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            default_timeframe: "1d".to_string(),
            data_sources: vec!["yahoo".to_string()],
            cache_enabled: true,
            cache_duration_hours: 24,
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            results_dir: "./results".to_string(),
            reports_dir: "./reports".to_string(),
            charts_enabled: true,
            export_formats: vec!["json".to_string(), "csv".to_string()],
        }
    }
}

impl BacktestConfig {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        
        let config: BacktestConfig = serde_yaml::from_str(&contents)?;
        
        config.validate()?;
        Ok(config)
    }
    
    /// Save configuration to a YAML file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let contents = serde_yaml::to_string(self)?;
        
        fs::write(path, contents)?;
        
        Ok(())
    }
    
    /// Create a default configuration file if it doesn't exist
    pub fn create_default_if_missing<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        
        if path.exists() {
            Self::from_file(path)
        } else {
            let default_config = Self::default();
            default_config.to_file(path)?;
            Ok(default_config)
        }
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate database config
        if self.database.host.is_empty() {
            return Err(ConfigError::MissingField {
                field: "database.host".to_string(),
            });
        }
        
        if self.database.port == 0 {
            return Err(ConfigError::InvalidValue {
                field: "database.port".to_string(),
                value: "0".to_string(),
                reason: "Port must be greater than 0".to_string(),
            });
        }
        
        // Validate trading config
        if self.trading.commission_rate < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "trading.commission_rate".to_string(),
                value: self.trading.commission_rate.to_string(),
                reason: "Commission rate cannot be negative".to_string(),
            });
        }
        
        if self.trading.initial_capital <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "trading.initial_capital".to_string(),
                value: self.trading.initial_capital.to_string(),
                reason: "Initial capital must be positive".to_string(),
            });
        }
        
        if let Some(max_pos) = self.trading.max_position_size {
            if max_pos <= 0.0 || max_pos > 1.0 {
                return Err(ConfigError::InvalidValue {
                    field: "trading.max_position_size".to_string(),
                    value: max_pos.to_string(),
                    reason: "Max position size must be between 0 and 1".to_string(),
                });
            }
        }
        
        // Validate data config
        if self.data.default_timeframe.is_empty() {
            return Err(ConfigError::MissingField {
                field: "data.default_timeframe".to_string(),
            });
        }
        
        if self.data.data_sources.is_empty() {
            return Err(ConfigError::MissingField {
                field: "data.data_sources".to_string(),
            });
        }
        
        // Validate output config
        if self.output.results_dir.is_empty() {
            return Err(ConfigError::MissingField {
                field: "output.results_dir".to_string(),
            });
        }
        
        if self.output.export_formats.is_empty() {
            return Err(ConfigError::MissingField {
                field: "output.export_formats".to_string(),
            });
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_default_config_creation() {
        let config = BacktestConfig::default();
        assert_eq!(config.database.host, "localhost");
        assert_eq!(config.trading.initial_capital, 100000.0);
        assert_eq!(config.data.default_timeframe, "1d");
    }

    #[test]
    fn test_config_validation() {
        let mut config = BacktestConfig::default();
        
        // Test valid config
        assert!(config.validate().is_ok());
        
        // Test invalid commission rate
        config.trading.commission_rate = -0.1;
        assert!(config.validate().is_err());
        
        // Reset and test invalid capital
        config.trading.commission_rate = 0.001;
        config.trading.initial_capital = -1000.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_yaml_serialization() {
        let config = BacktestConfig::default();
        let yaml_str = serde_yaml::to_string(&config).unwrap();
        
        // Should contain expected fields
        assert!(yaml_str.contains("database"));
        assert!(yaml_str.contains("trading"));
        assert!(yaml_str.contains("data"));
        assert!(yaml_str.contains("output"));
        
        // Should be able to deserialize back
        let parsed: BacktestConfig = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(parsed.database.host, config.database.host);
    }

    #[test]
    fn test_file_operations() {
        let temp_path = PathBuf::from("test_config.yaml");
        let config = BacktestConfig::default();
        
        // Test saving
        config.to_file(&temp_path).unwrap();
        assert!(temp_path.exists());
        
        // Test loading
        let loaded_config = BacktestConfig::from_file(&temp_path).unwrap();
        assert_eq!(loaded_config.database.host, config.database.host);
        assert_eq!(loaded_config.trading.initial_capital, config.trading.initial_capital);
        
        // Cleanup
        fs::remove_file(&temp_path).ok();
    }
}
