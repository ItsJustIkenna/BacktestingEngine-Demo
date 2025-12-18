//! Prometheus metrics for observability
//!
//! Exposes metrics at /metrics endpoint for Prometheus scraping.
//!
//! # Metrics Exported
//!
//! ## Counters
//! - `backtest_jobs_submitted_total` - Total jobs submitted
//! - `backtest_jobs_completed_total` - Total jobs completed successfully
//! - `backtest_jobs_failed_total` - Total jobs failed
//!
//! ## Gauges
//! - `backtest_queue_depth` - Current number of jobs in queue
//! - `backtest_workers_active` - Number of active workers
//!
//! ## Histograms
//! - `backtest_job_duration_seconds` - Job execution time
//! - `backtest_api_request_duration_seconds` - API request latency

use axum::{
    extract::State,
    response::IntoResponse,
};
use metrics_core::{counter, gauge, histogram, describe_counter, describe_gauge, describe_histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::{Arc, OnceLock};

/// Global static for the metrics handle - ensures we only initialize once
static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialize the Prometheus metrics recorder
/// 
/// This function is idempotent - it can be called multiple times safely.
/// Only the first call actually installs the recorder; subsequent calls
/// return a clone of the existing handle.
/// 
/// Returns a handle for the /metrics endpoint.
pub fn init_metrics() -> PrometheusHandle {
    METRICS_HANDLE.get_or_init(|| {
        let builder = PrometheusBuilder::new();
        let handle = builder
            .install_recorder()
            .expect("Failed to install Prometheus recorder");
        
        // Describe all metrics for documentation
        describe_counter!(
            "backtest_jobs_submitted_total",
            "Total number of backtest jobs submitted"
        );
        describe_counter!(
            "backtest_jobs_completed_total",
            "Total number of backtest jobs completed successfully"
        );
        describe_counter!(
            "backtest_jobs_failed_total",
            "Total number of backtest jobs that failed"
        );
        describe_gauge!(
            "backtest_queue_depth",
            "Current number of jobs waiting in queue"
        );
        describe_gauge!(
            "backtest_workers_active",
            "Number of workers currently processing jobs"
        );
        describe_histogram!(
            "backtest_job_duration_seconds",
            "Time taken to complete a backtest job"
        );
        describe_histogram!(
            "backtest_api_request_duration_seconds",
            "API request latency in seconds"
        );
        describe_histogram!(
            "backtest_data_prep_duration_seconds",
            "Time taken to prepare binary data files"
        );
        describe_counter!(
            "backtest_data_prep_bytes_total",
            "Total bytes of data prepared"
        );
        describe_gauge!(
            "backtest_binary_files_count",
            "Number of binary data files available"
        );
        
        tracing::info!("📊 Prometheus metrics initialized");
        
        handle
    }).clone()
}

/// Metrics handle for dependency injection
#[derive(Clone)]
pub struct MetricsState {
    pub handle: Arc<PrometheusHandle>,
}

/// Handler for /metrics endpoint
pub async fn metrics_handler(State(state): State<MetricsState>) -> impl IntoResponse {
    state.handle.render()
}// ============================================================================
// METRIC RECORDING HELPERS
// ============================================================================

/// Record a job submission
pub fn record_job_submitted(symbol: &str, exchange: &str, method: &str) {
    counter!(
        "backtest_jobs_submitted_total",
        "symbol" => symbol.to_string(),
        "exchange" => exchange.to_string(),
        "method" => method.to_string()
    ).increment(1);
}

/// Record a job completion
pub fn record_job_completed(symbol: &str, exchange: &str, method: &str, duration_secs: f64) {
    counter!(
        "backtest_jobs_completed_total",
        "symbol" => symbol.to_string(),
        "exchange" => exchange.to_string(),
        "method" => method.to_string()
    ).increment(1);
    
    histogram!(
        "backtest_job_duration_seconds",
        "symbol" => symbol.to_string(),
        "method" => method.to_string()
    ).record(duration_secs);
}

/// Record a job failure
pub fn record_job_failed(symbol: &str, exchange: &str, method: &str, error_type: &str) {
    counter!(
        "backtest_jobs_failed_total",
        "symbol" => symbol.to_string(),
        "exchange" => exchange.to_string(),
        "method" => method.to_string(),
        "error_type" => error_type.to_string()
    ).increment(1);
}

/// Update queue depth gauge
pub fn set_queue_depth(depth: usize) {
    gauge!("backtest_queue_depth").set(depth as f64);
}

/// Update active workers gauge
pub fn set_active_workers(count: usize) {
    gauge!("backtest_workers_active").set(count as f64);
}

/// Record API request latency
pub fn record_api_latency(endpoint: &str, method: &str, status: u16, duration_secs: f64) {
    histogram!(
        "backtest_api_request_duration_seconds",
        "endpoint" => endpoint.to_string(),
        "method" => method.to_string(),
        "status" => status.to_string()
    ).record(duration_secs);
}

/// Record data preparation metrics
pub fn record_data_prep(symbol: &str, exchange: &str, bytes: u64, duration_secs: f64) {
    histogram!(
        "backtest_data_prep_duration_seconds",
        "symbol" => symbol.to_string(),
        "exchange" => exchange.to_string()
    ).record(duration_secs);
    
    counter!(
        "backtest_data_prep_bytes_total",
        "symbol" => symbol.to_string(),
        "exchange" => exchange.to_string()
    ).increment(bytes);
}

/// Set binary files count
pub fn set_binary_files_count(count: usize) {
    gauge!("backtest_binary_files_count").set(count as f64);
}
