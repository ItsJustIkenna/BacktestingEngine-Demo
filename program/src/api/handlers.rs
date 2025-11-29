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
use crate::{log_info, log_error, log_warn};
use crate::logging_facade::MAIN_LOGGER;

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
}

/// Submit a new backtest job
/// POST /api/backtest
pub async fn submit_backtest(
    State(state): State<ApiState>,
    Json(mut params): Json<BacktestJobParams>,
) -> Result<Json<JobSubmitResponse>, AppError> {
    log_info!(MAIN_LOGGER, "📥 Received backtest request: symbol={}, exchange={}, capital=${:.2}, method={}", 
        params.symbol, params.exchange, params.initial_capital, params.optimization_method);
    
    // Auto-populate commission rate if not provided
    if params.commission_rate == 0.0 || params.fee_rate == 0.0 {
        let commission = get_exchange_commission_rate(&params.exchange);
        params.commission_rate = commission;
        params.fee_rate = commission;
        log_info!(MAIN_LOGGER, "📊 Auto-populated commission rate for {}: {:.4}%", params.exchange, commission * 100.0);
    }
    
    // Auto-populate date range if not provided
    if params.start_date.is_none() || params.end_date.is_none() {
        match get_date_range_for_symbol(&state.job_queue, &params.symbol, &params.exchange).await {
            Ok((start, end)) => {
                params.start_date = Some(start.clone());
                params.end_date = Some(end.clone());
                log_info!(MAIN_LOGGER, "📅 Auto-populated date range: {} to {}", start, end);
            },
            Err(e) => {
                log_warn!(MAIN_LOGGER, "⚠️ Could not auto-populate date range: {}", e);
            }
        }
    }
    
    let job_id = state
        .job_queue
        .submit_job(params)
        .await
        .map_err(|e| {
            log_error!(MAIN_LOGGER, "❌ Failed to submit job: {}", e);
            AppError::Internal(format!("Failed to submit job: {}", e))
        })?;
    
    log_info!(MAIN_LOGGER, "✅ Job submitted successfully: job_id={}", job_id);
    
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
    log_info!(MAIN_LOGGER, "🔍 Status check for job: {}", job_id);
    
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            log_error!(MAIN_LOGGER, "❌ Failed to fetch job {}: {}", job_id, e);
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            log_warn!(MAIN_LOGGER, "⚠️ Job {} not found", job_id);
            AppError::NotFound(format!("Job {} not found", job_id))
        })?;
    
    log_info!(MAIN_LOGGER, "✅ Status retrieved for job {}: status={:?}, progress={:.1}%", 
        job_id, job.status, job.progress);
    
    Ok(Json(JobStatusResponse {
        job_id: job.id,
        status: format!("{:?}", job.status).to_lowercase(),
        progress: job.progress,
        created_at: job.created_at.to_rfc3339(),
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
        completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
        error_message: job.error_message,
    }))
}

/// Get job details (parameters and configuration)
/// GET /api/backtest/:id
pub async fn get_job_details(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobDetailsResponse>, AppError> {
    log_info!(MAIN_LOGGER, "🔍 Details requested for job: {}", job_id);
    
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            log_error!(MAIN_LOGGER, "❌ Failed to fetch job {}: {}", job_id, e);
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            log_warn!(MAIN_LOGGER, "⚠️ Job {} not found", job_id);
            AppError::NotFound(format!("Job {} not found", job_id))
        })?;
    
    log_info!(MAIN_LOGGER, "✅ Details retrieved for job {}", job_id);
    
    Ok(Json(JobDetailsResponse {
        job_id: job.id,
        symbol: job.params.symbol,
        exchange: job.params.exchange,
        strategy: job.params.strategy_type,
        initial_capital: job.params.initial_capital,
        optimization_method: job.params.optimization_method,
        population_size: Some(job.params.population_size as u32),
        generations: Some(job.params.generations as u32),
        created_at: job.created_at.to_rfc3339(),
        started_at: job.started_at.map(|dt| dt.to_rfc3339()),
    }))
}

/// Get job results
/// GET /api/backtest/:id/results
pub async fn get_job_results(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<BacktestResult>, AppError> {
    log_info!(MAIN_LOGGER, "📊 Results requested for job: {}", job_id);
    
    // Check if job is completed
    let job = state
        .job_queue
        .get_job(&job_id)
        .await
        .map_err(|e| {
            log_error!(MAIN_LOGGER, "❌ Failed to fetch job {}: {}", job_id, e);
            AppError::Internal(format!("Failed to fetch job: {}", e))
        })?
        .ok_or_else(|| {
            log_warn!(MAIN_LOGGER, "⚠️ Job {} not found", job_id);
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
                    log_error!(MAIN_LOGGER, "❌ Failed to fetch results for job {}: {}", job_id, e);
                    AppError::Internal(format!("Failed to fetch results: {}", e))
                })?
                .ok_or_else(|| {
                    log_warn!(MAIN_LOGGER, "⚠️ Results not found for completed job: {}", job_id);
                    AppError::NotFound(format!("Results for job {} not found", job_id))
                })?;
            
            log_info!(MAIN_LOGGER, "✅ Results retrieved for job {}: sharpe={:.2}, trades={}", 
                job_id, result.sharpe_ratio, result.total_trades);
            
            return Ok(Json(result));
        },
        JobStatus::Running => {
            log_info!(MAIN_LOGGER, "⏳ Job {} still running: {:.1}% complete", job_id, job.progress);
            return Err(AppError::StillRunning { 
                job_id: job_id.clone(), 
                progress: job.progress,
                started_at: job.started_at.map(|dt| dt.to_rfc3339()),
            });
        },
        JobStatus::Failed => {
            log_warn!(MAIN_LOGGER, "❌ Job {} failed: {:?}", job_id, job.error_message);
            return Err(AppError::JobFailed {
                job_id: job_id.clone(),
                error: job.error_message.unwrap_or_else(|| "Unknown error".to_string()),
            });
        },
        JobStatus::Queued => {
            log_info!(MAIN_LOGGER, "⏸️ Job {} still queued, not started yet", job_id);
            return Err(AppError::NotStarted {
                job_id: job_id.clone(),
                created_at: job.created_at.to_rfc3339(),
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

/// Queue stats endpoint
/// GET /api/stats
pub async fn queue_stats(State(state): State<ApiState>) -> Result<impl IntoResponse, AppError> {
    log_info!(MAIN_LOGGER, "📈 Queue statistics requested");
    
    let queue_length = state
        .job_queue
        .queue_length()
        .await
        .map_err(|e| {
            log_error!(MAIN_LOGGER, "❌ Failed to get queue length: {}", e);
            AppError::Internal(format!("Failed to get queue length: {}", e))
        })?;
    
    log_info!(MAIN_LOGGER, "✅ Queue stats: {} jobs pending", queue_length);
    
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
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    log_info!(MAIN_LOGGER, "Request to cancel job: {}", job_id);
    
    #[cfg(feature = "postgres")]
    {
        use diesel::prelude::*;
        use diesel_async::{AsyncPgConnection, AsyncConnection};
        use diesel_async::RunQueryDsl;
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::Internal("DATABASE_URL not set".to_string()))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| AppError::Internal(format!("Database connection failed: {}", e)))?;
        
        // First check if job exists and its status
        use databaseschema::schema::backtest_jobs::dsl::*;
        let job: Option<(String, String)> = backtest_jobs
            .filter(job_id.eq(&job_id))
            .select((job_id, status))
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| AppError::Internal(format!("Failed to query job: {}", e)))?;
        
        match job {
            None => {
                log_warn!(MAIN_LOGGER, "Job {} not found", job_id);
                return Err(AppError::NotFound(format!("Job {} not found", job_id)));
            }
            Some((_, job_status)) => {
                // Only allow cancellation of running or queued jobs
                if !is_cancellable_status(&job_status) {
                    log_warn!(MAIN_LOGGER, "Job {} cannot be cancelled (status: {})", job_id, job_status);
                    return Err(AppError::BadRequest(format!(
                        "Job cannot be cancelled. Current status: {}. Only running or queued jobs can be cancelled.",
                        job_status
                    )));
                }
                
                // Cancel the job by marking as cancelled
                diesel::update(backtest_jobs)
                    .filter(job_id.eq(&job_id))
                    .set((
                        status.eq("cancelled"),
                        error_message.eq(Some("Cancelled by user")),
                        completed_at.eq(diesel::dsl::now),
                    ))
                    .execute(&mut conn)
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to cancel job: {}", e)))?;
                
                log_info!(MAIN_LOGGER, "Job {} cancelled successfully", job_id);
                
                return Ok(Json(serde_json::json!({
                    "message": "Job cancelled successfully",
                    "job_id": job_id,
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
    Path(job_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    log_info!(MAIN_LOGGER, "Request to delete job: {}", job_id);
    
    #[cfg(feature = "postgres")]
    {
        use diesel::prelude::*;
        use diesel_async::{AsyncPgConnection, AsyncConnection};
        use diesel_async::RunQueryDsl;
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::Internal("DATABASE_URL not set".to_string()))?;
        
        let mut conn = AsyncPgConnection::establish(&database_url)
            .await
            .map_err(|e| AppError::Internal(format!("Database connection failed: {}", e)))?;
        
        // First check if job exists and its status
        use databaseschema::schema::backtest_jobs::dsl::*;
        use databaseschema::schema::backtest_results;
        
        let job: Option<(String, String, Option<uuid::Uuid>)> = backtest_jobs
            .filter(job_id.eq(&job_id))
            .select((job_id, status, result_id))
            .first(&mut conn)
            .await
            .optional()
            .map_err(|e| AppError::Internal(format!("Failed to query job: {}", e)))?;
        
        match job {
            None => {
                log_warn!(MAIN_LOGGER, "Job {} not found", job_id);
                return Err(AppError::NotFound(format!("Job {} not found", job_id)));
            }
            Some((_, job_status, job_result_id)) => {
                // Only allow deletion of completed, failed, or cancelled jobs
                if !is_deletable_status(&job_status) {
                    log_warn!(MAIN_LOGGER, "Job {} cannot be deleted (status: {})", job_id, job_status);
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
                    
                    log_info!(MAIN_LOGGER, "Deleted backtest results for job {}", job_id);
                }
                
                // Delete the job itself
                diesel::delete(backtest_jobs.filter(job_id.eq(&job_id)))
                    .execute(&mut conn)
                    .await
                    .map_err(|e| AppError::Internal(format!("Failed to delete job: {}", e)))?;
                
                log_info!(MAIN_LOGGER, "Job {} deleted successfully", job_id);
                
                return Ok(Json(serde_json::json!({
                    "message": "Job and all associated data deleted successfully",
                    "job_id": job_id,
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
    log_info!(MAIN_LOGGER, "Request to cleanup old backtests");
    
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
            log_info!(MAIN_LOGGER, "No old backtests to cleanup");
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
            let result_count = diesel::delete(
                backtest_results::table.filter(backtest_results::id.eq_any(&result_ids))
            )
            .execute(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete results: {}", e)))?;
            
            log_info!(MAIN_LOGGER, "Deleted {} backtest results", result_count);
        }
        
        // Delete the jobs
        let job_uuids: Vec<uuid::Uuid> = old_jobs.iter().map(|(jid, _, _)| *jid).collect();
        let deleted_count = diesel::delete(backtest_jobs.filter(id.eq_any(&job_uuids)))
            .execute(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete jobs: {}", e)))?;
        
        log_info!(MAIN_LOGGER, "Cleanup: deleted {} old backtests (older than {} days)", deleted_count, retention_days);
        
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
