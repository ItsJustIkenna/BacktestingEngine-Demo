//! API router configuration

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use std::sync::Arc;

use super::handlers::{self, ApiState};
use super::websocket;
use super::history;
use super::metadata;
use crate::job_queue::JobQueue;

/// Build the API router
pub fn build_router(job_queue: Arc<dyn JobQueue>) -> Router {
    let state = ApiState { job_queue };
    
    Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        
        // API routes
        .route("/api/backtest", post(handlers::submit_backtest))
        .route("/api/backtest/:id", 
            get(handlers::get_job_details)
            .delete(handlers::delete_backtest))
        .route("/api/backtest/:id/status", get(handlers::get_job_status))
        .route("/api/backtest/:id/results", get(handlers::get_job_results))
        .route("/api/backtests", get(history::get_backtests))
        .route("/api/stats", get(handlers::queue_stats))
        
        // Metadata endpoints
        .route("/api/symbols", get(metadata::get_symbols))
        .route("/api/exchanges", get(metadata::get_exchanges))
        
        // WebSocket for real-time progress
        .route("/api/backtest/:id/progress", get(websocket::ws_handler))
        
        // Add CORS middleware
        .layer(CorsLayer::permissive())
        
        // Add state
        .with_state(state)
}
