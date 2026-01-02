//! API module for REST endpoints and WebSocket

pub mod handlers;
pub mod router;
pub mod websocket;
pub mod history;
pub mod metadata;
pub mod security;
pub mod metrics;
pub mod tardis_upload;

pub use router::build_router;
pub use security::{ApiKeyStore, auth_middleware};
pub use metrics::{init_metrics, MetricsState, metrics_handler};
pub use tardis_upload::{TardisUploadState, upload_file, list_sessions, delete_session, submit_tardis_backtest};