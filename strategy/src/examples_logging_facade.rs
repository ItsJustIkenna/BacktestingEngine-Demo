//! Strategy Examples Logging Integration
//!
//! UltraLogger integration for strategy examples

use ultra_logger::UltraLogger;
use std::sync::Arc;
use once_cell::sync::Lazy;

/// Strategy examples logger instance
pub static EXAMPLES_LOGGER: Lazy<Arc<UltraLogger>> = 
    Lazy::new(|| {
        Arc::new(UltraLogger::new("strategy_examples".to_string()))
    });

/// Performance-optimized logging macros for strategy examples
/// These use tokio::spawn to ensure non-blocking operation

#[macro_export]
macro_rules! examples_log_info {
    ($logger:expr, $fmt:expr $(,$arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(,$arg)*);
        tokio::spawn(async move {
            let _ = logger.info(msg).await;
        });
    }};
}

#[macro_export]
macro_rules! examples_log_warn {
    ($logger:expr, $fmt:expr $(,$arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(,$arg)*);
        tokio::spawn(async move {
            let _ = logger.warn(msg).await;
        });
    }};
}

#[macro_export]
macro_rules! examples_log_error {
    ($logger:expr, $fmt:expr $(,$arg:expr)*) => {{
        let logger = $logger.clone();
        let msg = format!($fmt $(,$arg)*);
        tokio::spawn(async move {
            let _ = logger.error(msg).await;
        });
    }};
}
