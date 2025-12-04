//! Metrics Module Logging Integration
//!
//! UltraLogger integration for the metrics module

use ultra_logger::UltraLogger;
use std::sync::Arc;
use once_cell::sync::Lazy;

/// Metrics-specific logger instance
pub static METRICS_LOGGER: Lazy<Arc<UltraLogger>> = 
    Lazy::new(|| {
        Arc::new(UltraLogger::new("metrics".to_string()))
    });

/// Performance-optimized logging macros for metrics module
/// These use sync versions to avoid async overhead in reporting contexts

#[macro_export]
macro_rules! log_info {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        logger.info_sync(msg);
    }};
}

#[macro_export]
macro_rules! log_warn {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        logger.warn_sync(msg);
    }};
}

#[macro_export]
macro_rules! log_error {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        logger.error_sync(msg);
    }};
}

#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $fmt:expr $(, $arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(, $arg)*);
        logger.debug_sync(msg);
    }};
}
