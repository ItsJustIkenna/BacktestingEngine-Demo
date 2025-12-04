//! API endpoints for fetching metadata (symbols, exchanges, etc.)

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use super::handlers::ApiState;

#[cfg(feature = "postgres")]
use diesel::prelude::*;
#[cfg(feature = "postgres")]
use databaseschema::schema::{securities, exchanges};

#[derive(Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub symbol: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Exchange {
    pub exchange: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataResponse<T> {
    pub data: Vec<T>,
}

#[cfg(feature = "postgres")]
#[derive(Queryable, Selectable)]
#[diesel(table_name = securities)]
struct SecurityRow {
    symbol: String,
}

#[cfg(feature = "postgres")]
#[derive(Queryable, Selectable)]
#[diesel(table_name = exchanges)]
struct ExchangeRow {
    exchange: String,
}

/// GET /api/symbols - Get all available symbols from database
#[cfg(feature = "postgres")]
pub async fn get_symbols(
    State(state): State<ApiState>,
) -> Result<Json<MetadataResponse<Symbol>>, (StatusCode, String)> {
    // Get database connection from job queue
    let pool = match state.job_queue.get_pool() {
        Some(pool) => pool,
        None => return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database connection not available".to_string(),
        )),
    };

    // Query symbols from database
    let mut conn = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get database connection: {}", e))
    })?;

    let symbols: Vec<Symbol> = diesel_async::RunQueryDsl::load(
        securities::table
            .select(securities::symbol)
            .order(securities::symbol.asc()),
        &mut conn
    )
    .await
    .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to query symbols: {}", e))
        })?
        .into_iter()
        .map(|symbol| Symbol { symbol })
        .collect();

    Ok(Json(MetadataResponse { data: symbols }))
}

/// GET /api/exchanges - Get all available exchanges from database
#[cfg(feature = "postgres")]
pub async fn get_exchanges(
    State(state): State<ApiState>,
) -> Result<Json<MetadataResponse<Exchange>>, (StatusCode, String)> {
    // Get database connection from job queue
    let pool = match state.job_queue.get_pool() {
        Some(pool) => pool,
        None => return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database connection not available".to_string(),
        )),
    };

    // Query exchanges from database
    let mut conn = pool.get().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to get database connection: {}", e))
    })?;

    let exchanges: Vec<Exchange> = diesel_async::RunQueryDsl::load(
        exchanges::table
            .select(exchanges::exchange)
            .order(exchanges::exchange.asc()),
        &mut conn
    )
    .await
    .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to query exchanges: {}", e))
        })?
        .into_iter()
        .map(|exchange| Exchange { exchange })
        .collect();

    Ok(Json(MetadataResponse { data: exchanges }))
}

/// GET /api/symbols - Stub implementation when postgres feature is disabled
#[cfg(not(feature = "postgres"))]
pub async fn get_symbols(
    State(_state): State<ApiState>,
) -> Result<Json<MetadataResponse<Symbol>>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Database features not enabled. Build with --features postgres".to_string(),
    ))
}

/// GET /api/exchanges - Stub implementation when postgres feature is disabled
#[cfg(not(feature = "postgres"))]
pub async fn get_exchanges(
    State(_state): State<ApiState>,
) -> Result<Json<MetadataResponse<Exchange>>, (StatusCode, String)> {
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Database features not enabled. Build with --features postgres".to_string(),
    ))
}


