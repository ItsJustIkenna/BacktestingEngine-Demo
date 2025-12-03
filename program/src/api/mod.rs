//! API module for REST endpoints and WebSocket

pub mod handlers;
pub mod router;
pub mod websocket;
pub mod history;
pub mod metadata;

pub use router::build_router;


