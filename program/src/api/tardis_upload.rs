//! Tardis CSV file upload handling
//! 
//! Handles drag-and-drop file uploads of Tardis.dev L2 orderbook and trades data.
//! Parses metadata (symbol, exchange, date range) from the CSV files.
//! Can also import orderbook data directly into the database for backtesting.

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::sync::Arc;
use flate2::read::GzDecoder;
use tokio::sync::RwLock;
use std::collections::HashMap;
use chrono::{Utc, TimeZone};
use bigdecimal::BigDecimal;
use std::str::FromStr;

#[cfg(feature = "postgres")]
use databaseschema::{
    models::{
        exchange::NewExchange,
        security::NewSecurity,
        order_book::NewOrderBook,
        l2_orderbook_snapshot::NewL2OrderbookSnapshot,
    },
    ops::{
        exchange_ops,
        securities_ops,
        order_book_ops,
        l2_orderbook_snapshot_ops,
    },
};
#[cfg(feature = "postgres")]
use diesel_async::pooled_connection::deadpool;
#[cfg(feature = "postgres")]
use diesel_async::AsyncPgConnection;

/// Uploaded file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFileInfo {
    pub file_id: String,
    pub original_name: String,
    pub file_type: TardisFileType,
    pub symbol: String,
    pub exchange: String,
    pub start_time: String,
    pub end_time: String,
    pub row_count: usize,
    pub file_size_bytes: usize,
    pub stored_path: String,
}

/// Type of Tardis data file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TardisFileType {
    Trades,
    Orderbook,
}

impl std::fmt::Display for TardisFileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TardisFileType::Trades => write!(f, "trades"),
            TardisFileType::Orderbook => write!(f, "orderbook"),
        }
    }
}

/// Parsed symbol info from filename or CSV content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolUploadSession {
    pub session_id: String,
    pub symbol: String,
    pub exchange: String,
    pub trades_file: Option<UploadedFileInfo>,
    pub orderbook_file: Option<UploadedFileInfo>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub created_at: String,
}

/// Response for file upload
#[derive(Debug, Serialize)]
pub struct FileUploadResponse {
    pub success: bool,
    pub file: Option<UploadedFileInfo>,
    pub session: Option<SymbolUploadSession>,
    pub error: Option<String>,
}

/// Response for listing upload sessions
#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SymbolUploadSession>,
}

/// State for managing uploaded files
#[derive(Clone)]
pub struct TardisUploadState {
    /// Upload sessions by session_id
    pub sessions: Arc<RwLock<HashMap<String, SymbolUploadSession>>>,
    /// Base directory for uploaded files
    pub upload_dir: PathBuf,
}

impl TardisUploadState {
    pub fn new(upload_dir: PathBuf) -> Self {
        // Create upload directory if it doesn't exist
        if !upload_dir.exists() {
            std::fs::create_dir_all(&upload_dir).ok();
        }
        
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            upload_dir,
        }
    }
}

/// Normalize symbol format from Tardis format to database format
/// Converts: BTCUSD -> BTC/USD, SOLUSD -> SOL/USD, SUIUSD -> SUI/USD
/// Also handles: BTCUSDT -> BTC/USDT, ETHBTC -> ETH/BTC
fn normalize_symbol(symbol: &str) -> String {
    let symbol = symbol.to_uppercase();
    
    // Common quote currencies to look for (in order of preference)
    let quote_currencies = ["USDT", "USDC", "BUSD", "USD", "EUR", "GBP", "BTC", "ETH"];
    
    for quote in &quote_currencies {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            return format!("{}/{}", base, quote);
        }
    }
    
    // Fallback: if no known quote currency found, return as-is
    symbol
}

/// Normalize exchange name from Tardis format to display format
/// Converts: binance-us -> Binance US, binance -> Binance, kraken -> Kraken
fn normalize_exchange(exchange: &str) -> String {
    match exchange.to_lowercase().as_str() {
        "binance-us" | "binanceus" => "Binance US".to_string(),
        "binance" => "Binance".to_string(),
        "kraken" => "Kraken".to_string(),
        "coinbase" | "coinbasepro" | "coinbase-pro" => "Coinbase".to_string(),
        "ftx" => "FTX".to_string(),
        "okx" | "okex" => "OKX".to_string(),
        "bybit" => "Bybit".to_string(),
        "huobi" => "Huobi".to_string(),
        "kucoin" => "KuCoin".to_string(),
        "bitfinex" => "Bitfinex".to_string(),
        "gemini" => "Gemini".to_string(),
        other => {
            // Capitalize first letter of each word
            other.split(|c| c == '-' || c == '_')
                .map(|word| {
                    let mut chars: Vec<char> = word.chars().collect();
                    if let Some(first) = chars.first_mut() {
                        *first = first.to_uppercase().next().unwrap_or(*first);
                    }
                    chars.into_iter().collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

/// Parse file type from filename
fn parse_file_type(filename: &str) -> Option<TardisFileType> {
    let lower = filename.to_lowercase();
    if lower.contains("trades") || lower.contains("trade") {
        Some(TardisFileType::Trades)
    } else if lower.contains("orderbook") || lower.contains("book") || lower.contains("snapshot") {
        Some(TardisFileType::Orderbook)
    } else {
        None
    }
}

/// Parse symbol from filename (e.g., "BTCUSD_trades.csv.gz" -> "BTCUSD")
fn parse_symbol_from_filename(filename: &str) -> Option<String> {
    // Common patterns:
    // BTCUSD_trades.csv.gz
    // binance-us_BTCUSD_trades_2024-10-01.csv.gz
    // BTCUSD-book_snapshot_25.csv.gz
    
    let basename = filename
        .replace(".csv.gz", "")
        .replace(".csv", "")
        .replace(".gz", "");
    
    // Try to extract symbol - usually uppercase letters before underscore or hyphen
    let parts: Vec<&str> = basename.split(|c| c == '_' || c == '-').collect();
    
    for part in &parts {
        // Symbol is usually all uppercase, 6-10 chars (e.g., BTCUSD, SOLUSD, BTCUSDT)
        if part.len() >= 5 && part.len() <= 12 && part.chars().all(|c| c.is_uppercase() || c.is_numeric()) {
            return Some(part.to_string());
        }
    }
    
    // Fallback: first part if it looks like a symbol
    if let Some(&first) = parts.get(0) {
        if first.len() >= 3 && first.chars().all(|c| c.is_uppercase() || c.is_numeric()) {
            return Some(first.to_string());
        }
    }
    
    None
}

/// Parse exchange from filename (e.g., "binance-us_BTCUSD_trades.csv.gz" -> "binance-us")
fn parse_exchange_from_filename(filename: &str) -> String {
    let lower = filename.to_lowercase();
    
    if lower.contains("binance-us") || lower.contains("binanceus") {
        return "binance-us".to_string();
    } else if lower.contains("binance") {
        return "binance".to_string();
    } else if lower.contains("kraken") {
        return "kraken".to_string();
    } else if lower.contains("coinbase") {
        return "coinbase".to_string();
    } else if lower.contains("ftx") {
        return "ftx".to_string();
    } else if lower.contains("okx") || lower.contains("okex") {
        return "okx".to_string();
    } else if lower.contains("bybit") {
        return "bybit".to_string();
    }
    
    // Default - will try to extract from CSV content
    "unknown".to_string()
}

/// Parse metadata from CSV content (first few lines)
fn parse_csv_metadata(data: &[u8], file_type: TardisFileType) -> Result<(String, String, String, String, usize), String> {
    // Decompress if gzipped
    let reader: Box<dyn Read> = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        Box::new(GzDecoder::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    
    let buf_reader = BufReader::new(reader);
    let mut lines = buf_reader.lines();
    
    // Read header
    let header = lines.next()
        .ok_or("Empty file")?
        .map_err(|e| format!("Read error: {}", e))?;
    
    // Validate header format
    let expected_fields = match file_type {
        TardisFileType::Trades => {
            // trades: exchange,symbol,timestamp,local_timestamp,id,side,price,amount
            if !header.contains("exchange") || !header.contains("timestamp") {
                return Err("Invalid trades CSV format - missing required columns".to_string());
            }
            8 // minimum fields
        }
        TardisFileType::Orderbook => {
            // orderbook: exchange,symbol,timestamp,local_timestamp,asks[0].price,...,bids[24].amount
            if !header.contains("exchange") || !header.contains("asks") || !header.contains("bids") {
                return Err("Invalid orderbook CSV format - missing required columns".to_string());
            }
            104 // 4 header + 50 asks + 50 bids
        }
    };
    
    let header_fields: Vec<&str> = header.split(',').collect();
    if header_fields.len() < expected_fields {
        return Err(format!(
            "Invalid {} CSV: expected {} fields, got {}",
            file_type, expected_fields, header_fields.len()
        ));
    }
    
    // Find column indices - exchange is optional, defaults to binance-us
    let exchange_idx = header_fields.iter().position(|&f| f == "exchange");
    let symbol_idx = header_fields.iter().position(|&f| f == "symbol")
        .ok_or("Missing 'symbol' column")?;
    let timestamp_idx = header_fields.iter().position(|&f| f == "timestamp")
        .ok_or("Missing 'timestamp' column")?;
    
    // Read first data row
    let first_row = lines.next()
        .ok_or("No data rows")?
        .map_err(|e| format!("Read error: {}", e))?;
    
    let first_fields: Vec<&str> = first_row.split(',').collect();
    if first_fields.len() <= timestamp_idx {
        return Err("First data row too short".to_string());
    }
    
    // Get exchange from column or default to binance-us
    let exchange = exchange_idx
        .and_then(|idx| first_fields.get(idx).map(|s| s.to_string()))
        .unwrap_or_else(|| "binance-us".to_string());
    let symbol = first_fields[symbol_idx].to_string();
    let first_timestamp = first_fields[timestamp_idx].parse::<i64>()
        .map_err(|_| "Invalid timestamp")?;
    
    // Count rows and find last timestamp
    let mut row_count = 1;
    let mut last_timestamp = first_timestamp;
    
    for line_result in lines {
        if let Ok(line) = line_result {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() > timestamp_idx {
                if let Ok(ts) = fields[timestamp_idx].parse::<i64>() {
                    last_timestamp = ts;
                }
            }
            row_count += 1;
        }
    }
    
    // Convert timestamps to datetime strings
    // Tardis uses microseconds
    let start_dt = Utc.timestamp_micros(first_timestamp)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    
    let end_dt = Utc.timestamp_micros(last_timestamp)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    
    Ok((exchange, symbol, start_dt, end_dt, row_count))
}

/// Handle file upload
/// POST /api/tardis/upload
pub async fn upload_file(
    State(state): State<TardisUploadState>,
    mut multipart: Multipart,
) -> Result<Json<FileUploadResponse>, (StatusCode, Json<FileUploadResponse>)> {
    let mut file_data: Option<(String, bytes::Bytes)> = None;
    let mut provided_exchange: Option<String> = None;
    
    // Parse all fields first
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
            success: false,
            file: None,
            session: None,
            error: Some(format!("Failed to read multipart: {}", e)),
        }))
    })? {
        let field_name = field.name().map(|s| s.to_string());
        
        match field_name.as_deref() {
            Some("exchange") => {
                let text = field.text().await.unwrap_or_default();
                if !text.is_empty() {
                    provided_exchange = Some(text);
                }
            }
            Some("file") | None => {
                let filename = field.file_name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown.csv.gz".to_string());
                let data = field.bytes().await.map_err(|e| {
                    (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                        success: false,
                        file: None,
                        session: None,
                        error: Some(format!("Failed to read file data: {}", e)),
                    }))
                })?;
                file_data = Some((filename, data));
            }
            _ => {}
        }
    }
    
    // Ensure we have file data
    let (filename, data) = file_data.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
            success: false,
            file: None,
            session: None,
            error: Some("No file provided".to_string()),
        }))
    })?;
    
    // Determine file type
    let file_type = match parse_file_type(&filename) {
        Some(ft) => ft,
        None => {
            return Err((StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                success: false,
                file: None,
                session: None,
                error: Some(format!(
                    "Could not determine file type from filename '{}'. \
                    Expected filename containing 'trades' or 'orderbook'.",
                    filename
                )),
            })));
        }
    };
    
    let file_size = data.len();
    
    // Parse CSV metadata (exchange is now optional in CSV)
    let (csv_exchange, symbol, start_time, end_time, row_count) = 
        parse_csv_metadata(&data, file_type).map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                success: false,
                file: None,
                session: None,
                error: Some(format!("Failed to parse CSV: {}", e)),
            }))
        })?;
    
    // Use provided exchange, fall back to CSV exchange
    let exchange = provided_exchange.unwrap_or(csv_exchange);
    
    // Generate file ID
    let file_id = uuid::Uuid::new_v4().to_string();
    
    // Store file
    let stored_filename = format!("{}_{}.csv.gz", file_id, file_type);
    let stored_path = state.upload_dir.join(&stored_filename);
    
    tokio::fs::write(&stored_path, &data).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(FileUploadResponse {
            success: false,
            file: None,
            session: None,
            error: Some(format!("Failed to store file: {}", e)),
        }))
    })?;
    
    let file_info = UploadedFileInfo {
        file_id: file_id.clone(),
        original_name: filename,
        file_type,
        symbol: symbol.clone(),
        exchange: exchange.clone(),
        start_time: start_time.clone(),
        end_time: end_time.clone(),
        row_count,
        file_size_bytes: file_size,
        stored_path: stored_path.to_string_lossy().to_string(),
    };
    
    // Create or update session
    let session_id = format!("{}_{}", symbol, exchange);
    let mut sessions = state.sessions.write().await;
    
    let session = sessions.entry(session_id.clone())
        .or_insert_with(|| SymbolUploadSession {
            session_id: session_id.clone(),
            symbol: symbol.clone(),
            exchange: exchange.clone(),
            trades_file: None,
            orderbook_file: None,
            start_time: None,
            end_time: None,
            created_at: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        });
    
    // Update session with this file
    match file_type {
        TardisFileType::Trades => {
            session.trades_file = Some(file_info.clone());
        }
        TardisFileType::Orderbook => {
            session.orderbook_file = Some(file_info.clone());
        }
    }
    
    // Update session date range (intersection of both files)
    if let (Some(ref trades), Some(ref orderbook)) = (&session.trades_file, &session.orderbook_file) {
        // Use the later start time and earlier end time
        session.start_time = Some(std::cmp::max(&trades.start_time, &orderbook.start_time).clone());
        session.end_time = Some(std::cmp::min(&trades.end_time, &orderbook.end_time).clone());
    } else if let Some(ref file) = session.trades_file.as_ref().or(session.orderbook_file.as_ref()) {
        session.start_time = Some(file.start_time.clone());
        session.end_time = Some(file.end_time.clone());
    }
    
    let session_clone = session.clone();
    
    Ok(Json(FileUploadResponse {
        success: true,
        file: Some(file_info),
        session: Some(session_clone),
        error: None,
    }))
}

/// List all upload sessions
/// GET /api/tardis/sessions
pub async fn list_sessions(
    State(state): State<TardisUploadState>,
) -> Json<ListSessionsResponse> {
    let sessions = state.sessions.read().await;
    let session_list: Vec<SymbolUploadSession> = sessions.values().cloned().collect();
    
    Json(ListSessionsResponse {
        sessions: session_list,
    })
}

/// Delete an upload session
/// DELETE /api/tardis/sessions/:session_id
pub async fn delete_session(
    State(state): State<TardisUploadState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut sessions = state.sessions.write().await;
    
    if let Some(session) = sessions.remove(&session_id) {
        // Delete associated files
        if let Some(ref trades) = session.trades_file {
            let _ = tokio::fs::remove_file(&trades.stored_path).await;
        }
        if let Some(ref orderbook) = session.orderbook_file {
            let _ = tokio::fs::remove_file(&orderbook.stored_path).await;
        }
        
        Ok(Json(serde_json::json!({
            "success": true,
            "deleted_session_id": session_id,
        })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Request to run backtest on uploaded Tardis data
#[derive(Debug, Deserialize)]
pub struct TardisBacktestRequest {
    pub session_id: String,
    pub initial_capital: f64,
    pub commission_rate: f64,
    pub strategy_type: String,
    pub optimize: bool,
    pub population_size: Option<usize>,
    pub generations: Option<usize>,
    // Strategy parameters (optional, for manual mode)
    pub risk_aversion: Option<f64>,
    pub inventory_target: Option<f64>,
    pub order_size_pct: Option<f64>,
    pub min_spread_bps: Option<f64>,
    pub volatility_cap: Option<f64>,
    pub time_horizon_minutes: Option<f64>,
}

/// Response for Tardis backtest request
#[derive(Debug, Serialize)]
pub struct TardisBacktestResponse {
    pub success: bool,
    pub job_id: Option<String>,
    pub message: String,
    pub session: Option<SymbolUploadSession>,
    pub error: Option<String>,
}

/// Submit a backtest job using uploaded Tardis data
/// POST /api/tardis/backtest
pub async fn submit_tardis_backtest(
    State((upload_state, job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
    Json(request): Json<TardisBacktestRequest>,
) -> Result<Json<TardisBacktestResponse>, (StatusCode, Json<TardisBacktestResponse>)> {
    // Find the session
    let sessions = upload_state.sessions.read().await;
    let session = sessions.get(&request.session_id).cloned();
    drop(sessions);
    
    let session = match session {
        Some(s) => s,
        None => {
            return Err((StatusCode::NOT_FOUND, Json(TardisBacktestResponse {
                success: false,
                job_id: None,
                message: "Session not found".to_string(),
                session: None,
                error: Some(format!("No upload session with ID '{}'", request.session_id)),
            })));
        }
    };
    
    // Validate that we have both files
    if session.trades_file.is_none() || session.orderbook_file.is_none() {
        let missing = if session.trades_file.is_none() && session.orderbook_file.is_none() {
            "trades and orderbook files"
        } else if session.trades_file.is_none() {
            "trades file"
        } else {
            "orderbook file"
        };
        
        return Err((StatusCode::BAD_REQUEST, Json(TardisBacktestResponse {
            success: false,
            job_id: None,
            message: format!("Missing {}", missing),
            session: Some(session),
            error: Some(format!(
                "Both trades and orderbook files are required. Missing: {}",
                missing
            )),
        })));
    }
    
    let trades_file = session.trades_file.as_ref().unwrap();
    let orderbook_file = session.orderbook_file.as_ref().unwrap();
    
    // Create job parameters using Default and overriding specific fields
    let mut job_params = crate::job_queue::BacktestJobParams::default();
    job_params.symbol = session.symbol.clone();
    job_params.exchange = session.exchange.clone();
    job_params.strategy_type = request.strategy_type.clone();
    job_params.initial_capital = request.initial_capital;
    job_params.optimization_method = if request.optimize { "genetic".to_string() } else { "manual".to_string() };
    job_params.population_size = request.population_size.unwrap_or(30);
    job_params.generations = request.generations.unwrap_or(15);
    // Data source: Tardis files
    job_params.data_source = "tardis".to_string();
    job_params.tardis_trades_path = Some(trades_file.stored_path.clone());
    job_params.tardis_orderbook_path = Some(orderbook_file.stored_path.clone());
    // Date range from files
    job_params.start_date = session.start_time.clone();
    job_params.end_date = session.end_time.clone();
    // Commission
    job_params.commission_rate = request.commission_rate;
    job_params.fee_rate = request.commission_rate;
    // Strategy parameters (if provided)
    if let Some(v) = request.risk_aversion { job_params.risk_aversion = v; }
    if let Some(v) = request.inventory_target { job_params.inventory_target = v; }
    if let Some(v) = request.order_size_pct { job_params.order_size_pct = v; }
    if let Some(v) = request.volatility_cap { job_params.volatility_cap = v; }
    if let Some(v) = request.time_horizon_minutes { job_params.time_horizon_minutes = v as u64; }
    if let Some(v) = request.min_spread_bps { job_params.min_spread_bps = v; }
    // Disable Monte Carlo and Walk-Forward for Tardis backtests (can enable later)
    job_params.monte_carlo_enabled = false;
    job_params.walk_forward_enabled = false;
    job_params.fill_model = "realistic".to_string();
    
    // Submit job to queue
    match job_queue.submit_job(job_params).await {
        Ok(job_id) => {
            Ok(Json(TardisBacktestResponse {
                success: true,
                job_id: Some(job_id.clone()),
                message: format!(
                    "Backtest job submitted for {} on {} ({} - {})",
                    session.symbol, session.exchange,
                    session.start_time.as_deref().unwrap_or("?"),
                    session.end_time.as_deref().unwrap_or("?")
                ),
                session: Some(session),
                error: None,
            }))
        }
        Err(e) => {
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(TardisBacktestResponse {
                success: false,
                job_id: None,
                message: "Failed to submit job".to_string(),
                session: Some(session),
                error: Some(format!("Job submission failed: {}", e)),
            })))
        }
    }
}

// =============================================================================
// Database Import Functionality
// =============================================================================

/// Request to import Tardis orderbook data into database
#[derive(Debug, Deserialize)]
pub struct TardisImportRequest {
    pub session_id: String,
    /// If true, clear existing orders for this symbol/exchange before importing
    #[serde(default)]
    pub clear_existing: bool,
}

/// Response for Tardis import request
#[derive(Debug, Serialize)]
pub struct TardisImportResponse {
    pub success: bool,
    pub message: String,
    pub symbol: Option<String>,
    pub exchange: Option<String>,
    pub buy_orders_inserted: usize,
    pub sell_orders_inserted: usize,
    pub rows_processed: usize,
    pub error: Option<String>,
}

/// Progress update during import
#[derive(Debug, Clone, Serialize)]
pub struct ImportProgress {
    pub rows_processed: usize,
    pub total_rows: usize,
    pub buy_orders: usize,
    pub sell_orders: usize,
    pub percent_complete: f64,
}

/// Import Tardis orderbook data into the database
/// POST /api/tardis/import
#[cfg(feature = "postgres")]
pub async fn import_tardis_to_database(
    State((upload_state, job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
    Json(request): Json<TardisImportRequest>,
) -> Result<Json<TardisImportResponse>, (StatusCode, Json<TardisImportResponse>)> {
    // Get database pool
    let pool = match job_queue.get_pool() {
        Some(pool) => pool,
        None => {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
                success: false,
                message: "Database not available".to_string(),
                symbol: None,
                exchange: None,
                buy_orders_inserted: 0,
                sell_orders_inserted: 0,
                rows_processed: 0,
                error: Some("Database connection not configured".to_string()),
            })));
        }
    };

    // Find the session
    let sessions = upload_state.sessions.read().await;
    let session = sessions.get(&request.session_id).cloned();
    drop(sessions);
    
    let session = match session {
        Some(s) => s,
        None => {
            return Err((StatusCode::NOT_FOUND, Json(TardisImportResponse {
                success: false,
                message: "Session not found".to_string(),
                symbol: None,
                exchange: None,
                buy_orders_inserted: 0,
                sell_orders_inserted: 0,
                rows_processed: 0,
                error: Some(format!("No upload session with ID '{}'", request.session_id)),
            })));
        }
    };

    // Need orderbook file for import
    let orderbook_file = match &session.orderbook_file {
        Some(f) => f,
        None => {
            return Err((StatusCode::BAD_REQUEST, Json(TardisImportResponse {
                success: false,
                message: "No orderbook file uploaded".to_string(),
                symbol: Some(session.symbol.clone()),
                exchange: Some(session.exchange.clone()),
                buy_orders_inserted: 0,
                sell_orders_inserted: 0,
                rows_processed: 0,
                error: Some("Upload an orderbook CSV file first".to_string()),
            })));
        }
    };

    // Normalize symbol and exchange
    let normalized_symbol = normalize_symbol(&session.symbol);
    let normalized_exchange = normalize_exchange(&session.exchange);
    
    tracing::info!(
        "Starting Tardis import: {} ({}) on {} ({})",
        session.symbol, normalized_symbol,
        session.exchange, normalized_exchange
    );

    // Get or create exchange using databaseschema ops
    let exchange_record = exchange_ops::create_exchange(
        pool.clone(),
        NewExchange { exchange: normalized_exchange.clone() }
    ).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to create exchange".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(format!("Exchange error: {}", e)),
        }))
    })?;
    let exchange_id = exchange_record.exchange_id;

    // Get or create security using databaseschema ops
    let security_record = securities_ops::create_security(
        pool.clone(),
        NewSecurity { symbol: normalized_symbol.clone() }
    ).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to create security".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(format!("Security error: {}", e)),
        }))
    })?;
    let security_id = security_record.security_id;

    // Get or create order book using databaseschema ops
    let order_book_record = order_book_ops::create_orderbook(
        pool.clone(),
        NewOrderBook {
            symbol: normalized_symbol.clone(),
            exchange: normalized_exchange.clone(),
            security_id,
            exchange_id,
        }
    ).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to create order book".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(format!("Order book error: {}", e)),
        }))
    })?;

    tracing::info!(
        "Using order_book_id: {}, security_id: {}, exchange_id: {}",
        order_book_record.order_book_id, security_id, exchange_id
    );

    // Clear existing L2 snapshots if requested
    if request.clear_existing {
        match l2_orderbook_snapshot_ops::delete_l2_snapshots_by_symbol_exchange(
            pool.clone(),
            &normalized_symbol,
            &normalized_exchange,
        ).await {
            Ok(count) => tracing::info!("Cleared {} L2 snapshots", count),
            Err(e) => tracing::warn!("Failed to clear L2 snapshots: {}", e),
        }
    }

    // Parse and import orderbook data
    let file_data = tokio::fs::read(&orderbook_file.stored_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to read orderbook file".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(format!("File read error: {}", e)),
        }))
    })?;

    // Parse CSV and insert L2 snapshots
    let (snapshots_inserted, rows_processed) = parse_and_insert_l2_orderbook(
        &file_data,
        &normalized_symbol,
        &normalized_exchange,
        security_id,
        exchange_id,
        pool.clone(),
    ).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to parse orderbook".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(e),
        }))
    })?;

    Ok(Json(TardisImportResponse {
        success: true,
        message: format!(
            "Successfully imported {} L2 snapshots from {} rows",
            snapshots_inserted, rows_processed
        ),
        symbol: Some(normalized_symbol),
        exchange: Some(normalized_exchange),
        buy_orders_inserted: snapshots_inserted,  // Repurpose field for snapshot count
        sell_orders_inserted: 0,
        rows_processed,
        error: None,
    }))
}

/// Parse orderbook CSV and insert L2 snapshots into database
#[cfg(feature = "postgres")]
async fn parse_and_insert_l2_orderbook(
    data: &[u8],
    symbol: &str,
    exchange: &str,
    security_id: uuid::Uuid,
    exchange_id: uuid::Uuid,
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
) -> Result<(usize, usize), String> {
    // Decompress if gzipped
    let reader: Box<dyn Read + Send> = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        Box::new(GzDecoder::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    
    let buf_reader = BufReader::new(reader);
    let mut lines = buf_reader.lines();
    
    // Read header
    let header = lines.next()
        .ok_or("Empty file")?
        .map_err(|e| format!("Read error: {}", e))?;
    
    let header_fields: Vec<&str> = header.split(',').collect();
    
    // Find column indices for asks and bids (0-24 levels each)
    let timestamp_idx = header_fields.iter().position(|&f| f == "timestamp")
        .ok_or("Missing 'timestamp' column")?;
    
    // Find ask and bid column ranges
    let mut ask_indices: Vec<(usize, usize)> = Vec::new();
    let mut bid_indices: Vec<(usize, usize)> = Vec::new();
    
    for level in 0..25 {
        let ask_price_col = format!("asks[{}].price", level);
        let ask_amount_col = format!("asks[{}].amount", level);
        let bid_price_col = format!("bids[{}].price", level);
        let bid_amount_col = format!("bids[{}].amount", level);
        
        if let (Some(ap), Some(aa)) = (
            header_fields.iter().position(|&f| f == ask_price_col),
            header_fields.iter().position(|&f| f == ask_amount_col),
        ) {
            ask_indices.push((ap, aa));
        }
        
        if let (Some(bp), Some(ba)) = (
            header_fields.iter().position(|&f| f == bid_price_col),
            header_fields.iter().position(|&f| f == bid_amount_col),
        ) {
            bid_indices.push((bp, ba));
        }
    }
    
    if ask_indices.is_empty() || bid_indices.is_empty() {
        return Err("No ask/bid columns found in orderbook CSV".to_string());
    }
    
    tracing::info!("Found {} ask levels and {} bid levels", ask_indices.len(), bid_indices.len());
    
    let mut rows_processed = 0;
    let mut total_snapshots = 0;
    
    // Batch for efficient inserts
    const BATCH_SIZE: usize = 1000;
    let mut snapshot_batch: Vec<NewL2OrderbookSnapshot> = Vec::with_capacity(BATCH_SIZE);
    
    for line_result in lines {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() <= timestamp_idx {
            continue;
        }
        
        // Parse timestamp (microseconds)
        let timestamp_us: i64 = match fields[timestamp_idx].parse() {
            Ok(ts) => ts,
            Err(_) => continue,
        };
        
        let timestamp = match Utc.timestamp_micros(timestamp_us).single() {
            Some(dt) => dt,
            None => continue,
        };
        
        rows_processed += 1;
        
        // Collect bid levels (prices and quantities)
        let mut bid_prices: Vec<BigDecimal> = Vec::with_capacity(bid_indices.len());
        let mut bid_quantities: Vec<BigDecimal> = Vec::with_capacity(bid_indices.len());
        
        for (price_idx, amount_idx) in bid_indices.iter() {
            if *price_idx >= fields.len() || *amount_idx >= fields.len() {
                break;
            }
            
            let price_str = fields[*price_idx].trim();
            let amount_str = fields[*amount_idx].trim();
            
            if price_str.is_empty() || amount_str.is_empty() {
                break;
            }
            
            match (BigDecimal::from_str(price_str), BigDecimal::from_str(amount_str)) {
                (Ok(price), Ok(quantity)) if price > BigDecimal::from(0) && quantity >= BigDecimal::from(0) => {
                    bid_prices.push(price);
                    bid_quantities.push(quantity);
                }
                _ => break,
            }
        }
        
        // Collect ask levels (prices and quantities)
        let mut ask_prices: Vec<BigDecimal> = Vec::with_capacity(ask_indices.len());
        let mut ask_quantities: Vec<BigDecimal> = Vec::with_capacity(ask_indices.len());
        
        for (price_idx, amount_idx) in ask_indices.iter() {
            if *price_idx >= fields.len() || *amount_idx >= fields.len() {
                break;
            }
            
            let price_str = fields[*price_idx].trim();
            let amount_str = fields[*amount_idx].trim();
            
            if price_str.is_empty() || amount_str.is_empty() {
                break;
            }
            
            match (BigDecimal::from_str(price_str), BigDecimal::from_str(amount_str)) {
                (Ok(price), Ok(quantity)) if price > BigDecimal::from(0) && quantity >= BigDecimal::from(0) => {
                    ask_prices.push(price);
                    ask_quantities.push(quantity);
                }
                _ => break,
            }
        }
        
        // Skip if no valid data
        if bid_prices.is_empty() && ask_prices.is_empty() {
            continue;
        }
        
        // Create L2 snapshot
        snapshot_batch.push(NewL2OrderbookSnapshot::new(
            timestamp,
            symbol,
            exchange,
            security_id,
            exchange_id,
            bid_prices,
            bid_quantities,
            ask_prices,
            ask_quantities,
        ));
        
        // Insert batch when it reaches the limit
        if snapshot_batch.len() >= BATCH_SIZE {
            let batch_to_insert = std::mem::take(&mut snapshot_batch);
            match l2_orderbook_snapshot_ops::create_l2_snapshots(pool.clone(), batch_to_insert).await {
                Ok(count) => total_snapshots += count,
                Err(e) => tracing::warn!("L2 snapshot batch insert error: {}", e),
            }
        }
        
        // Log progress every 10000 rows
        if rows_processed % 10000 == 0 {
            tracing::info!(
                "Progress: {} rows, {} L2 snapshots inserted",
                rows_processed, total_snapshots
            );
        }
    }
    
    // Insert remaining batch
    if !snapshot_batch.is_empty() {
        match l2_orderbook_snapshot_ops::create_l2_snapshots(pool.clone(), snapshot_batch).await {
            Ok(count) => total_snapshots += count,
            Err(e) => tracing::warn!("Final L2 snapshot batch insert error: {}", e),
        }
    }
    
    tracing::info!(
        "Import complete: {} rows processed, {} L2 snapshots inserted",
        rows_processed, total_snapshots
    );
    
    Ok((total_snapshots, rows_processed))
}

/// Stub implementation when postgres feature is disabled
#[cfg(not(feature = "postgres"))]
pub async fn import_tardis_to_database(
    State((_upload_state, _job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
    Json(_request): Json<TardisImportRequest>,
) -> Result<Json<TardisImportResponse>, (StatusCode, Json<TardisImportResponse>)> {
    Err((StatusCode::NOT_IMPLEMENTED, Json(TardisImportResponse {
        success: false,
        message: "Database features not enabled".to_string(),
        symbol: None,
        exchange: None,
        buy_orders_inserted: 0,
        sell_orders_inserted: 0,
        rows_processed: 0,
        error: Some("Build with --features postgres".to_string()),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Symbol Normalization Tests
    // ============================================================================
    // These tests ensure that both Tardis CSV data and Binance US API data
    // produce identical normalized symbols in the database.
    //
    // Binance US API format: "BTCUSD", "ETHBTC", "BNBUSDT" (concatenated)
    // Tardis CSV format:     "BTCUSD", "SOLUSD", "BTCUSDT" (concatenated)
    // Database format:       "BTC/USD", "SOL/USD", "BTC/USDT" (with slash)
    // ============================================================================

    #[test]
    fn test_normalize_symbol_usd_pairs() {
        // Common USD pairs from both Tardis and Binance US API
        assert_eq!(normalize_symbol("BTCUSD"), "BTC/USD");
        assert_eq!(normalize_symbol("ETHUSD"), "ETH/USD");
        assert_eq!(normalize_symbol("SOLUSD"), "SOL/USD");
        assert_eq!(normalize_symbol("SUIUSD"), "SUI/USD");
        assert_eq!(normalize_symbol("BNBUSD"), "BNB/USD");
        assert_eq!(normalize_symbol("XRPUSD"), "XRP/USD");
        assert_eq!(normalize_symbol("DOGEUSD"), "DOGE/USD");
        assert_eq!(normalize_symbol("AVAXUSD"), "AVAX/USD");
    }

    #[test]
    fn test_normalize_symbol_usdt_pairs() {
        // USDT pairs (common on Binance)
        assert_eq!(normalize_symbol("BTCUSDT"), "BTC/USDT");
        assert_eq!(normalize_symbol("ETHUSDT"), "ETH/USDT");
        assert_eq!(normalize_symbol("SOLUSDT"), "SOL/USDT");
        assert_eq!(normalize_symbol("BNBUSDT"), "BNB/USDT");
    }

    #[test]
    fn test_normalize_symbol_usdc_pairs() {
        // USDC pairs
        assert_eq!(normalize_symbol("BTCUSDC"), "BTC/USDC");
        assert_eq!(normalize_symbol("ETHUSDC"), "ETH/USDC");
    }

    #[test]
    fn test_normalize_symbol_btc_quote_pairs() {
        // BTC as quote currency
        assert_eq!(normalize_symbol("ETHBTC"), "ETH/BTC");
        assert_eq!(normalize_symbol("BNBBTC"), "BNB/BTC");
        assert_eq!(normalize_symbol("LTCBTC"), "LTC/BTC");
        assert_eq!(normalize_symbol("SOLBTC"), "SOL/BTC");
    }

    #[test]
    fn test_normalize_symbol_eth_quote_pairs() {
        // ETH as quote currency
        assert_eq!(normalize_symbol("BNBETH"), "BNB/ETH");
        assert_eq!(normalize_symbol("LINKETH"), "LINK/ETH");
    }

    #[test]
    fn test_normalize_symbol_case_insensitive() {
        // Should handle lowercase input (Tardis sometimes uses lowercase)
        assert_eq!(normalize_symbol("btcusd"), "BTC/USD");
        assert_eq!(normalize_symbol("Btcusd"), "BTC/USD");
        assert_eq!(normalize_symbol("BtCuSd"), "BTC/USD");
    }

    #[test]
    fn test_normalize_symbol_already_normalized() {
        // If already has slash, should preserve (edge case)
        // Current implementation would return as-is since no quote match
        let result = normalize_symbol("BTC");
        assert_eq!(result, "BTC"); // No quote currency found
    }

    #[test]
    fn test_normalize_symbol_fiat_pairs() {
        // EUR and GBP pairs
        assert_eq!(normalize_symbol("BTCEUR"), "BTC/EUR");
        assert_eq!(normalize_symbol("ETHEUR"), "ETH/EUR");
        assert_eq!(normalize_symbol("BTCGBP"), "BTC/GBP");
    }

    // ============================================================================
    // Exchange Normalization Tests
    // ============================================================================
    // These tests ensure exchange names are consistently formatted.
    //
    // Tardis CSV format:  "binance-us", "binance", "kraken" (lowercase with hyphens)
    // Database format:    "Binance US", "Binance", "Kraken" (display format)
    // ============================================================================

    #[test]
    fn test_normalize_exchange_binance_us() {
        // Various ways Binance US might appear
        assert_eq!(normalize_exchange("binance-us"), "Binance US");
        assert_eq!(normalize_exchange("binanceus"), "Binance US");
        assert_eq!(normalize_exchange("BINANCE-US"), "Binance US");
        assert_eq!(normalize_exchange("BinanceUS"), "Binance US");
    }

    #[test]
    fn test_normalize_exchange_binance() {
        assert_eq!(normalize_exchange("binance"), "Binance");
        assert_eq!(normalize_exchange("BINANCE"), "Binance");
        assert_eq!(normalize_exchange("Binance"), "Binance");
    }

    #[test]
    fn test_normalize_exchange_kraken() {
        assert_eq!(normalize_exchange("kraken"), "Kraken");
        assert_eq!(normalize_exchange("KRAKEN"), "Kraken");
    }

    #[test]
    fn test_normalize_exchange_coinbase() {
        assert_eq!(normalize_exchange("coinbase"), "Coinbase");
        assert_eq!(normalize_exchange("coinbasepro"), "Coinbase");
        assert_eq!(normalize_exchange("coinbase-pro"), "Coinbase");
    }

    #[test]
    fn test_normalize_exchange_other_major() {
        assert_eq!(normalize_exchange("okx"), "OKX");
        assert_eq!(normalize_exchange("okex"), "OKX");
        assert_eq!(normalize_exchange("bybit"), "Bybit");
        assert_eq!(normalize_exchange("huobi"), "Huobi");
        assert_eq!(normalize_exchange("kucoin"), "KuCoin");
        assert_eq!(normalize_exchange("bitfinex"), "Bitfinex");
        assert_eq!(normalize_exchange("gemini"), "Gemini");
        assert_eq!(normalize_exchange("ftx"), "FTX");
    }

    #[test]
    fn test_normalize_exchange_unknown() {
        // Unknown exchanges should be capitalized properly
        assert_eq!(normalize_exchange("new-exchange"), "New Exchange");
        assert_eq!(normalize_exchange("my_exchange"), "My Exchange");
        assert_eq!(normalize_exchange("testexchange"), "Testexchange");
    }

    // ============================================================================
    // File Type Detection Tests
    // ============================================================================

    #[test]
    fn test_parse_file_type_trades() {
        assert_eq!(parse_file_type("BTCUSD_trades.csv.gz"), Some(TardisFileType::Trades));
        assert_eq!(parse_file_type("binance-us_BTCUSD_trades_2024-10-01.csv.gz"), Some(TardisFileType::Trades));
        assert_eq!(parse_file_type("trade_data.csv"), Some(TardisFileType::Trades));
    }

    #[test]
    fn test_parse_file_type_orderbook() {
        assert_eq!(parse_file_type("BTCUSD_book_snapshot_25.csv.gz"), Some(TardisFileType::Orderbook));
        assert_eq!(parse_file_type("binance-us_BTCUSD_orderbook_2024-10-01.csv.gz"), Some(TardisFileType::Orderbook));
        assert_eq!(parse_file_type("snapshot_data.csv"), Some(TardisFileType::Orderbook));
    }

    #[test]
    fn test_parse_file_type_unknown() {
        assert_eq!(parse_file_type("BTCUSD_data.csv"), None);
        assert_eq!(parse_file_type("random_file.txt"), None);
    }

    // ============================================================================
    // Symbol Extraction from Filename Tests
    // ============================================================================

    #[test]
    fn test_parse_symbol_from_filename() {
        assert_eq!(parse_symbol_from_filename("BTCUSD_trades.csv.gz"), Some("BTCUSD".to_string()));
        assert_eq!(parse_symbol_from_filename("binance-us_SOLUSD_trades_2024-10-01.csv.gz"), Some("SOLUSD".to_string()));
        assert_eq!(parse_symbol_from_filename("ETHUSDT_book_snapshot_25.csv"), Some("ETHUSDT".to_string()));
    }

    // ============================================================================
    // Exchange Extraction from Filename Tests
    // ============================================================================

    #[test]
    fn test_parse_exchange_from_filename() {
        assert_eq!(parse_exchange_from_filename("binance-us_BTCUSD_trades.csv.gz"), "binance-us");
        assert_eq!(parse_exchange_from_filename("binanceus_BTCUSD_trades.csv.gz"), "binance-us");
        assert_eq!(parse_exchange_from_filename("kraken_BTCUSD_trades.csv.gz"), "kraken");
        assert_eq!(parse_exchange_from_filename("coinbase_BTCUSD_trades.csv.gz"), "coinbase");
        assert_eq!(parse_exchange_from_filename("BTCUSD_trades.csv.gz"), "unknown");
    }

    // ============================================================================
    // Integration-style Tests (CSV vs API format consistency)
    // ============================================================================
    // These tests verify that data from different sources produces identical 
    // database records after normalization.

    #[test]
    fn test_tardis_and_binance_api_produce_same_output() {
        // Tardis CSV might have symbol as "BTCUSD" with exchange "binance-us"
        let tardis_symbol = "BTCUSD";
        let tardis_exchange = "binance-us";
        
        // Binance API returns symbol as "BTCUSD" (you know it's Binance US from the endpoint)
        let binance_api_symbol = "BTCUSD";
        let binance_api_exchange = "binance-us"; // Known from API endpoint
        
        // Both should normalize to the same values
        assert_eq!(normalize_symbol(tardis_symbol), normalize_symbol(binance_api_symbol));
        assert_eq!(normalize_exchange(tardis_exchange), normalize_exchange(binance_api_exchange));
        
        // Final database format
        assert_eq!(normalize_symbol(tardis_symbol), "BTC/USD");
        assert_eq!(normalize_exchange(tardis_exchange), "Binance US");
    }

    #[test]
    fn test_different_symbol_formats_same_pair() {
        // All these represent the same trading pair and should normalize identically
        let formats = vec!["BTCUSD", "btcusd", "BtcUsd", "BTCUSD"];
        
        let normalized: Vec<String> = formats.iter().map(|s| normalize_symbol(s)).collect();
        
        // All should produce the same result
        assert!(normalized.iter().all(|s| s == "BTC/USD"));
    }

    #[test]
    fn test_quote_currency_priority() {
        // USDT should be matched before USD (longer match first)
        // This ensures "BTCUSDT" doesn't become "BTCUSD/T"
        assert_eq!(normalize_symbol("BTCUSDT"), "BTC/USDT");
        assert_eq!(normalize_symbol("BTCUSD"), "BTC/USD");
        
        // USDC should be matched correctly
        assert_eq!(normalize_symbol("BTCUSDC"), "BTC/USDC");
        
        // BUSD should be matched correctly
        assert_eq!(normalize_symbol("BTCBUSD"), "BTC/BUSD");
    }
}
