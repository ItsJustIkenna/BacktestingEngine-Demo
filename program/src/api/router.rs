//! API router configuration

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use std::sync::Arc;

use super::handlers::{self, ApiState};
use super::websocket;
use super::history;
use super::metadata;
use super::security::{self, ApiKeyStore};
use super::metrics::{self, MetricsState};
use crate::job_queue::JobQueue;

/// Build the API router with security middleware
pub fn build_router(job_queue: Arc<dyn JobQueue>) -> Router {
    let api_state = ApiState { job_queue };
    let key_store = ApiKeyStore::new();
    
    // Initialize Prometheus metrics
    let metrics_handle = metrics::init_metrics();
    let metrics_state = MetricsState { 
        handle: Arc::new(metrics_handle) 
    };
    
    // Public routes (no auth required) - health checks and metrics
    let public_routes = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/health/live", get(handlers::liveness_check));
    
    // Readiness check needs ApiState to check job queue
    let readiness_route = Router::new()
        .route("/health/ready", get(handlers::readiness_check))
        .with_state(api_state.clone());
    
    // Metrics endpoint with its own state
    let metrics_routes = Router::new()
        .route("/metrics", get(metrics::metrics_handler))
        .with_state(metrics_state);
    
    // Protected API routes with ApiState
    let api_routes = Router::new()
        .route("/api/backtest", post(handlers::submit_backtest))
        .route("/api/backtest/cleanup", post(handlers::cleanup_old_backtests))
        .route("/api/backtest/:id", 
            get(handlers::get_job_details)
            .delete(handlers::delete_backtest))
        .route("/api/backtest/:id/cancel", post(handlers::cancel_backtest))
        .route("/api/backtest/:id/status", get(handlers::get_job_status))
        .route("/api/backtest/:id/results", get(handlers::get_job_results))
        .route("/api/backtests", get(history::get_backtests))
        .route("/api/stats", get(handlers::queue_stats))
        .route("/api/symbols", get(metadata::get_symbols))
        .route("/api/exchanges", get(metadata::get_exchanges))
        .route("/api/data/prepare", post(handlers::prepare_data))
        .route("/api/backtest/:id/progress", get(websocket::ws_handler))
        .with_state(api_state)
        // Apply API key authentication to protected routes
        .layer(middleware::from_fn_with_state(key_store, security::auth_middleware));
    
    // Merge all routers
    Router::new()
        .merge(public_routes)
        .merge(readiness_route)
        .merge(metrics_routes)
        .merge(api_routes)
        .layer(CorsLayer::permissive())
}