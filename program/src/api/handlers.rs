//! REST API handlers for backtest service

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use crate::job_queue::{BacktestJobParams, BacktestResult, JobQueue, JobStatus};
use crate::log_warn;
// use crate::logging_facade::MAIN_LOGGER;

/// API state shared across handlers
#[derive(Clone)]
pub struct ApiState {
    pub job_queue: Arc<dyn JobQueue>,
}

/// API error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Job submission response
#[derive(Debug, Serialize)]
pub struct JobSubmitResponse {
    pub job_id: String,
    pub status: String,
    pub message: String,
}

/// Job status response
#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub status: String,
    pub progress: f64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
    pub current_phase: String,
    pub phase_details: serde_json::Value,
}

/// Job details response
#[derive(Debug, Serialize)]
pub struct JobDetailsResponse {
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub strategy: String,
    pub initial_capital: f64,
    pub optimization_method: String,
    pub population_size: Option<u32>,
    pub generations: Option<u32>,
    pub created_at: String,
    pub started_at: Option<String>,
    // All configuration parameters from BacktestJobParams
    pub initial_inventory_pct: f64,
    pub initial_30d_volume: f64,
    pub monte_carlo_enabled: bool,
    pub monte_carlo_runs: usize,
    pub walk_forward_enabled: bool,
    pub train_ratio: f64,
    pub test_ratio: f64,
    pub validate_ratio: f64,
    pub auto_save_profitable: bool,
    pub min_sharpe_ratio: f64,
    pub min_profit_factor: f64,
    pub max_drawdown: f64,
    pub min_win_rate: f64,
    pub min_trades: usize,
    pub min_sortino_ratio: Option<f64>,
    pub min_calmar_ratio: Option<f64>,
    pub min_total_pnl: Option<f64>,
    pub fitness_weights: Option<crate::job_queue::FitnessWeights>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub commission_rate: f64,
    // Manual strategy parameters
    pub risk_aversion: f64,
    pub inventory_target: f64,
    pub order_size: f64,
    pub order_size_pct: f64,
    pub window_ms: u64,
    pub min_quote_lifetime_ms: u64,
    pub volatility_cap: f64,
    pub time_horizon_minutes: u64,
    pub market_depth_k: f64,
    pub volatility_adjustment_factor: f64,
    pub inventory_penalty_factor: f64,
    pub trend_sensitivity: f64,
    pub flow_sensitivity: f64,
    pub take_profit_pct: f64,
    pub stop_loss_pct: f64,
    pub min_spread_bps: f64,
    pub max_trades_per_minute: f64,
}

/// Submit a new backtest job
/// POST /api/backtest
pub async fn submit_backtest(
    State(state): State<ApiState>,
    Json(mut params): Json<BacktestJobParams>,
) -> Result<Json<JobSubmitResponse>, AppError> {
    // log_info!(...)
    
    // Validate order_size_pct - must be between 0.001 (0.1%) and 0.25 (25%) of capital per trade
    // This is the preferred way to specify order size - as a percentage of available capital
    if params.order_size_pct < 0.001 {
        log_warn!(crate::MAIN_LOGGER, "order_size_pct {} too small, setting to 0.1% minimum", 
            params.order_size_pct * 100.0);
        params.order_size_pct = 0.001;
    } else if params.order_size_pct > 0.25 {
        log_warn!(crate::MAIN_LOGGER, "order_size_pct {} too large (>25%), capping at 10%", 
            params.order_size_pct * 100.0);
        params.order_size_pct = 0.10;
    }
    
    // Auto-populate commission rate if not provided
    if params.commission_rate == 0.0 || params.fee_rate == 0.0 {
        let commission = get_exchange_commission_rate(&params.exchange);
        params.commission_rate = commission;
        params.fee_rate = commission;
        // log_info!(...)
    }
    
    // Auto-populate date range if not provided
    if params.start_date.is_none() || params.end_date.is_none() {
        match get_date_range_for_symbol(&state.job_queue, &params.symbol, &params.exchange).await {
            Ok((start, end)) => {
                params.start_date = Some(start.clone());
                params.end_date = Some(end.clone());
            },
            Err(_e) => {
                // Date range lookup failed - will use defaults
            }
        }
    }
    
    // ========================================================================
    // AUTO DATA PREPARATION - Ensure binary data exists before queueing job
    // ========================================================================
    // This prevents timeout issues with distributed workers trying to prepare
    // data on-the-fly. The API pod prepares data once, and all workers can use it.
    #[cfg(feature = "postgres")]
    {
        if let Err(e) = ensure_binary_data_for_job(
            &state.job_queue, 
            &params.symbol, 
            &params.exchange,
            params.start_date.as_deref(),
            params.end_date.as_deref(),
        ).await {
            log_warn!(crate::MAIN_LOGGER, "Data preparation warning for {}/{}: {} (job will still be queued)", 
                params.symbol, params.exchange, e);
            // Don't fail the job submission - workers can fall back to DB streaming
        }
    }
    
    let job_id = state
        .job_queue
        .submit_job(params)
        .await
        .map_err(|e| {
            // log_error!(...)
            AppError::Internal(format!("Failed to submit job: {}", e))
        })?;
    
    // log_info!(...)
    
    Ok(Json(JobSubmitResponse {
        job_id: job_id.clone(),
        status: "queued".to_string(),
        message: format!("Backtest job {} submitted successfully", job_id),
    }))
}

/// Get job status
/// GET /api/backtest/:id/status
pub async fn get_job_status(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, AppError> {
    // log_info!(...)
    
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            // log_error!(...)
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            // log_warn!(...)
            AppError::NotFound(format!("Job {} not found", job_id))
        })?;
    
    // log_info!(...)
    
    // Return generation info when total_generations > 0 (indicates a genetic optimization job)
    // This ensures UI shows "Gen 0/N" during initialization instead of "Gen null/null"
    let has_generation_info = job.total_generations > 0;
    
    Ok(Json(JobStatusResponse {
        job_id: job.id,
        status: format!("{:?}", job.status).to_lowercase(),
        progress: job.progress,
        created_at: job.created_at.to_rfc3339(),
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
        completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
        error_message: job.error_message,
        current_generation: if has_generation_info { Some(job.current_generation) } else { None },
        total_generations: if has_generation_info { Some(job.total_generations) } else { None },
        current_phase: job.current_phase,
        phase_details: job.phase_details,
    }))
}

/// Get job details (parameters and configuration)
/// GET /api/backtest/:id
pub async fn get_job_details(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobDetailsResponse>, AppError> {
    // log_info!(...)
    
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            // log_error!(...)
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            // log_warn!(...)
            AppError::NotFound(format!("Job {} not found", job_id))
        })?;
    
    // log_info!(...)
    
    Ok(Json(JobDetailsResponse {
        job_id: job.id,
        symbol: job.params.symbol.clone(),
        exchange: job.params.exchange.clone(),
        strategy: job.params.strategy_type.clone(),
        initial_capital: job.params.initial_capital,
        optimization_method: job.params.optimization_method.clone(),
        population_size: Some(job.params.population_size as u32),
        generations: Some(job.params.generations as u32),
        created_at: job.created_at.to_rfc3339(),
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
        // All configuration parameters
        initial_inventory_pct: job.params.initial_inventory_pct,
        initial_30d_volume: job.params.initial_30d_volume,
        monte_carlo_enabled: job.params.monte_carlo_enabled,
        monte_carlo_runs: job.params.monte_carlo_runs,
        walk_forward_enabled: job.params.walk_forward_enabled,
        train_ratio: job.params.train_ratio,
        test_ratio: job.params.test_ratio,
        validate_ratio: job.params.validate_ratio,
        auto_save_profitable: job.params.auto_save_profitable,
        min_sharpe_ratio: job.params.min_sharpe_ratio,
        min_profit_factor: job.params.min_profit_factor,
        max_drawdown: job.params.max_drawdown,
        min_win_rate: job.params.min_win_rate,
        min_trades: job.params.min_trades,
        min_sortino_ratio: job.params.min_sortino_ratio,
        min_calmar_ratio: job.params.min_calmar_ratio,
        min_total_pnl: job.params.min_total_pnl,
        fitness_weights: job.params.fitness_weights.clone(),
        start_date: job.params.start_date.clone(),
        end_date: job.params.end_date.clone(),
        commission_rate: job.params.commission_rate,
        // Manual strategy parameters
        risk_aversion: job.params.risk_aversion,
        inventory_target: job.params.inventory_target,
        order_size: job.params.order_size,
        order_size_pct: job.params.order_size_pct,
        window_ms: job.params.window_ms,
        min_quote_lifetime_ms: job.params.min_quote_lifetime_ms,
        volatility_cap: job.params.volatility_cap,
        time_horizon_minutes: job.params.time_horizon_minutes,
        market_depth_k: job.params.market_depth_k,
        volatility_adjustment_factor: job.params.volatility_adjustment_factor,
        inventory_penalty_factor: job.params.inventory_penalty_factor,
        trend_sensitivity: job.params.trend_sensitivity,
        flow_sensitivity: job.params.flow_sensitivity,
        take_profit_pct: job.params.take_profit_pct,
        stop_loss_pct: job.params.stop_loss_pct,
        min_spread_bps: job.params.min_spread_bps,
        max_trades_per_minute: job.params.max_trades_per_minute,
    }))
}

/// Get job results
/// GET /api/backtest/:id/results
pub async fn get_job_results(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<BacktestResult>, AppError> {
    // log_info!(...)
    
    // Check if job is completed
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            // log_error!(...)
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            // log_warn!(...)
            AppError::NotFound(format!("Job {} not found", job_id))
        })?;
    
    // Return helpful status-specific responses
    match job.status {
        JobStatus::Completed => {
            // Fetch the results
            let result = state
                .job_queue
                .get_result(&job_id)
                .await
                .map_err(|e| {
                    // log_error!(...)
                    AppError::Internal(format!("Failed to fetch results: {}", e))
                })?
                .ok_or_else(|| {
                    // log_warn!(...)
                    AppError::NotFound(format!("Results for job {} not found", job_id))
                })?;
            
            // log_info!(...)
            
            return Ok(Json(result));
        },
        JobStatus::Running => {
            // log_info!(...)
            return Err(AppError::StillRunning { 
                job_id: job_id.clone(), 
                progress: job.progress,
                started_at: job.started_at.map(|dt| dt.to_rfc3339()),
            });
        },
        JobStatus::Failed => {
            // log_warn!(...)
            return Err(AppError::JobFailed {
                job_id: job_id.clone(),
                error: job.error_message.unwrap_or_else(|| "Unknown error".to_string()),
            });
        },
        JobStatus::Queued => {
            // log_info!(...)
            return Err(AppError::NotStarted {
                job_id: job_id.clone(),
                created_at: job.created_at.to_rfc3339(),
            });
        },
        JobStatus::Cancelled => {
            return Err(AppError::JobFailed {
                job_id: job_id.clone(),
                error: "Job was cancelled".to_string(),
            });
        },
    }
    
    // This is unreachable but needed for type checking
    #[allow(unreachable_code)]
    {
        Err(AppError::Internal("Unexpected job status".to_string()))
    }
}

/// Health check endpoint
/// GET /health
pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "backtesting-engine",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Kubernetes readiness probe
/// GET /health/ready
/// Returns 200 when the service is ready to accept traffic
pub async fn readiness_check(State(state): State<ApiState>) -> impl IntoResponse {
    // Check if job queue is accessible
    match state.job_queue.queue_length().await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "status": "ready",
                "checks": {
                    "job_queue": "ok"
                }
            }))
        ),
        Err(_) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "not_ready",
                "checks": {
                    "job_queue": "unavailable"
                }
            }))
        ),
    }
}

/// Kubernetes liveness probe
/// GET /health/live
/// Returns 200 if the service is alive (not deadlocked)
pub async fn liveness_check() -> impl IntoResponse {
    // Simple liveness - if we can respond, we're alive
    Json(serde_json::json!({
        "status": "alive",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Queue stats endpoint
/// GET /api/stats
pub async fn queue_stats(State(state): State<ApiState>) -> Result<impl IntoResponse, AppError> {
    // log_info!(...)
    
    let queue_length = state
        .job_queue
        .queue_length()
        .await
        .map_err(|e| {
            // log_error!(...)
            AppError::Internal(format!("Failed to get queue length: {}", e))
        })?;
    
    // log_info!(...)
    
    Ok(Json(serde_json::json!({
        "queue_length": queue_length,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}

/// Application error types
#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
    StillRunning { job_id: String, progress: f64, started_at: Option<String> },
    JobFailed { job_id: String, error: String },
    NotStarted { job_id: String, created_at: String },
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound(msg) => {
                (StatusCode::NOT_FOUND, Json(ErrorResponse { error: msg })).into_response()
            },
            AppError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })).into_response()
            },
            AppError::Internal(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: msg })).into_response()
            },
            AppError::StillRunning { job_id, progress, started_at } => {
                (StatusCode::ACCEPTED, Json(serde_json::json!({
                    "status": "running",
                    "job_id": job_id,
                    "progress": progress,
                    "started_at": started_at,
                    "message": format!("Job is still running ({:.1}% complete). Check status at /api/backtest/{}/status", progress, job_id),
                }))).into_response()
            },
            AppError::JobFailed { job_id, error } => {
                (StatusCode::OK, Json(serde_json::json!({
                    "status": "failed",
                    "job_id": job_id,
                    "error": error,
                    "message": "Job execution failed. Check error details above.",
                }))).into_response()
            },
            AppError::NotStarted { job_id, created_at } => {
                (StatusCode::ACCEPTED, Json(serde_json::json!({
                    "status": "queued",
                    "job_id": job_id,
                    "created_at": created_at,
                    "message": format!("Job is queued but not started yet. Check status at /api/backtest/{}/status", job_id),
                }))).into_response()
            },
        }
    }
}

/// Cancel a running backtest job
/// POST /api/backtest/:id/cancel
pub async fn cancel_backtest(
    State(_state): State<ApiState>,
    Path(_job_id_str): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    #[cfg(feature = "postgres")]
    {
        use diesel::prelude::*;
        use diesel_async::{AsyncPgConnection, AsyncConnection};
        use diesel_async::RunQueryDsl;
        
        let job_id_str = &_job_id_str;
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::Internal("DATABASE_URL not set".to_string()))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| AppError::Internal(format!("Database connection failed: {}", e)))?;
        
        // First check if job exists and its status
        use databaseschema::schema::backtest_jobs::dsl::*;
        let job: Option<(String, String)> = backtest_jobs
            .filter(job_id.eq(job_id_str))
            .select((job_id, status))
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| AppError::Internal(format!("Failed to query job: {}", e)))?;
        
        match job {
            None => {
                // log_warn!(...)
                return Err(AppError::NotFound(format!("Job {} not found", job_id_str)));
            }
            Some((_, job_status)) => {
                // Only allow cancellation of running or queued jobs
                if !is_cancellable_status(&job_status) {
                    // log_warn!(...)
                    return Err(AppError::BadRequest(format!(
                        "Job cannot be cancelled. Current status: {}. Only running or queued jobs can be cancelled.",
                        job_status
                    )));
                }
                
                // Cancel the job by marking as cancelled
                diesel::update(backtest_jobs)
                    .filter(job_id.eq(&job_id_str))
                    .set((
                        status.eq("cancelled"),
                        error_message.eq(Some("Cancelled by user")),
                        completed_at.eq(diesel::dsl::now),
                    ))
                    .execute(&mut conn)
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to cancel job: {}", e)))?;
                
                // log_info!(...)
                
                return Ok(Json(serde_json::json!({
                    "message": "Job cancelled successfully",
                    "job_id": job_id_str,
                    "status": "cancelled",
                })));
            }
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    Err(AppError::Internal("Cancel not supported for this backend".to_string()))
}

/// Delete a completed backtest job and its results
/// DELETE /api/backtest/:id
pub async fn delete_backtest(
    State(_state): State<ApiState>,
    Path(_job_id_str): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    
    #[cfg(feature = "postgres")]
    {
        use diesel::prelude::*;
        use diesel_async::{AsyncPgConnection, AsyncConnection};
        use diesel_async::RunQueryDsl;
        
        let job_id_str = &_job_id_str;
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::Internal("DATABASE_URL not set".to_string()))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| AppError::Internal(format!("Database connection failed: {}", e)))?;
        
        // First check if job exists and its status
        use databaseschema::schema::backtest_jobs::dsl::*;
        use databaseschema::schema::backtest_results;
        
        let job: Option<(String, String, Option<uuid::Uuid>)> = backtest_jobs
            .filter(job_id.eq(job_id_str))
            .select((job_id, status, result_id))
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| AppError::Internal(format!("Failed to query job: {}", e)))?;
        
        match job {
            None => {
                // log_warn!(...)
                return Err(AppError::NotFound(format!("Job {} not found", job_id_str)));
            }
            Some((_, job_status, job_result_id)) => {
                // Only allow deletion of completed, failed, or cancelled jobs
                if !is_deletable_status(&job_status) {
                    // log_warn!(...)
                    return Err(AppError::BadRequest(format!(
                        "Job cannot be deleted while {}. Please cancel the job first or wait for it to complete.",
                        job_status
                    )));
                }
                
                // Delete the results first (if they exist) - this cascades to related tables
                if let Some(result_uuid) = job_result_id {
                    diesel::delete(backtest_results::table.filter(backtest_results::id.eq(result_uuid)))
                        .execute(&mut conn)
                        .await
                        .map_err(|e| AppError::Internal(format!("Failed to delete backtest results: {}", e)))?;
                    
                    // log_info!(...)
                }
                
                // Delete the job itself
                diesel::delete(backtest_jobs.filter(job_id.eq(&job_id_str)))
                    .execute(&mut conn)
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to delete job: {}", e)))?;
                
                // log_info!(...)
                
                return Ok(Json(serde_json::json!({
                    "message": "Job and all associated data deleted successfully",
                    "job_id": job_id_str,
                })));
            }
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    Err(AppError::Internal("Delete not supported for this backend".to_string()))
}

/// Check if a job status allows cancellation
#[cfg(feature = "postgres")]
fn is_cancellable_status(status: &str) -> bool {
    matches!(status, "running" | "queued" | "pending")
}

/// Check if a job status allows deletion
#[cfg(feature = "postgres")]
fn is_deletable_status(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

/// Cleanup old completed backtest jobs
/// POST /api/backtest/cleanup
/// Query parameters: days_old (default: 30)
pub async fn cleanup_old_backtests(
    State(_state): State<ApiState>,
) -> Result<Json<serde_json::Value>, AppError> {
    // log_info!(...)
    
    #[cfg(feature = "postgres")]
    {
        use diesel::prelude::*;
        use diesel_async::{AsyncPgConnection, AsyncConnection};
        use diesel_async::RunQueryDsl;
        use chrono::{Utc, Duration};
        
        // Default: cleanup jobs older than 30 days
        let retention_days = 30;
        let cutoff_date = Utc::now() - Duration::days(retention_days);
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::Internal("DATABASE_URL not set".to_string()))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| AppError::Internal(format!("Database connection failed: {}", e)))?;
        
        use databaseschema::schema::backtest_jobs::dsl::*;
        use databaseschema::schema::backtest_results;
        
        // Find old completed/failed/cancelled jobs
        let old_jobs: Vec<(uuid::Uuid, String, Option<uuid::Uuid>)> = backtest_jobs
            .filter(completed_at.lt(cutoff_date.naive_utc()))
            .filter(status.eq_any(vec!["completed", "failed", "cancelled"]))
            .select((id, job_id, result_id))
            .load(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to query old jobs: {}", e)))?;
        
        let job_count = old_jobs.len();
        
        if job_count == 0 {
            // log_info!(...)
            return Ok(Json(serde_json::json!({
                "message": "No old backtests found to cleanup",
                "deleted_count": 0,
                "retention_days": retention_days,
            })));
        }
        
        // Delete results first (cascades to related tables)
        let result_ids: Vec<uuid::Uuid> = old_jobs.iter()
            .filter_map(|(_, _, rid)| *rid)
            .collect();
        
        if !result_ids.is_empty() {
            let _result_count = diesel::delete(
                backtest_results::table.filter(backtest_results::id.eq_any(&result_ids))
            )
            .execute(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete results: {}", e)))?;
            
            // log_info!(...)
        }
        
        // Delete the jobs
        let job_uuids: Vec<uuid::Uuid> = old_jobs.iter().map(|(jid, _, _)| *jid).collect();
        let deleted_count = diesel::delete(backtest_jobs.filter(id.eq_any(&job_uuids)))
            .execute(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete jobs: {}", e)))?;
        
        // log_info!(...)
        
        return Ok(Json(serde_json::json!({
            "message": format!("Successfully cleaned up old backtests"),
            "deleted_count": deleted_count,
            "retention_days": retention_days,
            "cutoff_date": cutoff_date.to_rfc3339(),
        })));
    }
    
    #[cfg(not(feature = "postgres"))]
    Err(AppError::Internal("Cleanup not supported for this backend".to_string()))
}

/// Get commission rate for a given exchange
fn get_exchange_commission_rate(exchange: &str) -> f64 {
    match exchange.to_lowercase().as_str() {
        "kraken" => 0.0016,      // 0.16% maker/taker
        "coinbase" => 0.005,     // 0.5% retail
        "binance" => 0.001,      // 0.1% with BNB
        "ftx" => 0.0007,         // 0.07% maker (RIP)
        "bybit" => 0.001,        // 0.1% maker
        "okx" => 0.0008,         // 0.08% maker
        "huobi" => 0.002,        // 0.2% maker
        "bitfinex" => 0.001,     // 0.1% maker
        "gemini" => 0.0025,      // 0.25% maker
        "bitstamp" => 0.005,     // 0.5% retail
        _ => 0.001,              // Default 0.1%
    }
}

/// Get date range for a symbol/exchange pair from the database
async fn get_date_range_for_symbol(
    _job_queue: &Arc<dyn JobQueue>,
    #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
    symbol: &str,
    #[cfg_attr(not(feature = "postgres"), allow(unused_variables))]
    exchange: &str,
) -> Result<(String, String), String> {
    #[cfg(feature = "postgres")]
    {
        use diesel_async::{AsyncPgConnection, AsyncConnection, RunQueryDsl};
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|e| format!("DATABASE_URL not set: {}", e))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| format!("Database connection failed: {}", e))?;
        
        // Define a struct for the query result
        #[derive(diesel::QueryableByName)]
        struct DateRange {
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamp>)]
            start_date: Option<chrono::NaiveDateTime>,
            #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Timestamp>)]
            end_date: Option<chrono::NaiveDateTime>,
        }
        
        // Query for min/max timestamp for this symbol+exchange
        let query = format!(
            "SELECT MIN(timestamp) as start_date, MAX(timestamp) as end_date \
             FROM orderbook_snapshots \
             WHERE symbol = '{}' AND exchange = '{}'",
            symbol, exchange
        );
        
        let result: Result<DateRange, _> = diesel::sql_query(query)
            .get_result(&mut conn)
            .await;
        
        match result {
            Ok(date_range) => {
                if let (Some(start), Some(end)) = (date_range.start_date, date_range.end_date) {
                    Ok((start.format("%Y-%m-%dT%H:%M:%SZ").to_string(), 
                        end.format("%Y-%m-%dT%H:%M:%SZ").to_string()))
                } else {
                    Err(format!("No data found for {}/{}", symbol, exchange))
                }
            },
            Err(e) => Err(format!("Database query failed: {}", e)),
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    Err("Date range auto-detection not supported for this backend".to_string())
}

// ============================================================================
// DATA PREPARATION API
// ============================================================================

/// Request to prepare binary data files
#[derive(Debug, serde::Deserialize)]
pub struct PrepareDataRequest {
    /// Symbol to prepare (e.g., "BTC/USD"). If None, prepares all available symbols.
    pub symbol: Option<String>,
    /// Exchange to prepare (e.g., "Kraken"). If None, prepares all available exchanges.
    pub exchange: Option<String>,
    /// Force rebuild even if binary file exists
    #[serde(default)]
    pub force: bool,
}

/// Response from data preparation
#[derive(Debug, Serialize)]
pub struct PrepareDataResponse {
    pub success: bool,
    pub message: String,
    pub prepared: Vec<PreparedFile>,
}

/// Info about a prepared file
#[derive(Debug, Serialize)]
pub struct PreparedFile {
    pub symbol: String,
    pub exchange: String,
    pub ticks: u64,
    pub size_mb: u64,
    pub elapsed_secs: f64,
}

/// Prepare binary data files for backtesting
/// 
/// POST /api/data/prepare
/// 
/// This endpoint is designed to be called by a nightly cron job to ensure
/// binary data files are ready for the next day's backtests.
/// 
/// Request body:
/// ```json
/// {
///   "symbol": "BTC/USD",      // Optional: specific symbol or all
///   "exchange": "Kraken",     // Optional: specific exchange or all
///   "force": false            // Optional: rebuild even if exists
/// }
/// ```
#[cfg(feature = "postgres")]
pub async fn prepare_data(
    State(state): State<ApiState>,
    Json(request): Json<PrepareDataRequest>,
) -> Result<Json<PrepareDataResponse>, AppError> {
    use data_prep::{DataPrepConfig, BinaryFileReader};
    use std::path::PathBuf;
    
    let pool = state.job_queue.get_pool()
        .ok_or_else(|| AppError::Internal("Database pool not available".to_string()))?;
    
    let data_dir = std::env::var("DATA_DIR")
        .unwrap_or_else(|_| "/data/historical".to_string());
    let data_path = PathBuf::from(&data_dir);
    
    // Create output directory
    std::fs::create_dir_all(&data_path)
        .map_err(|e| AppError::Internal(format!("Failed to create data dir: {}", e)))?;
    
    // Get list of symbol/exchange pairs to prepare
    let pairs = get_symbol_exchange_pairs(&state.job_queue, request.symbol.as_deref(), request.exchange.as_deref()).await?;
    
    let mut prepared = Vec::new();
    
    for (symbol, exchange) in pairs {
        let filename = format!("{}_{}.bin", symbol.replace('/', "_"), &exchange);
        let binary_path = data_path.join(&filename);
        
        // Check if file exists and is valid (skip if not forcing rebuild)
        if !request.force && binary_path.exists() {
            if let Ok(reader) = BinaryFileReader::open(&binary_path) {
                if reader.tick_count() > 0 {
                    // File exists and is valid, skip
                    continue;
                }
            }
        }
        
        // Prepare data
        let config = DataPrepConfig {
            symbol: symbol.clone(),
            exchange: exchange.clone(),
            output_dir: data_path.clone(),
            chunk_size: 10_000,
            compute_features: false,
            start_time: None,
            end_time: None,
        };
        
        let prep_start = std::time::Instant::now();
        match data_prep::prepare_data(&pool, &config).await {
            Ok(result) => {
                prepared.push(PreparedFile {
                    symbol,
                    exchange,
                    ticks: result.total_ticks,
                    size_mb: result.file_size_bytes / 1_000_000,
                    elapsed_secs: prep_start.elapsed().as_secs_f64(),
                });
            }
            Err(e) => {
                return Err(AppError::Internal(format!(
                    "Failed to prepare {}/{}: {}", symbol, exchange, e
                )));
            }
        }
    }
    
    let message = if prepared.is_empty() {
        "All binary files are up to date".to_string()
    } else {
        format!("Prepared {} binary file(s)", prepared.len())
    };
    
    Ok(Json(PrepareDataResponse {
        success: true,
        message,
        prepared,
    }))
}

/// Get list of symbol/exchange pairs from database
#[cfg(feature = "postgres")]
async fn get_symbol_exchange_pairs(
    job_queue: &Arc<dyn crate::job_queue::JobQueue>,
    symbol_filter: Option<&str>,
    exchange_filter: Option<&str>,
) -> Result<Vec<(String, String)>, AppError> {
    use diesel::prelude::*;
    use diesel::sql_types::Varchar;
    use diesel_async::RunQueryDsl;
    
    let pool = job_queue.get_pool()
        .ok_or_else(|| AppError::Internal("Database pool not available".to_string()))?;
    
    let mut conn = pool.get().await
        .map_err(|e| AppError::Internal(format!("Failed to get connection: {}", e)))?;
    
    #[derive(QueryableByName)]
    struct SymbolExchange {
        #[diesel(sql_type = Varchar)]
        symbol: String,
        #[diesel(sql_type = Varchar)]
        exchange: String,
    }
    
    // Use parameterized queries to prevent SQL injection
    // Diesel's bind() method safely escapes values
    use diesel::sql_types::Text;
    
    let results: Vec<SymbolExchange> = match (symbol_filter, exchange_filter) {
        (Some(sym), Some(ex)) => {
            diesel::sql_query(
                "SELECT DISTINCT symbol, exchange FROM historical_orders WHERE symbol = $1 AND exchange = $2"
            )
            .bind::<Text, _>(sym)
            .bind::<Text, _>(ex)
            .get_results(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to query pairs: {}", e)))?
        }
        (Some(sym), None) => {
            diesel::sql_query(
                "SELECT DISTINCT symbol, exchange FROM historical_orders WHERE symbol = $1"
            )
            .bind::<Text, _>(sym)
            .get_results(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to query pairs: {}", e)))?
        }
        (None, Some(ex)) => {
            diesel::sql_query(
                "SELECT DISTINCT symbol, exchange FROM historical_orders WHERE exchange = $1"
            )
            .bind::<Text, _>(ex)
            .get_results(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to query pairs: {}", e)))?
        }
        (None, None) => {
            diesel::sql_query(
                "SELECT DISTINCT symbol, exchange FROM historical_orders"
            )
            .get_results(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to query pairs: {}", e)))?
        }
    };
    
    Ok(results.into_iter().map(|r| (r.symbol, r.exchange)).collect())
}

#[cfg(not(feature = "postgres"))]
pub async fn prepare_data(
    State(_state): State<ApiState>,
    Json(_request): Json<PrepareDataRequest>,
) -> Result<Json<PrepareDataResponse>, AppError> {
    Err(AppError::Internal("Data preparation requires PostgreSQL feature".to_string()))
}

// ============================================================================
// Helper function for auto data preparation at job submission time
// ============================================================================

/// Ensures binary data file exists and includes the latest data from DB.
/// 
/// As data grows perpetually, this function:
/// 1. Checks if binary file exists and is valid
/// 2. Queries DB for the latest timestamp for this symbol/exchange
/// 3. Compares file's end timestamp with DB's latest - regenerates if stale
/// 4. Also checks if job needs older data than file contains
/// 
/// This ensures backtests always use the most up-to-date data.
#[cfg(feature = "postgres")]
async fn ensure_binary_data_for_job(
    job_queue: &std::sync::Arc<dyn JobQueue>,
    symbol: &str,
    exchange: &str,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use data_prep::DataPrepConfig;
    use std::path::PathBuf;
    use chrono::{NaiveDate, TimeZone, Utc};
    
    // Parse job date range (if provided)
    let job_start = start_date.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let job_end = end_date.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    
    // Get data directory from environment or use default
    let data_dir = std::env::var("DATA_DIR")
        .unwrap_or_else(|_| "/data/historical".to_string());
    let data_path = PathBuf::from(&data_dir);
    
    // Get database pool early - we'll need it for freshness check
    let pool = job_queue.get_pool()
        .ok_or("Database pool not available for data preparation")?;
    
    // Query DB for latest timestamp to check freshness
    let db_latest_ms = get_db_latest_timestamp(&pool, symbol, exchange).await?;
    
    // Check if binary file already exists (unified format with all event types)
    let filename = format!("{}_{}.bin", symbol.replace('/', "_"), exchange);
    let binary_path = data_path.join(&filename);
    
    // Check binary file using OrderBookBinaryReader (unified L3 format)
    if binary_path.exists() {
        match data_prep::OrderBookBinaryReader::open(&binary_path) {
            Ok(reader) => {
                let event_count = reader.len();
                if event_count == 0 {
                    log_warn!(crate::MAIN_LOGGER, "API: Binary file for {}/{} has 0 events, will regenerate", 
                        symbol, exchange);
                } else {
                    let file_start_ms = reader.start_timestamp_ms();
                    let file_end_ms = reader.end_timestamp_ms();
                    
                    let mut needs_regeneration = false;
                    let mut reason = String::new();
                    
                    // Check 1: Is there newer data in DB? (allow 5 min tolerance)
                    let staleness_tolerance_ms = 5 * 60 * 1000; // 5 minutes
                    if let Some(db_latest) = db_latest_ms {
                        if db_latest > file_end_ms + staleness_tolerance_ms {
                            needs_regeneration = true;
                            let new_data_hours = (db_latest - file_end_ms) as f64 / 3_600_000.0;
                            reason = format!("DB has {:.1}h of new data (file ends {}, DB has data until {})",
                                new_data_hours,
                                chrono::DateTime::from_timestamp_millis(file_end_ms)
                                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default(),
                                chrono::DateTime::from_timestamp_millis(db_latest)
                                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default());
                        }
                    }
                    
                    // Check 2: Does job need older data than file contains?
                    if !needs_regeneration {
                        if let Some(job_start) = job_start {
                            let job_start_ms = Utc.from_utc_datetime(&job_start.and_hms_opt(0, 0, 0).unwrap())
                                .timestamp_millis();
                            if file_start_ms > job_start_ms + 86_400_000 {
                                needs_regeneration = true;
                                reason = format!("file starts {} but job needs {}", 
                                    chrono::DateTime::from_timestamp_millis(file_start_ms)
                                        .map(|d| d.format("%Y-%m-%d").to_string())
                                        .unwrap_or_default(),
                                    job_start);
                            }
                        }
                    }
                    
                    // Check 3: Does job need newer data than file contains?
                    if !needs_regeneration {
                        if let Some(job_end) = job_end {
                            let job_end_ms = Utc.from_utc_datetime(&job_end.and_hms_opt(23, 59, 59).unwrap())
                                .timestamp_millis();
                            if file_end_ms < job_end_ms - 86_400_000 {
                                needs_regeneration = true;
                                reason = format!("file ends {} but job needs {}", 
                                    chrono::DateTime::from_timestamp_millis(file_end_ms)
                                        .map(|d| d.format("%Y-%m-%d").to_string())
                                        .unwrap_or_default(),
                                    job_end);
                            }
                        }
                    }
                    
                    if !needs_regeneration {
                        log_info!(crate::MAIN_LOGGER, "API: Binary data up-to-date for {}/{}: {} ({} events, {} - {})", 
                            symbol, exchange, binary_path.display(), event_count,
                            chrono::DateTime::from_timestamp_millis(file_start_ms)
                                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_default(),
                            chrono::DateTime::from_timestamp_millis(file_end_ms)
                                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_default());
                        return Ok(());
                    }
                    
                    log_info!(crate::MAIN_LOGGER, "API: Binary file for {}/{} needs regeneration: {}", 
                        symbol, exchange, reason);
                }
            }
            Err(e) => {
                log_warn!(crate::MAIN_LOGGER, "API: Binary file for {}/{} invalid ({}), will regenerate", 
                    symbol, exchange, e);
            }
        }
    }
    
    // Binary file doesn't exist, is invalid, or is stale - prepare it
    log_info!(crate::MAIN_LOGGER, "API: Preparing binary data for {}/{} -> {}", 
        symbol, exchange, binary_path.display());
    
    // Create output directory
    std::fs::create_dir_all(&data_path)?;
    
    // Prepare data configuration - prepare ALL data for this symbol/exchange
    let config = DataPrepConfig {
        symbol: symbol.to_string(),
        exchange: exchange.to_string(),
        output_dir: data_path,
        chunk_size: 10_000,
        compute_features: false, // Skip feature computation for speed
        start_time: None, // Prepare all data
        end_time: None,   // Prepare all data
    };
    
    // Prepare unified binary with ALL event types (new, modify, cancel, trade)
    // This single file is used for both:
    // - Fast GA evaluations (filter to trade events at read time)
    // - Realistic final evaluation (full orderbook reconstruction)
    let prep_start = std::time::Instant::now();
    let result = data_prep::prepare_orderbook_data(&pool, &config).await?;
    let prep_elapsed = prep_start.elapsed();
    
    log_info!(crate::MAIN_LOGGER, "API: Binary data prepared for {}/{} in {:.1}s: {} events, {} MB", 
        symbol, exchange,
        prep_elapsed.as_secs_f64(),
        result.total_ticks,
        result.file_size_bytes / 1_000_000
    );
    
    Ok(())
}

/// Query the database for the latest timestamp for a symbol/exchange pair
#[cfg(feature = "postgres")]
async fn get_db_latest_timestamp(
    pool: &diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>,
    symbol: &str,
    exchange: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;
    use diesel::sql_types::{Text, Nullable, BigInt};
    
    #[derive(QueryableByName)]
    struct MaxTimestamp {
        #[diesel(sql_type = Nullable<BigInt>)]
        max_ts: Option<i64>,
    }
    
    let mut conn = pool.get().await?;
    
    let result: MaxTimestamp = diesel::sql_query(
        "SELECT EXTRACT(EPOCH FROM MAX(timestamp))::BIGINT * 1000 as max_ts \
         FROM historical_orders \
         WHERE symbol = $1 AND exchange = $2"
    )
    .bind::<Text, _>(symbol)
    .bind::<Text, _>(exchange)
    .get_result(&mut conn)
    .await?;
    
    Ok(result.max_ts)
}