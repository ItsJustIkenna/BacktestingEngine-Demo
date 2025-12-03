use ultra_logger::{UltraLogger, LoggerConfig, TransportConfig, ConnectionConfig};
use std::sync::Arc;
use std::collections::HashMap;

/// Create Elasticsearch configuration for BacktestingEngine
fn create_elasticsearch_config(_component: &str) -> LoggerConfig {
    // Check if we should use Elasticsearch (environment variable or default)
    let use_elasticsearch = std::env::var("USE_ELASTICSEARCH_LOGGING")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(true); // Default to true for production

    if use_elasticsearch {
        // Get credentials from environment variables set by Kubernetes
        let endpoint = std::env::var("ELASTICSEARCH_ENDPOINT")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_ENDPOINT"))
            .unwrap_or_else(|_| "https://my-observability-deployment-76d771.es.us-east-2.aws.elastic-cloud.com".to_string());
        let username = std::env::var("ELASTICSEARCH_USERNAME")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_USERNAME"))
            .unwrap_or_else(|_| "elastic".to_string());
        let password = std::env::var("ELASTICSEARCH_PASSWORD")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_PASSWORD"))
            .unwrap_or_else(|_| "mN9CNxqYU9J0nw4aywXVxTAw".to_string());

        let mut options = HashMap::new();
        options.insert("index".to_string(), "backtestingengine-logs".to_string());

        LoggerConfig {
            level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            transport: TransportConfig {
                transport_type: "elasticsearch".to_string(),
                connection: ConnectionConfig {
                    host: endpoint,
                    port: 443,
                    username: Some(username),
                    password: Some(password),
                    options,
                },
            },
        }
    } else {
        // Fallback to stdout for development
        LoggerConfig::default()
    }
}

/// Backtest Engine specific logger instance
pub static BACKTEST_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-backtest_engine".to_string(),
            create_elasticsearch_config("backtest_engine")
        ))
    });

/// Strategy execution logger
pub static STRATEGY_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-strategy".to_string(),
            create_elasticsearch_config("strategy")
        ))
    });

/// Portfolio management logger
pub static PORTFOLIO_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-portfolio".to_string(),
            create_elasticsearch_config("portfolio")
        ))
    });

/// Risk management logger
pub static RISK_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-risk".to_string(),
            create_elasticsearch_config("risk")
        ))
    });

/// Order book operations logger
pub static ORDERBOOK_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-orderbook".to_string(),
            create_elasticsearch_config("orderbook")
        ))
    });

/// Data loading logger
pub static DATALOADER_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-dataloader".to_string(),
            create_elasticsearch_config("dataloader")
        ))
    });

/// Walk-forward analysis logger
pub static WALKFORWARD_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-walkforward".to_string(),
            create_elasticsearch_config("walkforward")
        ))
    });

/// Genetic optimization logger
pub static GENETIC_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-genetic".to_string(),
            create_elasticsearch_config("genetic")
        ))
    });

/// Metrics and reporting logger
pub static METRICS_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-metrics".to_string(),
            create_elasticsearch_config("metrics")
        ))
    });

/// Main program logger
pub static MAIN_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-main".to_string(),
            create_elasticsearch_config("main")
        ))
    });

/// Performance-optimized logging macros for BacktestingEngine
/// These use tokio::spawn to ensure non-blocking operation in sync contexts

#[macro_export]
/// Log info level messages using UltraLogger
#[macro_export]
macro_rules! log_info {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        tokio::spawn(async move {
            let _ = logger.info(msg).await;
        });
    }};
}

#[macro_export]
macro_rules! log_warn {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        tokio::spawn(async move {
            let _ = logger.warn(msg).await;
        });
    }};
}

#[macro_export]
macro_rules! log_error {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        tokio::spawn(async move {
            let _ = logger.error(msg).await;
        });
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        tokio::spawn(async move {
            let _ = logger.debug(msg).await;
        });
    }};
}

/// Log trace level messages using UltraLogger
#[macro_export]
macro_rules! log_trace {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        tokio::spawn(async move {
            let _ = logger.debug(msg).await;
        });
    }};
}

/// Utility struct for BacktestingEngine logging
pub struct BacktestEngineLogger {
    logger: Arc<UltraLogger>,
}

impl BacktestEngineLogger {
    pub fn new(service_name: &str) -> Self {
        Self {
            logger: Arc::new(UltraLogger::new(service_name.to_string())),
        }
    }

    pub fn get_logger(&self) -> Arc<UltraLogger> {
        self.logger.clone()
    }
}


