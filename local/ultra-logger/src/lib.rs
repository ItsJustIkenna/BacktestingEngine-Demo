//! Fresh in-repo logger for standalone demo builds.
//! Provides the type and method surface the demo crates already call.

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

static GLOBAL: OnceCell<Arc<UltraLogger>> = OnceCell::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Critical => "CRITICAL",
        };
        f.write_str(s)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("logger error: {0}")]
    Message(String),
}

pub type LogResult<T> = Result<T, LogError>;

#[derive(Debug, Clone, Default)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub options: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct TransportConfig {
    pub transport_type: String,
    pub connection: ConnectionConfig,
}

#[derive(Debug, Clone, Default)]
pub struct LoggerConfig {
    pub level: String,
    pub transport: TransportConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogEntry {
    pub level: LogLevel,
    pub component: String,
    pub message: String,
    pub metadata: HashMap<String, String>,
    pub timestamp: String,
}

impl LogEntry {
    pub fn new(level: LogLevel, component: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level,
            component: component.into(),
            message: message.into(),
            metadata: HashMap::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone)]
pub struct UltraLogger {
    component: String,
    _config: LoggerConfig,
    #[allow(dead_code)]
    min_level: String,
}

impl UltraLogger {
    pub fn new(component: String) -> Self {
        Self::with_config(component, LoggerConfig::default())
    }

    pub fn with_config(component: String, config: LoggerConfig) -> Self {
        Self {
            min_level: if config.level.is_empty() {
                "info".to_string()
            } else {
                config.level.clone()
            },
            component,
            _config: config,
        }
    }

    fn emit(&self, level: LogLevel, message: &str, metadata: Option<&HashMap<String, String>>) {
        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
        if let Some(meta) = metadata {
            if !meta.is_empty() {
                eprintln!("[{ts}] [{level}] [{}] {message} {meta:?}", self.component);
                return;
            }
        }
        eprintln!("[{ts}] [{level}] [{}] {message}", self.component);
    }

    async fn emit_result(&self, level: LogLevel, message: String) -> Result<(), LogError> {
        self.emit(level, &message, None);
        Ok(())
    }

    pub async fn log(&self, level: LogLevel, message: impl Into<String>) -> Result<(), LogError> {
        self.emit_result(level, message.into()).await
    }

    pub async fn info(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Info, message).await
    }
    pub async fn warn(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Warn, message).await
    }
    pub async fn error(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Error, message).await
    }
    pub async fn debug(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Debug, message).await
    }
    pub async fn trace(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Trace, message).await
    }
    pub async fn critical(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.log(LogLevel::Critical, message).await
    }

    pub fn info_sync(&self, message: impl Into<String>) {
        self.emit(LogLevel::Info, &message.into(), None);
    }
    pub fn warn_sync(&self, message: impl Into<String>) {
        self.emit(LogLevel::Warn, &message.into(), None);
    }
    pub fn error_sync(&self, message: impl Into<String>) {
        self.emit(LogLevel::Error, &message.into(), None);
    }
    pub fn debug_sync(&self, message: impl Into<String>) {
        self.emit(LogLevel::Debug, &message.into(), None);
    }

    pub async fn info_ctx(&self, message: impl Into<String>, _ctx: impl std::fmt::Debug) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn warn_ctx(&self, message: impl Into<String>, _ctx: impl std::fmt::Debug) -> Result<(), LogError> {
        self.warn(message).await
    }
    pub async fn error_ctx(&self, message: impl Into<String>, _ctx: impl std::fmt::Debug) -> Result<(), LogError> {
        self.error(message).await
    }
    pub async fn debug_ctx(&self, message: impl Into<String>, _ctx: impl std::fmt::Debug) -> Result<(), LogError> {
        self.debug(message).await
    }

    pub async fn log_entry(&self, entry: LogEntry) -> Result<(), LogError> {
        self.emit(entry.level, &entry.message, Some(&entry.metadata));
        Ok(())
    }

    pub async fn log_with_metadata(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        metadata: HashMap<String, String>,
    ) -> Result<(), LogError> {
        let msg = message.into();
        self.emit(level, &msg, Some(&metadata));
        Ok(())
    }

    pub async fn log_signal(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_execution(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_portfolio_update(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_risk_check(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_websocket_event(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_orderbook_update(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_trade_processing(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_performance_metrics(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
    pub async fn log_critical_error(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.error(message).await
    }
    pub async fn log_auth_event(&self, message: impl Into<String>) -> Result<(), LogError> {
        self.info(message).await
    }
}

pub fn init_global_logger(component_name: String) {
    let _ = GLOBAL.set(Arc::new(UltraLogger::new(component_name)));
}

pub fn init_global_logger_from_env(component_name: String) {
    init_global_logger(component_name);
}

pub fn get_global_logger() -> Option<Arc<UltraLogger>> {
    GLOBAL.get().cloned()
}

#[macro_export]
macro_rules! ultra_error {
    ($msg:expr) => {{
        eprintln!("[ERROR] {}", $msg);
    }};
    ($($arg:tt)*) => {{
        eprintln!("[ERROR] {}", format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! ultra_warn {
    ($msg:expr) => {{
        eprintln!("[WARN] {}", $msg);
    }};
    ($($arg:tt)*) => {{
        eprintln!("[WARN] {}", format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! ultra_info {
    ($msg:expr) => {{
        eprintln!("[INFO] {}", $msg);
    }};
    ($($arg:tt)*) => {{
        eprintln!("[INFO] {}", format!($($arg)*));
    }};
}

