use ultra_logger::UltraLogger;
use std::sync::Arc;

/// Backtest Engine specific logger instance
pub static BACKTEST_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("backtest_engine".to_string()))
    });

/// Strategy execution logger
pub static STRATEGY_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("strategy".to_string()))
    });

/// Portfolio management logger
pub static PORTFOLIO_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("portfolio".to_string()))
    });

/// Risk management logger
pub static RISK_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("risk".to_string()))
    });

/// Order book operations logger
pub static ORDERBOOK_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("orderbook".to_string()))
    });

/// Data loading logger
pub static DATALOADER_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("dataloader".to_string()))
    });

/// Walk-forward analysis logger
pub static WALKFORWARD_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("walkforward".to_string()))
    });

/// Genetic optimization logger
pub static GENETIC_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("genetic".to_string()))
    });

/// Metrics and reporting logger
pub static METRICS_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("metrics".to_string()))
    });

/// Main program logger
pub static MAIN_LOGGER: once_cell::sync::Lazy<Arc<UltraLogger>> = 
    once_cell::sync::Lazy::new(|| {
        Arc::new(UltraLogger::new("main".to_string()))
    });

/// Performance-optimized logging macros for BacktestingEngine
/// These use tokio::spawn to ensure non-blocking operation in sync contexts

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
