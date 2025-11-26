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

/// Cancel/delete a backtest job
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
        
        diesel::update(databaseschema::schema::backtest_jobs::table)
            .filter(databaseschema::schema::backtest_jobs::job_id.eq(&job_id))
            .set((
                databaseschema::schema::backtest_jobs::status.eq("failed"),
                databaseschema::schema::backtest_jobs::error_message.eq(Some("Cancelled by user")),
            ))
            .execute(&mut conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to cancel job: {}", e)))?;
        
        log_info!(MAIN_LOGGER, "Job {} marked as cancelled", job_id);
        
        return Ok(Json(serde_json::json!({
            "message": "Job cancelled successfully",
            "job_id": job_id,
        })));
    }
    
    #[cfg(not(feature = "postgres"))]
    Err(AppError::Internal("Delete not supported for this backend".to_string()))
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
