//! API router configuration

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post, delete},
    Router,
};
use tower_http::cors::CorsLayer;
use std::sync::Arc;
use std::path::PathBuf;

use super::handlers::{self, ApiState};
use super::websocket;
use super::history;
use super::metadata;
use super::security::{self, ApiKeyStore};
use super::metrics::{self, MetricsState};
use super::tardis_upload::{self, TardisUploadState};
use crate::job_queue::JobQueue;

/// Build the API router with security middleware
pub fn build_router(job_queue: Arc<dyn JobQueue>) -> Router {
    let api_state = ApiState { job_queue: Arc::clone(&job_queue) };
    let key_store = ApiKeyStore::new();
    
    // Initialize Tardis upload state
    let upload_dir = std::env::var("TARDIS_UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/tardis_uploads"));
    let tardis_state = TardisUploadState::new(upload_dir);
    
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
    
    // Tardis file upload routes - upload now auto-imports to database
    // Allow up to 2GB uploads for large Tardis CSV files
    #[cfg(feature = "postgres")]
    let tardis_routes = Router::new()
        .route("/api/tardis/upload", post(tardis_upload::upload_file))
        .route("/api/tardis/sessions", get(tardis_upload::list_sessions))
        .route("/api/tardis/sessions/:session_id", delete(tardis_upload::delete_session))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2GB limit for file uploads
        .with_state((tardis_state.clone(), Arc::clone(&job_queue)));
    
    #[cfg(not(feature = "postgres"))]
    let tardis_routes = Router::new()
        .route("/api/tardis/sessions", get(tardis_upload::list_sessions))
        .route("/api/tardis/sessions/:session_id", delete(tardis_upload::delete_session))
        .with_state((tardis_state.clone(), Arc::clone(&job_queue)));
    
    // Tardis backtest submission needs both upload state and job queue
    let tardis_backtest_route = Router::new()
        .route("/api/tardis/backtest", post(tardis_upload::submit_tardis_backtest))
        .route("/api/tardis/import", post(tardis_upload::import_tardis_to_database))
        .with_state((tardis_state, Arc::clone(&job_queue)));
    
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
        .merge(tardis_routes)
        .merge(tardis_backtest_route)
        .merge(api_routes)
        .layer(CorsLayer::permissive())
}