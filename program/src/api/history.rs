//! Historical backtest results API

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::job_queue::PostgresQueue;
// use crate::{log_info, log_error};
// use crate::logging_facade::MAIN_LOGGER;

/// Query parameters for historical backtests
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub status: Option<String>, // "queued", "running", "completed", "failed"
    
    #[serde(default)]
    pub symbol: Option<String>,
    
    #[serde(default)]
    pub exchange: Option<String>,
    
    #[serde(default)]
    pub start_date: Option<String>, // RFC3339 format
    
    #[serde(default)]
    pub end_date: Option<String>, // RFC3339 format
    
    #[serde(default = "default_limit")]
    pub limit: i64,
    
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Historical backtest summary
#[derive(Debug, Serialize)]
pub struct BacktestSummary {
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub status: String,
    pub progress: f64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    
    // Phase tracking for better visibility
    pub current_phase: String,
    pub phase_details: serde_json::Value,
    
    // Generation progress (for GA jobs)
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
    
    // Error message (for failed jobs)
    pub error_message: Option<String>,
    
    // Results (if completed)
    pub total_pnl: Option<f64>,
    pub sharpe_ratio: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub win_rate: Option<f64>,
    pub total_trades: Option<usize>,
}

/// Historical backtests response
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub backtests: Vec<BacktestSummary>,
    pub total: usize,
    pub limit: i64,
    pub offset: i64,
}

/// Get historical backtests
/// GET /api/backtests?status=completed&symbol=XBTUSD&limit=50&offset=0
pub async fn get_backtests(
    State(state): State<super::handlers::ApiState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, super::handlers::AppError> {
    // log_info!(...)
    
    // This endpoint only works with PostgreSQL backend
    // Try to downcast to PostgresQueue
    let postgres_queue = state.job_queue
        .as_any()
        .downcast_ref::<PostgresQueue>()
        .ok_or_else(|| super::handlers::AppError::BadRequest(
            "Historical results API requires PostgreSQL backend".to_string()
        ))?;
    
    // Query historical results
    let (backtests, total) = postgres_queue
        .get_historical_backtests(&query)
        .await
        .map_err(|e| {
            // log_error!(...)
            super::handlers::AppError::Internal(e.to_string())
        })?;
    
    // log_info!(...)
    
    Ok(Json(HistoryResponse {
        backtests,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}


