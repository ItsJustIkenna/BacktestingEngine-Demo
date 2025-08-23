use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
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

/// Report generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    pub output_dir: PathBuf,
    pub formats: Vec<ReportFormat>,
    pub include_trades: bool,
    pub include_charts: bool,
    pub template_dir: Option<PathBuf>,
    pub custom_css: Option<String>,
    /// Whether to persist report metadata to database
    pub persist_to_database: bool,
    /// Database connection for persistence (if enabled)
    pub database_url: Option<String>,
    /// User or system generating the report
    pub generated_by: Option<String>,
    /// Source of report generation
    pub generation_source: String,
}

/// Supported report formats
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportFormat {
    Html,
    Json,
    Csv,
    Pdf,
}

impl std::fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportFormat::Html => write!(f, "html"),
            ReportFormat::Json => write!(f, "json"),
            ReportFormat::Csv => write!(f, "csv"),
            ReportFormat::Pdf => write!(f, "pdf"),
        }
    }
}

/// Genetic algorithm configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneticConfig {
    pub population_size: usize,
    pub generations: usize,
    pub crossover_rate: f64,
    pub mutation_rate: f64,
}

/// Configuration for order book reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookConfig {
    /// Maximum number of price levels to maintain on each side
    pub max_levels: Option<usize>,
    /// Whether to maintain detailed order information
    pub track_individual_orders: bool,
    /// Snapshot validation frequency (in events)
    pub snapshot_validation_frequency: Option<usize>,
    /// Whether to enable strict validation
    pub strict_validation: bool,
}

/// Batch processing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    pub batch_size: usize,
    pub max_parallel: usize,
    pub retry_attempts: usize,
}

/// Strategy configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Unique strategy identifier
    pub id: String,
    /// Human-readable strategy name
    pub name: String,
    /// Strategy type/category
    pub strategy_type: String,
    /// Symbol to trade
    pub symbol: String,
    /// Exchange to trade on
    pub exchange: String,
    /// Strategy-specific parameters
    pub parameters: HashMap<String, ParameterValue>,
    /// Risk management settings
    pub risk_limits: RiskLimits,
    /// Whether the strategy is enabled
    pub enabled: bool,
}

/// Parameter value that can hold different types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
}

impl ParameterValue {
    /// Get as integer
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ParameterValue::Integer(v) => Some(*v),
            ParameterValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            ParameterValue::Float(v) => Some(*v),
            ParameterValue::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Get as string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            ParameterValue::String(v) => Some(v),
            _ => None,
        }
    }

    /// Get as boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParameterValue::Boolean(v) => Some(*v),
            _ => None,
        }
    }
}

/// Risk management limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum position size
    pub max_position_size: f64,
    /// Maximum order size
    pub max_order_size: f64,
    /// Maximum daily loss threshold
    pub max_daily_loss: f64,
    /// Maximum portfolio value at risk
    pub max_var: Option<f64>,
    /// Stop loss percentage
    pub stop_loss_pct: Option<f64>,
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

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./reports"),
            formats: vec![ReportFormat::Html, ReportFormat::Json],
            include_trades: true,
            include_charts: true,
            template_dir: None,
            custom_css: None,
            persist_to_database: false,
            database_url: None,
            generated_by: None,
            generation_source: "cli".to_string(),
        }
    }
}

impl Default for GeneticConfig {
    fn default() -> Self {
        Self {
            population_size: 50,
            generations: 100,
            crossover_rate: 0.8,
            mutation_rate: 0.1,
        }
    }
}

impl Default for OrderBookConfig {
    fn default() -> Self {
        Self {
            max_levels: Some(100), // Keep top 100 levels by default
            track_individual_orders: true,
            snapshot_validation_frequency: Some(1000),
            strict_validation: true,
        }
    }
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            max_parallel: 4,
            retry_attempts: 3,
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default Strategy".to_string(),
            strategy_type: "basic".to_string(),
            symbol: "BTCUSD".to_string(),
            exchange: "default".to_string(),
            parameters: HashMap::new(),
            risk_limits: RiskLimits::default(),
            enabled: true,
        }
    }
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position_size: 1.0,
            max_order_size: 0.1,
            max_daily_loss: 1000.0,
            max_var: None,
            stop_loss_pct: None,
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
