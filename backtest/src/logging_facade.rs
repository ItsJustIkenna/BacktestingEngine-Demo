//! Backtest Module Logging Integration
//!
//! UltraLogger integration for the backtest module with Elasticsearch support

use ultra_logger::{UltraLogger, LoggerConfig, TransportConfig, ConnectionConfig};
use std::sync::Arc;
use std::collections::HashMap;
use once_cell::sync::Lazy;

/// Create Elasticsearch configuration for backtest module
fn create_elasticsearch_config() -> LoggerConfig {
    // Check if we should use Elasticsearch (environment variable or default)
    let use_elasticsearch = std::env::var("USE_ELASTICSEARCH_LOGGING")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(true); // Default to true for production

    if use_elasticsearch {
        // Get credentials from environment variables set by Kubernetes
        let endpoint = std::env::var("ELASTICSEARCH_ENDPOINT")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_ENDPOINT"))
            .unwrap_or_default();
        let username = std::env::var("ELASTICSEARCH_USERNAME")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_USERNAME"))
            .unwrap_or_else(|_| "elastic".to_string());
        let password = std::env::var("ELASTICSEARCH_PASSWORD")
            .or_else(|_| std::env::var("ELASTIC_CLOUD_PASSWORD"))
            .unwrap_or_default();

        // Only configure elasticsearch if endpoint is set
        if endpoint.is_empty() {
            return LoggerConfig::default();
        }

        let mut options = HashMap::new();
        options.insert("index".to_string(), "backtestingengine-backtest-logs".to_string());

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

/// Backtest-specific logger instance with Elasticsearch transport
pub static BACKTEST_LOGGER: Lazy<Arc<UltraLogger>> = 
    Lazy::new(|| {
        Arc::new(UltraLogger::with_config(
            "BacktestingEngine-simulation".to_string(),
            create_elasticsearch_config()
        ))
    });

/// Performance-optimized logging macros for backtest module
/// These use tokio::spawn to ensure non-blocking operation
/// Safe to call from any context - silently drops logs if no Tokio runtime is available
/// (e.g., when called from Rayon threads during tests)

#[macro_export]
macro_rules! log_info {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        // Only log if Tokio runtime is available (safe for Rayon threads)
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let logger = $logger.clone();
            let msg = format!($fmt $(, $arg)*);
            handle.spawn(async move {
                let _ = logger.info(msg).await;
            });
        }
    }};
}

#[macro_export]
macro_rules! log_warn {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let logger = $logger.clone();
            let msg = format!($fmt $(, $arg)*);
            handle.spawn(async move {
                let _ = logger.warn(msg).await;
            });
        }
    }};
}

#[macro_export]
macro_rules! log_error {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let logger = $logger.clone();
            let msg = format!($fmt $(, $arg)*);
            handle.spawn(async move {
                let _ = logger.error(msg).await;
            });
        }
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let logger = $logger.clone();
            let msg = format!($fmt $(, $arg)*);
            handle.spawn(async move {
                let _ = logger.debug(msg).await;
            });
        }
    }};
}
