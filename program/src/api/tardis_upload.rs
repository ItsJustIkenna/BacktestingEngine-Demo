//! Tardis CSV file upload handling
//! 
//! Handles drag-and-drop file uploads of Tardis.dev L2 orderbook and trades data.
//! Parses metadata (symbol, exchange, date range) from the CSV files.

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
use chrono::{DateTime, Utc, TimeZone};

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
    if let Some(first) = parts.first() {
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
    
    // Find column indices
    let exchange_idx = header_fields.iter().position(|&f| f == "exchange")
        .ok_or("Missing 'exchange' column")?;
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
    
    let exchange = first_fields[exchange_idx].to_string();
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
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
            success: false,
            file: None,
            session: None,
            error: Some(format!("Failed to read multipart: {}", e)),
        }))
    })? {
        let filename = field.file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown.csv.gz".to_string());
        
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
        
        // Read file data
        let data = field.bytes().await.map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                success: false,
                file: None,
                session: None,
                error: Some(format!("Failed to read file data: {}", e)),
            }))
        })?;
        
        let file_size = data.len();
        
        // Parse CSV metadata
        let (exchange, symbol, start_time, end_time, row_count) = 
            parse_csv_metadata(&data, file_type).map_err(|e| {
                (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                    success: false,
                    file: None,
                    session: None,
                    error: Some(format!("Failed to parse CSV: {}", e)),
                }))
            })?;
        
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
        
        return Ok(Json(FileUploadResponse {
            success: true,
            file: Some(file_info),
            session: Some(session_clone),
            error: None,
        }));
    }
    
    Err((StatusCode::BAD_REQUEST, Json(FileUploadResponse {
        success: false,
        file: None,
        session: None,
        error: Some("No file provided".to_string()),
    })))
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
