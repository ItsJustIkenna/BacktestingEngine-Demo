//! API module for REST endpoints and WebSocket

pub mod handlers;
pub mod router;
pub mod websocket;
pub mod history;
pub mod metadata;
pub mod security;
pub mod metrics;

pub use router::build_router;
pub use security::{ApiKeyStore, auth_middleware};
pub use metrics::{init_metrics, MetricsState, metrics_handler};