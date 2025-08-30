//! WalkForward Module Logging Integration
//!
//! UltraLogger integration for the walkforward module

use ultra_logger::UltraLogger;
use std::sync::Arc;
use once_cell::sync::Lazy;

/// WalkForward-specific logger instance
pub static WALKFORWARD_LOGGER: Lazy<Arc<UltraLogger>> = 
    Lazy::new(|| {
        Arc::new(UltraLogger::new("walkforward".to_string()))
    });

/// Performance-optimized logging macros for walkforward module
/// These use tokio::spawn to ensure non-blocking operation

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
