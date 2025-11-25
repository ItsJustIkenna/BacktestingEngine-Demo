//! Integration tests for the config module

use config::{BacktestConfig, DatabaseConfig, TradingConfig, DataConfig, OutputConfig, ReportFormat, AnalysisConfig};
use std::fs;
use tempfile::TempDir;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creation() {
        let config = BacktestConfig::default();
        
        // Test database config defaults
        assert_eq!(config.database.host, "localhost");
        assert_eq!(config.database.port, 5432);
        assert_eq!(config.database.database, "trading_platform");
        assert_eq!(config.database.username, "postgres");
        
        // Test trading config defaults
        assert_eq!(config.trading.initial_capital, 100000.0);
        assert_eq!(config.trading.commission_rate, 0.001);
        
        // Test data config defaults
        assert_eq!(config.data.default_timeframe, "1d");
        assert!(config.data.cache_enabled);
        
        // Test output config defaults
        assert!(config.output.results_dir.ends_with("results"));
        assert!(config.output.charts_enabled);
    }

    #[test]
    fn test_config_validation() {
        let mut config = BacktestConfig::default();
        
        // Test valid config
        assert!(config.validate().is_ok());
        
        // Test invalid initial capital
        config.trading.initial_capital = -1000.0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Initial capital must be positive"));
        
        // Reset and test invalid commission rate
        config.trading.initial_capital = 100000.0;
        config.trading.commission_rate = -0.1;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Commission rate cannot be negative"));
        
        // Test empty data sources
        config.trading.commission_rate = 0.001;
        config.data.data_sources.clear();
        let result = config.validate();
        assert!(result.is_err());
        // The actual error message uses "Missing required field" for empty data_sources
        assert!(result.unwrap_err().to_string().contains("data.data_sources"));
    }

    #[test]
    fn test_yaml_serialization_deserialization() {
        let config = BacktestConfig::default();
        
        // Serialize to YAML
        let yaml_str = serde_yaml::to_string(&config).unwrap();
        assert!(yaml_str.contains("database:"));
        assert!(yaml_str.contains("trading:"));
        assert!(yaml_str.contains("data:"));
        assert!(yaml_str.contains("output:"));
        
        // Deserialize back from YAML
        let parsed_config: BacktestConfig = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(parsed_config.database.host, config.database.host);
        assert_eq!(parsed_config.trading.initial_capital, config.trading.initial_capital);
        assert_eq!(parsed_config.data.default_timeframe, config.data.default_timeframe);
    }

    #[test]
    fn test_config_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.yaml");
        
        let mut config = BacktestConfig::default();
        config.trading.initial_capital = 50000.0;
        config.data.data_sources = vec!["binance".to_string(), "coinbase".to_string()];
        
        // Save config to file
        assert!(config.to_file(&config_path).is_ok());
        assert!(config_path.exists());
        
        // Load config from file
        let loaded_config = BacktestConfig::from_file(&config_path).unwrap();
        assert_eq!(loaded_config.trading.initial_capital, 50000.0);
        assert_eq!(loaded_config.data.data_sources.len(), 2);
        assert!(loaded_config.data.data_sources.contains(&"binance".to_string()));
        assert!(loaded_config.data.data_sources.contains(&"coinbase".to_string()));
    }

    #[test]
    fn test_config_error_handling() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.yaml");
        
        // Test loading from nonexistent file
        let result = BacktestConfig::from_file(&nonexistent_path);
        assert!(result.is_err());
        // from_file returns IO error, not ConfigError::FileNotFound
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No such file") || error_msg.contains("cannot find") || error_msg.contains("not found"));
        
        // Test loading from invalid YAML
        let invalid_yaml_path = temp_dir.path().join("invalid.yaml");
        fs::write(&invalid_yaml_path, "invalid: yaml: content: [").unwrap();
        
        let result = BacktestConfig::from_file(&invalid_yaml_path);
        assert!(result.is_err());
        // Should be a YAML parsing error
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("YAML") || error_msg.contains("parsing") || error_msg.contains("deserialize"));
    }

    #[test]
    fn test_report_format_display() {
        assert_eq!(ReportFormat::Html.to_string(), "html");
        assert_eq!(ReportFormat::Json.to_string(), "json");
        assert_eq!(ReportFormat::Csv.to_string(), "csv");
        assert_eq!(ReportFormat::Pdf.to_string(), "pdf");
    }

    #[test]
    fn test_database_config_construction() {
        let db_config = DatabaseConfig {
            host: "testhost".to_string(),
            port: 5433,
            database: "testdb".to_string(),
            username: "testuser".to_string(),
            password: "testpass".to_string(),
            connection_timeout: Some(30),
            max_connections: Some(20),
        };
        
        assert_eq!(db_config.host, "testhost");
        assert_eq!(db_config.port, 5433);
        assert_eq!(db_config.database, "testdb");
        assert_eq!(db_config.username, "testuser");
        assert_eq!(db_config.password, "testpass");
        assert_eq!(db_config.connection_timeout, Some(30));
        assert_eq!(db_config.max_connections, Some(20));
    }

    #[test]
    fn test_data_config_construction() {
        let data_config = DataConfig {
            default_timeframe: "1h".to_string(),
            data_sources: vec!["database".to_string(), "binance".to_string()],
            cache_enabled: false,
            cache_duration_hours: 12,
            gap_detection: config::GapDetectionConfig::default(),
            market_impact: config::MarketImpactConfig::default(),
        };
        
        // This would be validated in the parent BacktestConfig validation
        let config = BacktestConfig {
            database: DatabaseConfig::default(),
            trading: TradingConfig::default(),
            data: data_config,
            output: OutputConfig::default(),
            analysis: AnalysisConfig::default(),
        };
        
        assert_eq!(config.data.default_timeframe, "1h");
        assert_eq!(config.data.data_sources.len(), 2);
        assert!(!config.data.cache_enabled);
        assert_eq!(config.data.cache_duration_hours, 12);
    }

    #[test]
    fn test_output_config_construction() {
        let temp_dir = TempDir::new().unwrap();
        let output_config = OutputConfig {
            results_dir: temp_dir.path().join("results").to_string_lossy().to_string(),
            reports_dir: temp_dir.path().join("reports").to_string_lossy().to_string(),
            charts_enabled: true,
            export_formats: vec!["json".to_string(), "csv".to_string()],
        };
        
        assert!(output_config.results_dir.contains("results"));
        assert!(output_config.reports_dir.contains("reports"));
        assert_eq!(output_config.export_formats.len(), 2);
        assert!(output_config.charts_enabled);
    }
}
