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
        historical_order::NewHistoricalOrder,
        trade::NewTrade,
    },
    ops::{
        exchange_ops,
        securities_ops,
        order_book_ops,
        l2_orderbook_snapshot_ops,
        historical_order_ops,
        trades_ops,
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
    /// L2 orderbook snapshots (book_snapshot_5, book_snapshot_25)
    Orderbook,
    /// L2 incremental updates (incremental_book_L2) - higher fidelity than snapshots
    IncrementalL2,
}

impl std::fmt::Display for TardisFileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TardisFileType::Trades => write!(f, "trades"),
            TardisFileType::Orderbook => write!(f, "orderbook"),
            TardisFileType::IncrementalL2 => write!(f, "incremental_l2"),
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
    /// L2 snapshot orderbook file (book_snapshot_5, book_snapshot_25)
    pub orderbook_file: Option<UploadedFileInfo>,
    /// L2 incremental updates file (incremental_book_L2) - higher fidelity than snapshots
    pub incremental_l2_file: Option<UploadedFileInfo>,
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
    // Note: More specific suffixes (USDT, USDC, BUSD) must come before less specific (USD)
    // to avoid false matches like BNBUSD -> BN/BUSD instead of BNB/USD.
    // We also need to validate the base currency is reasonable (>= 2 chars for most, >= 3 for ambiguous).
    let quote_currencies = ["USDT", "USDC", "BUSD", "USD", "EUR", "GBP", "BTC", "ETH"];
    
    for quote in &quote_currencies {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            
            // Special handling for BUSD: require at least 3 char base to avoid
            // false positives like BNBUSD being parsed as BN/BUSD instead of BNB/USD
            if *quote == "BUSD" && base.len() < 3 {
                continue; // Skip BUSD match, let USD match handle it
            }
            
            return format!("{}/{}", base, quote);
        }
    }
    
    // Fallback: if no known quote currency found, return as-is
    symbol
}

/// Normalize exchange name from Tardis format to display format
/// Converts: binance-us -> Binance, binance -> Binance, kraken -> Kraken
fn normalize_exchange(exchange: &str) -> String {
    match exchange.to_lowercase().as_str() {
        "binance-us" | "binanceus" | "binance" => "Binance".to_string(),
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
/// Returns None if type cannot be determined from filename (will try CSV headers)
fn parse_file_type_from_filename(filename: &str) -> Option<TardisFileType> {
    let lower = filename.to_lowercase();
    if lower.contains("trades") || lower.contains("trade") {
        Some(TardisFileType::Trades)
    } else if lower.contains("incremental_book") || lower.contains("incremental-book") {
        Some(TardisFileType::IncrementalL2)
    } else if lower.contains("book_snapshot") || lower.contains("book-snapshot") {
        Some(TardisFileType::Orderbook)
    } else if lower.contains("orderbook") || lower.contains("snapshot") {
        Some(TardisFileType::Orderbook)
    } else {
        None // Will auto-detect from CSV headers
    }
}

/// Detect file type from CSV header columns
fn detect_file_type_from_headers(header: &str) -> Option<TardisFileType> {
    let lower = header.to_lowercase();
    
    // L2 snapshots have asks[0].price, bids[0].price columns
    if lower.contains("asks[0].price") && lower.contains("bids[0].price") {
        return Some(TardisFileType::Orderbook);
    }
    
    // Incremental L2 has is_snapshot,side,price,amount columns
    if lower.contains("is_snapshot") && lower.contains(",side,") && lower.contains(",price,") && lower.contains(",amount") {
        return Some(TardisFileType::IncrementalL2);
    }
    
    // Trades have id,side,price,amount (no is_snapshot)
    if lower.contains(",id,") && lower.contains(",side,") && lower.contains(",price,") && lower.contains(",amount") {
        // Check it's NOT incremental (no is_snapshot)
        if !lower.contains("is_snapshot") {
            return Some(TardisFileType::Trades);
        }
    }
    
    None
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
/// Returns: (exchange, symbol, start_time, end_time, row_count, detected_file_type)
fn parse_csv_metadata(data: &[u8], file_type_hint: Option<TardisFileType>) -> Result<(String, String, String, String, usize, TardisFileType), String> {
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
    
    // Auto-detect file type from headers if not provided
    let file_type = file_type_hint
        .or_else(|| detect_file_type_from_headers(&header))
        .ok_or_else(|| format!(
            "Could not determine file type from headers. Expected one of:\n\
             - trades: exchange,symbol,timestamp,local_timestamp,id,side,price,amount\n\
             - orderbook (L2 snapshot): exchange,symbol,timestamp,...,asks[0].price,...,bids[0].price,...\n\
             - incremental_l2: exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount\n\
             Got header: {}", header
        ))?;
    
    // Validate header format based on detected type
    let expected_fields = match file_type {
        TardisFileType::Trades => {
            // trades: exchange,symbol,timestamp,local_timestamp,id,side,price,amount
            if !header.contains("timestamp") {
                return Err("Invalid trades CSV format - missing required columns".to_string());
            }
            6 // minimum fields (exchange may be optional)
        }
        TardisFileType::Orderbook => {
            // orderbook: exchange,symbol,timestamp,local_timestamp,asks[0].price,...,bids[24].amount
            if !header.contains("asks") || !header.contains("bids") {
                return Err("Invalid orderbook CSV format - missing required columns".to_string());
            }
            24 // 4 header + at least 10 asks + 10 bids (book_snapshot_5)
        }
        TardisFileType::IncrementalL2 => {
            // incremental_book_L2: exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
            if !header.contains("is_snapshot") || !header.contains("side") || !header.contains("price") {
                return Err("Invalid incremental_book_L2 CSV format - missing required columns".to_string());
            }
            7 // minimum fields
        }
    };
    
    let header_fields: Vec<&str> = header.split(',').collect();
    if header_fields.len() < expected_fields {
        return Err(format!(
            "Invalid {} CSV: expected at least {} fields, got {}",
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
    
    Ok((exchange, symbol, start_dt, end_dt, row_count, file_type))
}

/// Handle file upload and auto-import to database
/// POST /api/tardis/upload
#[cfg(feature = "postgres")]
pub async fn upload_file(
    State((state, job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
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
    
    // Try to determine file type from filename (hint for CSV parsing)
    let file_type_hint = parse_file_type_from_filename(&filename);
    
    let file_size = data.len();
    
    // Parse CSV metadata with auto-detection from headers if filename didn't indicate type
    let (csv_exchange, symbol, start_time, end_time, row_count, detected_file_type) = 
        parse_csv_metadata(&data, file_type_hint).map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(FileUploadResponse {
                success: false,
                file: None,
                session: None,
                error: Some(format!("Failed to parse CSV: {}", e)),
            }))
        })?;
    
    let file_type = detected_file_type;
    
    // Use provided exchange, fall back to CSV exchange
    let exchange = provided_exchange.unwrap_or(csv_exchange);
    
    // Generate file ID
    let file_id = uuid::Uuid::new_v4().to_string();
    
    // Store file
    let stored_filename = format!("{}_{}.csv.gz", file_id, file_type);
    let stored_path = state.upload_dir.join(&stored_filename);
    
    eprintln!("📁 [TARDIS] Storing file to: {:?} ({} bytes)", stored_path, data.len());
    
    tokio::fs::write(&stored_path, &data).await.map_err(|e| {
        eprintln!("❌ [TARDIS] Failed to write file: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(FileUploadResponse {
            success: false,
            file: None,
            session: None,
            error: Some(format!("Failed to store file: {}", e)),
        }))
    })?;
    
    eprintln!("✅ [TARDIS] File stored successfully");
    
    // Normalize symbol and exchange for database consistency
    let normalized_symbol = normalize_symbol(&symbol);
    let normalized_exchange = normalize_exchange(&exchange);
    
    eprintln!("🔄 [TARDIS] Normalizing: {}:{} -> {}:{}", symbol, exchange, normalized_symbol, normalized_exchange);
    
    eprintln!(
        "📤 [TARDIS] Uploaded {} file: {} ({} rows) - {} on {} (normalized: {}/{})",
        file_type, filename, row_count, symbol, exchange, normalized_symbol, normalized_exchange
    );
    
    let file_info = UploadedFileInfo {
        file_id: file_id.clone(),
        original_name: filename,
        file_type,
        symbol: normalized_symbol.clone(),
        exchange: normalized_exchange.clone(),
        start_time: start_time.clone(),
        end_time: end_time.clone(),
        row_count,
        file_size_bytes: file_size,
        stored_path: stored_path.to_string_lossy().to_string(),
    };
    
    // Create or update session using normalized values
    let session_id = format!("{}_{}", normalized_symbol, normalized_exchange);
    let mut sessions = state.sessions.write().await;
    
    let session = sessions.entry(session_id.clone())
        .or_insert_with(|| SymbolUploadSession {
            session_id: session_id.clone(),
            symbol: normalized_symbol.clone(),
            exchange: normalized_exchange.clone(),
            trades_file: None,
            orderbook_file: None,
            incremental_l2_file: None,
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
        TardisFileType::IncrementalL2 => {
            session.incremental_l2_file = Some(file_info.clone());
        }
    }
    
    // Update session date range (intersection of all uploaded files)
    // Prefer incremental L2 over L2 snapshots for orderbook data
    let orderbook_source = session.incremental_l2_file.as_ref()
        .or(session.orderbook_file.as_ref());
    
    if let (Some(ref trades), Some(ref orderbook)) = (&session.trades_file, orderbook_source) {
        // Use the later start time and earlier end time
        session.start_time = Some(std::cmp::max(&trades.start_time, &orderbook.start_time).clone());
        session.end_time = Some(std::cmp::min(&trades.end_time, &orderbook.end_time).clone());
    } else if let Some(ref file) = session.trades_file.as_ref()
        .or(session.incremental_l2_file.as_ref())
        .or(session.orderbook_file.as_ref()) 
    {
        session.start_time = Some(file.start_time.clone());
        session.end_time = Some(file.end_time.clone());
    }
    
    let session_clone = session.clone();
    drop(sessions); // Release the write lock before database operations
    
    eprintln!("🔍 [TARDIS] Checking for database pool...");
    
    // Auto-import to database
    if let Some(pool) = job_queue.get_pool() {
        eprintln!("✅ [TARDIS] Database pool found, starting auto-import for file_type={:?}", file_info.file_type);
        eprintln!("   file_id: {}", file_info.file_id);
        eprintln!("   stored_path: {}", file_info.stored_path);
        eprintln!("   symbol: {}, exchange: {}", normalized_symbol, normalized_exchange);
        eprintln!("   row_count from metadata: {}", file_info.row_count);
        
        let import_result = auto_import_to_database(
            &file_info,
            &normalized_symbol,
            &normalized_exchange,
            pool,
        ).await;
        
        match import_result {
            Ok((rows_inserted, data_type)) => {
                eprintln!(
                    "✅ [TARDIS] Auto-imported {} {} rows for {}/{} to database",
                    rows_inserted, data_type, normalized_symbol, normalized_exchange
                );
            }
            Err(e) => {
                eprintln!("⚠️ [TARDIS] Auto-import failed (data saved to file): {}", e);
                // Don't fail the upload - the file is still saved
            }
        }
    } else {
        eprintln!("⚠️ [TARDIS] No database pool available, skipping auto-import");
    }
    
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
    State((state, _job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
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
    State((state, job_queue)): State<(TardisUploadState, Arc<dyn crate::job_queue::JobQueue>)>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut sessions = state.sessions.write().await;
    
    if let Some(session) = sessions.remove(&session_id) {
        let symbol = session.symbol.clone();
        let exchange = session.exchange.clone();
        
        // Delete associated files
        if let Some(ref trades) = session.trades_file {
            let _ = tokio::fs::remove_file(&trades.stored_path).await;
        }
        if let Some(ref orderbook) = session.orderbook_file {
            let _ = tokio::fs::remove_file(&orderbook.stored_path).await;
        }
        if let Some(ref incremental) = session.incremental_l2_file {
            let _ = tokio::fs::remove_file(&incremental.stored_path).await;
        }
        
        // Also delete imported data from database
        let mut deleted_trades = 0usize;
        let mut deleted_historical = 0usize;
        let mut deleted_snapshots = 0usize;
        
        if let Some(pool) = job_queue.get_pool() {
            eprintln!("🗑️ [TARDIS] Deleting database data for {} on {}...", symbol, exchange);
            
            // Delete trades
            match databaseschema::ops::trades_ops::delete_trades_by_symbol_exchange(
                pool.clone(), &symbol, &exchange
            ).await {
                Ok(count) => {
                    deleted_trades = count;
                    eprintln!("✅ [TARDIS] Deleted {} trades", count);
                },
                Err(e) => eprintln!("⚠️ [TARDIS] Failed to delete trades: {}", e),
            }
            
            // Delete historical orders
            match databaseschema::ops::historical_order_ops::delete_historical_orders_by_symbol_exchange(
                pool.clone(), &symbol, &exchange
            ).await {
                Ok(count) => {
                    deleted_historical = count;
                    eprintln!("✅ [TARDIS] Deleted {} historical orders", count);
                },
                Err(e) => eprintln!("⚠️ [TARDIS] Failed to delete historical orders: {}", e),
            }
            
            // Delete L2 snapshots
            match databaseschema::ops::l2_orderbook_snapshot_ops::delete_l2_snapshots_by_symbol_exchange(
                pool.clone(), &symbol, &exchange
            ).await {
                Ok(count) => {
                    deleted_snapshots = count;
                    eprintln!("✅ [TARDIS] Deleted {} L2 snapshots", count);
                },
                Err(e) => eprintln!("⚠️ [TARDIS] Failed to delete L2 snapshots: {}", e),
            }
        }
        
        Ok(Json(serde_json::json!({
            "success": true,
            "deleted_session_id": session_id,
            "deleted_trades": deleted_trades,
            "deleted_historical_orders": deleted_historical,
            "deleted_l2_snapshots": deleted_snapshots,
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
    
    // Validate that we have both files (orderbook can be either orderbook_file or incremental_l2_file)
    let has_orderbook = session.orderbook_file.is_some() || session.incremental_l2_file.is_some();
    if session.trades_file.is_none() || !has_orderbook {
        let missing = if session.trades_file.is_none() && !has_orderbook {
            "trades and orderbook files"
        } else if session.trades_file.is_none() {
            "trades file"
        } else {
            "orderbook file (or incremental_l2 file)"
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
    // Use incremental_l2_file if available, otherwise orderbook_file
    let orderbook_file = session.incremental_l2_file.as_ref()
        .or(session.orderbook_file.as_ref())
        .unwrap();
    
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

/// Auto-import uploaded file to database
/// Called automatically after file upload
#[cfg(feature = "postgres")]
async fn auto_import_to_database(
    file_info: &UploadedFileInfo,
    normalized_symbol: &str,
    normalized_exchange: &str,
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
) -> Result<(usize, &'static str), String> {
    eprintln!("🔄 [TARDIS] ====== AUTO-IMPORT START ======");
    eprintln!("   file_type: {:?}", file_info.file_type);
    eprintln!("   symbol: {}", normalized_symbol);
    eprintln!("   exchange: {}", normalized_exchange);
    eprintln!("   stored_path: {}", file_info.stored_path);
    
    // Check if file exists
    let path = std::path::Path::new(&file_info.stored_path);
    if !path.exists() {
        let err = format!("File does not exist: {}", file_info.stored_path);
        eprintln!("❌ [TARDIS] {}", err);
        return Err(err);
    }
    eprintln!("✅ [TARDIS] File exists at path");
    
    // Read file data
    let file_data = tokio::fs::read(&file_info.stored_path).await
        .map_err(|e| {
            eprintln!("❌ [TARDIS] Failed to read file: {}", e);
            format!("Failed to read file: {}", e)
        })?;
    
    eprintln!("📖 [TARDIS] Read {} bytes from {}", file_data.len(), file_info.stored_path);
    
    // Check if gzipped
    let is_gzipped = file_data.len() >= 2 && file_data[0] == 0x1f && file_data[1] == 0x8b;
    eprintln!("📦 [TARDIS] File is gzipped: {}", is_gzipped);
    
    // Get or create exchange
    eprintln!("🔄 [TARDIS] Creating/finding exchange record...");
    let exchange_record = exchange_ops::create_exchange(
        pool.clone(),
        NewExchange { exchange: normalized_exchange.to_string() }
    ).await.map_err(|e| {
        eprintln!("❌ [TARDIS] Exchange creation failed: {}", e);
        format!("Exchange error: {}", e)
    })?;
    let exchange_id = exchange_record.exchange_id;
    eprintln!("✅ [TARDIS] Created/found exchange '{}' with id {}", normalized_exchange, exchange_id);

    // Get or create security
    eprintln!("🔄 [TARDIS] Creating/finding security record...");
    let security_record = securities_ops::create_security(
        pool.clone(),
        NewSecurity { symbol: normalized_symbol.to_string() }
    ).await.map_err(|e| {
        eprintln!("❌ [TARDIS] Security creation failed: {}", e);
        format!("Security error: {}", e)
    })?;
    let security_id = security_record.security_id;
    eprintln!("✅ [TARDIS] Created/found security '{}' with id {}", normalized_symbol, security_id);

    // Get or create order book
    eprintln!("🔄 [TARDIS] Creating/finding orderbook record...");
    let order_book_record = order_book_ops::create_orderbook(
        pool.clone(),
        NewOrderBook {
            symbol: normalized_symbol.to_string(),
            exchange: normalized_exchange.to_string(),
            security_id,
            exchange_id,
        }
    ).await.map_err(|e| {
        eprintln!("❌ [TARDIS] Orderbook creation failed: {}", e);
        format!("Order book error: {}", e)
    })?;
    eprintln!("✅ [TARDIS] Created/found orderbook with id {}", order_book_record.order_book_id);
    
    eprintln!(
        "📊 [TARDIS] Using order_book_id: {}, security_id: {}, exchange_id: {} for auto-import",
        order_book_record.order_book_id, security_id, exchange_id
    );

    // Import based on file type
    eprintln!("🔀 [TARDIS] Dispatching to handler for file_type: {:?}", file_info.file_type);
    
    match file_info.file_type {
        TardisFileType::Trades => {
            eprintln!("📈 [TARDIS] Calling parse_and_insert_trades...");
            let result = parse_and_insert_trades(
                &file_data,
                normalized_symbol,
                normalized_exchange,
                security_id,
                exchange_id,
                pool,
            ).await;
            
            match result {
                Ok((trades_inserted, rows_processed)) => {
                    eprintln!("✅ [TARDIS] parse_and_insert_trades returned Ok: trades_inserted={}, rows_processed={}", trades_inserted, rows_processed);
                    Ok((trades_inserted, "trades"))
                }
                Err(e) => {
                    eprintln!("❌ [TARDIS] parse_and_insert_trades returned Err: {}", e);
                    Err(e)
                }
            }
        }
        TardisFileType::Orderbook => {
            let (snapshots_inserted, _rows_processed) = parse_and_insert_l2_orderbook(
                &file_data,
                normalized_symbol,
                normalized_exchange,
                security_id,
                exchange_id,
                pool,
            ).await?;
            Ok((snapshots_inserted, "L2 snapshots"))
        }
        TardisFileType::IncrementalL2 => {
            let (orders_inserted, _rows_processed) = parse_and_insert_incremental_l2(
                &file_data,
                normalized_symbol,
                normalized_exchange,
                security_id,
                exchange_id,
                pool,
            ).await?;
            Ok((orders_inserted, "incremental L2 updates"))
        }
    }
}

/// Parse and insert trades from Tardis CSV into the database
/// Trades format: exchange,symbol,timestamp,local_timestamp,id,side,price,amount
#[cfg(feature = "postgres")]
async fn parse_and_insert_trades(
    data: &[u8],
    symbol: &str,
    exchange: &str,
    security_id: uuid::Uuid,
    exchange_id: uuid::Uuid,
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
) -> Result<(usize, usize), String> {
    eprintln!("📝 [TARDIS] parse_and_insert_trades called:");
    eprintln!("   data_len: {} bytes", data.len());
    eprintln!("   symbol: {}", symbol);
    eprintln!("   exchange: {}", exchange);
    eprintln!("   security_id: {}", security_id);
    eprintln!("   exchange_id: {}", exchange_id);
    
    // Decompress if gzipped
    let is_gzipped = data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b;
    eprintln!("📦 [TARDIS] Data is gzipped: {}", is_gzipped);
    
    let reader: Box<dyn Read + Send> = if is_gzipped {
        Box::new(GzDecoder::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    
    let buf_reader = BufReader::new(reader);
    let mut lines = buf_reader.lines();
    
    // Read header
    eprintln!("📋 [TARDIS] Reading header line...");
    let header = lines.next()
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] File is empty (no header line)");
            "Empty file"
        })?
        .map_err(|e| {
            eprintln!("❌ [TARDIS] Error reading header: {}", e);
            format!("Read error: {}", e)
        })?;
    
    eprintln!("📋 [TARDIS] Header line: {}", header);
    
    let header_fields: Vec<&str> = header.split(',').collect();
    eprintln!("📋 [TARDIS] Header has {} columns: {:?}", header_fields.len(), header_fields);
    
    // Find column indices
    let timestamp_idx = header_fields.iter().position(|&f| f == "timestamp")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'timestamp' column in header");
            "Missing 'timestamp' column"
        })?;
    let id_idx = header_fields.iter().position(|&f| f == "id")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'id' column in header");
            "Missing 'id' column"
        })?;
    let side_idx = header_fields.iter().position(|&f| f == "side")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'side' column in header");
            "Missing 'side' column"
        })?;
    let price_idx = header_fields.iter().position(|&f| f == "price")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'price' column in header");
            "Missing 'price' column"
        })?;
    let amount_idx = header_fields.iter().position(|&f| f == "amount")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'amount' column in header");
            "Missing 'amount' column"
        })?;
    
    eprintln!("📊 [TARDIS] Column indices - timestamp: {}, id: {}, side: {}, price: {}, amount: {}",
        timestamp_idx, id_idx, side_idx, price_idx, amount_idx);
    
    let mut rows_processed = 0;
    let mut trades_inserted = 0;
    let mut skipped_empty = 0;
    let mut skipped_parse_error = 0;
    let mut skipped_fields = 0;
    
    // Batch for efficient inserts
    const BATCH_SIZE: usize = 5000;
    let mut trade_batch: Vec<NewTrade> = Vec::with_capacity(BATCH_SIZE);
    let mut historical_order_batch: Vec<NewHistoricalOrder> = Vec::with_capacity(BATCH_SIZE);
    
    eprintln!("🔄 [TARDIS] Starting to parse trades rows...");
    
    for line_result in lines {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("Error reading line {}: {}", rows_processed, e);
                continue;
            }
        };
        
        rows_processed += 1;
        let fields: Vec<&str> = line.split(',').collect();
        
        // Skip if we don't have enough fields
        let max_idx = *[timestamp_idx, id_idx, side_idx, price_idx, amount_idx]
            .iter()
            .max()
            .unwrap();
        if fields.len() <= max_idx {
            skipped_fields += 1;
            if skipped_fields <= 3 {
                eprintln!("⚠️ [TARDIS] Row {}: not enough fields ({} <= {})", rows_processed, fields.len(), max_idx);
            }
            continue;
        }
        
        // Parse trade data
        let timestamp_str = fields[timestamp_idx];
        let trade_id_str = fields[id_idx];
        let side_str = fields[side_idx].to_uppercase();
        let price_str = fields[price_idx];
        let amount_str = fields[amount_idx];
        
        // Log first few rows for debugging
        if rows_processed <= 3 {
            eprintln!("📝 [TARDIS] Row {}: timestamp={}, id={}, side={}, price={}, amount={}",
                rows_processed, timestamp_str, trade_id_str, side_str, price_str, amount_str);
        }
        
        // Skip empty values
        if price_str.is_empty() || amount_str.is_empty() {
            skipped_empty += 1;
            continue;
        }
        
        // Parse timestamp (microseconds from epoch)
        let timestamp = match timestamp_str.parse::<i64>() {
            Ok(ts_micros) => Utc.timestamp_micros(ts_micros).single(),
            Err(e) => {
                skipped_parse_error += 1;
                if skipped_parse_error <= 3 {
                    eprintln!("⚠️ [TARDIS] Row {}: timestamp parse error: {} (value='{}')", rows_processed, e, timestamp_str);
                }
                None
            },
        };
        let timestamp = match timestamp {
            Some(ts) => ts,
            None => continue,
        };
        
        // Parse price and quantity
        let price = match BigDecimal::from_str(price_str) {
            Ok(p) => p,
            Err(e) => {
                skipped_parse_error += 1;
                if skipped_parse_error <= 3 {
                    eprintln!("⚠️ [TARDIS] Row {}: price parse error: {} (value='{}')", rows_processed, e, price_str);
                }
                continue;
            }
        };
        let quantity = match BigDecimal::from_str(amount_str) {
            Ok(q) => q,
            Err(e) => {
                skipped_parse_error += 1;
                if skipped_parse_error <= 3 {
                    eprintln!("⚠️ [TARDIS] Row {}: amount parse error: {} (value='{}')", rows_processed, e, amount_str);
                }
                continue;
            }
        };
        
        // Normalize side to "BUY" or "SELL"
        let normalized_side = if side_str.contains("BUY") || side_str == "B" {
            "BUY".to_string()
        } else if side_str.contains("SELL") || side_str == "S" || side_str == "ASK" {
            "SELL".to_string()
        } else {
            side_str.clone()
        };
        
        // Add to trades table batch (for backwards compatibility)
        trade_batch.push(NewTrade {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            trade_id: trade_id_str.to_string(),
            security_id,
            exchange_id,
            side: normalized_side.clone(),
            price: price.clone(),
            quantity: quantity.clone(),
        });
        
        // Also add to historical_orders with event_type='trade' for candle aggregation
        historical_order_batch.push(NewHistoricalOrder::new(
            timestamp,
            trade_id_str,
            "trade",
            &normalized_side,
            &price,
            &quantity,
            None,
            None,
            "filled",
            exchange,
            symbol,
            exchange_id,
            security_id,
        ));
        
        // Insert batch when full
        if trade_batch.len() >= BATCH_SIZE {
            let batch_len = trade_batch.len();
            
            eprintln!("💾 [TARDIS] Inserting batch of {} trades...", batch_len);
            
            // Insert into trades table
            trades_ops::create_trades(pool.clone(), std::mem::take(&mut trade_batch))
                .await
                .map_err(|e| {
                    eprintln!("❌ [TARDIS] trades table insert failed: {}", e);
                    format!("Database insert error (trades): {}", e)
                })?;
            
            // Insert into historical_orders for candle aggregation
            historical_order_ops::create_historical_orders(pool.clone(), std::mem::take(&mut historical_order_batch))
                .await
                .map_err(|e| {
                    eprintln!("❌ [TARDIS] historical_orders table insert failed: {}", e);
                    format!("Database insert error (historical_orders): {}", e)
                })?;
            
            trades_inserted += batch_len;
            
            if trades_inserted % 50000 == 0 {
                eprintln!("📊 [TARDIS] Progress: {} trades inserted so far...", trades_inserted);
            }
        }
    }
    
    eprintln!("📊 [TARDIS] Parsing complete. rows_processed={}, pending_batch={}", rows_processed, trade_batch.len());
    eprintln!("   skipped_empty={}, skipped_parse_error={}, skipped_fields={}", skipped_empty, skipped_parse_error, skipped_fields);
    
    // Insert remaining batch
    if !trade_batch.is_empty() {
        let batch_len = trade_batch.len();
        eprintln!("💾 [TARDIS] Inserting final batch of {} trades...", batch_len);
        
        trades_ops::create_trades(pool.clone(), trade_batch)
            .await
            .map_err(|e| {
                eprintln!("❌ [TARDIS] Final trades table insert failed: {}", e);
                format!("Database insert error (trades): {}", e)
            })?;
        
        historical_order_ops::create_historical_orders(pool.clone(), historical_order_batch)
            .await
            .map_err(|e| {
                eprintln!("❌ [TARDIS] Final historical_orders table insert failed: {}", e);
                format!("Database insert error (historical_orders): {}", e)
            })?;
        
        trades_inserted += batch_len;
    }
    
    eprintln!("✅ [TARDIS] ====== IMPORT COMPLETE ======");
    eprintln!("   Total rows processed: {}", rows_processed);
    eprintln!("   Total trades inserted: {}", trades_inserted);
    eprintln!("   Skipped (empty): {}", skipped_empty);
    eprintln!("   Skipped (parse error): {}", skipped_parse_error);
    eprintln!("   Skipped (not enough fields): {}", skipped_fields);
    
    Ok((trades_inserted, rows_processed))
}

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

    // Need orderbook file for import - prefer incremental L2 over L2 snapshots
    let (file_to_import, is_incremental) = if let Some(ref f) = session.incremental_l2_file {
        (f.clone(), true)
    } else if let Some(ref f) = session.orderbook_file {
        (f.clone(), false)
    } else {
        return Err((StatusCode::BAD_REQUEST, Json(TardisImportResponse {
            success: false,
            message: "No orderbook file uploaded".to_string(),
            symbol: Some(session.symbol.clone()),
            exchange: Some(session.exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some("Upload an orderbook or incremental_book_L2 CSV file first".to_string()),
        })));
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

    // Clear existing data if requested
    if request.clear_existing {
        if is_incremental {
            // Clear historical_orders for this symbol/exchange
            match historical_order_ops::delete_historical_orders_by_symbol_exchange(
                pool.clone(),
                &normalized_symbol,
                &normalized_exchange,
            ).await {
                Ok(count) => tracing::info!("Cleared {} historical orders", count),
                Err(e) => tracing::warn!("Failed to clear historical orders: {}", e),
            }
        } else {
            // Clear L2 snapshots
            match l2_orderbook_snapshot_ops::delete_l2_snapshots_by_symbol_exchange(
                pool.clone(),
                &normalized_symbol,
                &normalized_exchange,
            ).await {
                Ok(count) => tracing::info!("Cleared {} L2 snapshots", count),
                Err(e) => tracing::warn!("Failed to clear L2 snapshots: {}", e),
            }
        }
    }

    // Read file data
    let file_data = tokio::fs::read(&file_to_import.stored_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
            success: false,
            message: "Failed to read file".to_string(),
            symbol: Some(normalized_symbol.clone()),
            exchange: Some(normalized_exchange.clone()),
            buy_orders_inserted: 0,
            sell_orders_inserted: 0,
            rows_processed: 0,
            error: Some(format!("File read error: {}", e)),
        }))
    })?;

    // Parse and import based on file type
    if is_incremental {
        // Parse incremental_book_L2 and insert into historical_orders
        tracing::info!("Importing incremental_book_L2 data to historical_orders table");
        let (orders_inserted, rows_processed) = parse_and_insert_incremental_l2(
            &file_data,
            &normalized_symbol,
            &normalized_exchange,
            security_id,
            exchange_id,
            pool.clone(),
        ).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(TardisImportResponse {
                success: false,
                message: "Failed to parse incremental L2".to_string(),
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
                "Successfully imported {} order book updates from {} rows (incremental L2)",
                orders_inserted, rows_processed
            ),
            symbol: Some(normalized_symbol),
            exchange: Some(normalized_exchange),
            buy_orders_inserted: orders_inserted,
            sell_orders_inserted: 0,
            rows_processed,
            error: None,
        }))
    } else {
        // Parse L2 snapshots and insert into l2_orderbook_snapshots
        tracing::info!("Importing L2 snapshot data to l2_orderbook_snapshots table");
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
            buy_orders_inserted: snapshots_inserted,
            sell_orders_inserted: 0,
            rows_processed,
            error: None,
        }))
    }
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
    eprintln!("📝 [TARDIS] parse_and_insert_l2_orderbook called:");
    eprintln!("   data_len: {} bytes", data.len());
    eprintln!("   symbol: {}", symbol);
    eprintln!("   exchange: {}", exchange);
    eprintln!("   security_id: {}", security_id);
    eprintln!("   exchange_id: {}", exchange_id);
    
    // Decompress if gzipped
    let is_gzipped = data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b;
    eprintln!("📦 [TARDIS] L2 Orderbook data is gzipped: {}", is_gzipped);
    
    let reader: Box<dyn Read + Send> = if is_gzipped {
        Box::new(GzDecoder::new(std::io::Cursor::new(data)))
    } else {
        Box::new(std::io::Cursor::new(data))
    };
    
    let buf_reader = BufReader::new(reader);
    let mut lines = buf_reader.lines();
    
    // Read header
    eprintln!("📋 [TARDIS] Reading L2 orderbook header...");
    let header = lines.next()
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] L2 orderbook file is empty");
            "Empty file"
        })?
        .map_err(|e| {
            eprintln!("❌ [TARDIS] Error reading L2 orderbook header: {}", e);
            format!("Read error: {}", e)
        })?;
    
    eprintln!("📋 [TARDIS] L2 header (first 200 chars): {}...", &header[..std::cmp::min(200, header.len())]);
    
    let header_fields: Vec<&str> = header.split(',').collect();
    eprintln!("📋 [TARDIS] L2 header has {} columns", header_fields.len());
    
    // Find column indices for asks and bids (0-24 levels each)
    let timestamp_idx = header_fields.iter().position(|&f| f == "timestamp")
        .ok_or_else(|| {
            eprintln!("❌ [TARDIS] Missing 'timestamp' column in L2 orderbook");
            "Missing 'timestamp' column"
        })?;
    eprintln!("📊 [TARDIS] timestamp column at index {}", timestamp_idx);
    
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
        eprintln!("❌ [TARDIS] No ask/bid columns found! ask_indices={}, bid_indices={}", ask_indices.len(), bid_indices.len());
        eprintln!("   First 10 columns: {:?}", &header_fields[..std::cmp::min(10, header_fields.len())]);
        return Err("No ask/bid columns found in orderbook CSV".to_string());
    }
    
    eprintln!("📊 [TARDIS] Found {} ask levels and {} bid levels", ask_indices.len(), bid_indices.len());
    
    let mut rows_processed = 0;
    let mut rows_skipped = 0;
    let mut total_snapshots = 0;
    let mut total_historical_orders = 0;
    
    // Batch for efficient inserts - max batch sizes for fastest import
    const BATCH_SIZE: usize = 10000;          // L2 snapshots
    const HISTORICAL_BATCH_SIZE: usize = 100000;  // Historical orders (~30MB per batch)
    let mut snapshot_batch: Vec<NewL2OrderbookSnapshot> = Vec::with_capacity(BATCH_SIZE);
    let mut historical_order_batch: Vec<NewHistoricalOrder> = Vec::with_capacity(HISTORICAL_BATCH_SIZE);
    
    eprintln!("🔄 [TARDIS] Starting L2 orderbook row parsing...");
    
    for line_result in lines {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                rows_skipped += 1;
                if rows_skipped <= 3 {
                    eprintln!("⚠️ [TARDIS] L2 row read error: {}", e);
                }
                continue;
            }
        };
        
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() <= timestamp_idx {
            rows_skipped += 1;
            if rows_skipped <= 3 {
                eprintln!("⚠️ [TARDIS] L2 row has too few fields: {} <= {}", fields.len(), timestamp_idx);
            }
            continue;
        }
        
        // Parse timestamp (microseconds)
        let timestamp_us: i64 = match fields[timestamp_idx].parse() {
            Ok(ts) => ts,
            Err(e) => {
                rows_skipped += 1;
                if rows_skipped <= 3 {
                    eprintln!("⚠️ [TARDIS] L2 timestamp parse error: {} (value='{}')", e, fields[timestamp_idx]);
                }
                continue;
            }
        };
        
        let timestamp = match Utc.timestamp_micros(timestamp_us).single() {
            Some(dt) => dt,
            None => {
                rows_skipped += 1;
                continue;
            }
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
            bid_prices.clone(),
            bid_quantities.clone(),
            ask_prices.clone(),
            ask_quantities.clone(),
        ));
        
        // Insert batch when it reaches the limit
        if snapshot_batch.len() >= BATCH_SIZE {
            let batch_len = snapshot_batch.len();
            eprintln!("💾 [TARDIS] Inserting L2 snapshot batch of {} records...", batch_len);
            let batch_to_insert = std::mem::take(&mut snapshot_batch);
            match l2_orderbook_snapshot_ops::create_l2_snapshots(pool.clone(), batch_to_insert).await {
                Ok(count) => {
                    total_snapshots += count;
                    eprintln!("✅ [TARDIS] L2 snapshot batch inserted: {} records", count);
                },
                Err(e) => eprintln!("❌ [TARDIS] L2 snapshot batch insert error: {}", e),
            }
        }
        
        // ALSO insert into historical_orders for backtesting queries
        // Each price level becomes a historical_order entry with event_type="snapshot"
        // Order ID is deterministic: symbol_timestamp_side_level_price (enables idempotent re-imports)
        for (i, (price, qty)) in bid_prices.iter().zip(bid_quantities.iter()).enumerate() {
            historical_order_batch.push(NewHistoricalOrder::new(
                timestamp,
                &format!("{}_{}_bid_{}_{}", symbol, timestamp.timestamp_micros(), i, price),
                "snapshot",
                "buy",
                price,
                qty,
                None,
                None,
                "open",
                exchange,
                symbol,
                exchange_id,
                security_id,
            ));
        }
        for (i, (price, qty)) in ask_prices.iter().zip(ask_quantities.iter()).enumerate() {
            historical_order_batch.push(NewHistoricalOrder::new(
                timestamp,
                &format!("{}_{}_ask_{}_{}", symbol, timestamp.timestamp_micros(), i, price),
                "snapshot",
                "sell",
                price,
                qty,
                None,
                None,
                "open",
                exchange,
                symbol,
                exchange_id,
                security_id,
            ));
        }
        
        // Insert historical_order batch when it reaches the limit
        if historical_order_batch.len() >= HISTORICAL_BATCH_SIZE {
            let batch_len = historical_order_batch.len();
            eprintln!("💾 [TARDIS] Inserting historical order batch of {} records...", batch_len);
            let batch_to_insert = std::mem::take(&mut historical_order_batch);
            match historical_order_ops::create_historical_orders(pool.clone(), batch_to_insert).await {
                Ok(inserted) => {
                    total_historical_orders += inserted.len();
                    eprintln!("✅ [TARDIS] Historical order batch inserted: {} records", inserted.len());
                },
                Err(e) => eprintln!("❌ [TARDIS] Historical order batch insert error: {}", e),
            }
        }
        
        // Log progress every 10000 rows
        if rows_processed % 10000 == 0 {
            eprintln!("📊 [TARDIS] L2 Progress: {} rows processed, {} snapshots, {} historical orders",
                rows_processed, total_snapshots, total_historical_orders);
        }
    }
    
    eprintln!("📊 [TARDIS] L2 parsing complete. rows_processed={}, pending_snapshots={}, pending_historical={}",
        rows_processed, snapshot_batch.len(), historical_order_batch.len());
    
    // Insert remaining snapshot batch
    if !snapshot_batch.is_empty() {
        let batch_len = snapshot_batch.len();
        eprintln!("💾 [TARDIS] Inserting final L2 snapshot batch of {} records...", batch_len);
        match l2_orderbook_snapshot_ops::create_l2_snapshots(pool.clone(), snapshot_batch).await {
            Ok(count) => {
                total_snapshots += count;
                eprintln!("✅ [TARDIS] Final L2 snapshot batch inserted: {} records", count);
            },
            Err(e) => eprintln!("❌ [TARDIS] Final L2 snapshot batch insert error: {}", e),
        }
    }
    
    // Insert remaining historical order batch
    if !historical_order_batch.is_empty() {
        let batch_len = historical_order_batch.len();
        eprintln!("💾 [TARDIS] Inserting final historical order batch of {} records...", batch_len);
        match historical_order_ops::create_historical_orders(pool.clone(), historical_order_batch).await {
            Ok(inserted) => {
                total_historical_orders += inserted.len();
                eprintln!("✅ [TARDIS] Final historical order batch inserted: {} records", inserted.len());
            },
            Err(e) => eprintln!("❌ [TARDIS] Final historical order batch insert error: {}", e),
        }
    }
    
    eprintln!("✅ [TARDIS] ====== L2 ORDERBOOK IMPORT COMPLETE ======");
    eprintln!("   Total rows processed: {}", rows_processed);
    eprintln!("   Total L2 snapshots inserted: {}", total_snapshots);
    eprintln!("   Total historical orders inserted: {}", total_historical_orders);
    eprintln!("   Rows skipped: {}", rows_skipped);
    
    Ok((total_snapshots, rows_processed))
}

/// Parse incremental_book_L2 CSV and insert into historical_orders table
/// Tardis incremental_book_L2 format:
/// exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount
#[cfg(feature = "postgres")]
async fn parse_and_insert_incremental_l2(
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
    
    // Find column indices
    let timestamp_idx = header_fields.iter().position(|&f| f == "timestamp")
        .ok_or("Missing 'timestamp' column")?;
    let is_snapshot_idx = header_fields.iter().position(|&f| f == "is_snapshot")
        .ok_or("Missing 'is_snapshot' column")?;
    let side_idx = header_fields.iter().position(|&f| f == "side")
        .ok_or("Missing 'side' column")?;
    let price_idx = header_fields.iter().position(|&f| f == "price")
        .ok_or("Missing 'price' column")?;
    let amount_idx = header_fields.iter().position(|&f| f == "amount")
        .ok_or("Missing 'amount' column")?;
    
    tracing::info!(
        "Parsing incremental_book_L2: timestamp={}, is_snapshot={}, side={}, price={}, amount={}",
        timestamp_idx, is_snapshot_idx, side_idx, price_idx, amount_idx
    );
    
    let mut rows_processed = 0;
    let mut total_orders = 0;
    let mut order_counter: u64 = 0; // Synthetic order ID counter
    
    // Batch for efficient inserts
    const BATCH_SIZE: usize = 5000;
    let mut order_batch: Vec<NewHistoricalOrder> = Vec::with_capacity(BATCH_SIZE);
    
    for line_result in lines {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() <= amount_idx {
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
        
        // Parse is_snapshot (true = initial snapshot, false = delta update)
        let is_snapshot: bool = fields[is_snapshot_idx].parse().unwrap_or(false);
        
        // Parse side (bid/ask) - convert to buy/sell for database constraint
        let side_str = fields[side_idx].to_lowercase();
        let side = if side_str == "bid" || side_str == "buy" {
            "buy"
        } else if side_str == "ask" || side_str == "sell" {
            "sell"
        } else {
            continue;
        };
        
        // Parse price
        let price_str = fields[price_idx].trim();
        let price = match BigDecimal::from_str(price_str) {
            Ok(p) if p > BigDecimal::from(0) => p,
            _ => continue,
        };
        
        // Parse amount (0 = level deleted)
        let amount_str = fields[amount_idx].trim();
        let quantity = match BigDecimal::from_str(amount_str) {
            Ok(q) if q >= BigDecimal::from(0) => q,
            _ => continue,
        };
        
        rows_processed += 1;
        order_counter += 1;
        
        // Determine event type based on quantity
        // quantity == 0 means the level was removed
        // is_snapshot means initial order book state
        // otherwise it's a modify/add
        let event_type = if is_snapshot {
            "snapshot"
        } else if quantity == BigDecimal::from(0) {
            "cancel"
        } else {
            "modify" // Could be add or modify - we can't distinguish without tracking state
        };
        
        // Create historical order entry
        // We use a synthetic order_id based on price level + side since Tardis L2 data
        // doesn't have actual order IDs (it's L2 aggregated data, not L3)
        let synthetic_order_id = format!("{}_{}_{}_{}", symbol, side, price_str, order_counter);
        
        order_batch.push(NewHistoricalOrder::new(
            timestamp,
            &synthetic_order_id,
            event_type,
            side,
            &price,
            &quantity,
            None, // prev_price
            None, // prev_quantity
            if quantity == BigDecimal::from(0) { "cancelled" } else { "open" },
            exchange,
            symbol,
            exchange_id,
            security_id,
        ));
        
        // Insert batch when it reaches the limit
        if order_batch.len() >= BATCH_SIZE {
            let batch_to_insert = std::mem::take(&mut order_batch);
            match historical_order_ops::create_historical_orders(pool.clone(), batch_to_insert).await {
                Ok(inserted) => total_orders += inserted.len(),
                Err(e) => tracing::warn!("Historical order batch insert error: {}", e),
            }
        }
        
        // Log progress every 50000 rows
        if rows_processed % 50000 == 0 {
            tracing::info!(
                "Progress: {} rows, {} orders inserted",
                rows_processed, total_orders
            );
        }
    }
    
    // Insert remaining batch
    if !order_batch.is_empty() {
        match historical_order_ops::create_historical_orders(pool.clone(), order_batch).await {
            Ok(inserted) => total_orders += inserted.len(),
            Err(e) => tracing::warn!("Final historical order batch insert error: {}", e),
        }
    }
    
    tracing::info!(
        "Import complete: {} rows processed, {} historical orders inserted",
        rows_processed, total_orders
    );
    
    Ok((total_orders, rows_processed))
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
    // Database format:    "Binance", "Kraken" (display format, max 8 chars)
    // ============================================================================

    #[test]
    fn test_normalize_exchange_binance_us() {
        // binance-us normalizes to "Binance" to fit VARCHAR(8)
        assert_eq!(normalize_exchange("binance-us"), "Binance");
        assert_eq!(normalize_exchange("binanceus"), "Binance");
        assert_eq!(normalize_exchange("BINANCE-US"), "Binance");
        assert_eq!(normalize_exchange("BinanceUS"), "Binance");
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
    fn test_parse_file_type_from_filename_trades() {
        assert_eq!(parse_file_type_from_filename("BTCUSD_trades.csv.gz"), Some(TardisFileType::Trades));
        assert_eq!(parse_file_type_from_filename("binance-us_BTCUSD_trades_2024-10-01.csv.gz"), Some(TardisFileType::Trades));
        assert_eq!(parse_file_type_from_filename("trade_data.csv"), Some(TardisFileType::Trades));
    }

    #[test]
    fn test_parse_file_type_from_filename_orderbook() {
        assert_eq!(parse_file_type_from_filename("BTCUSD_book_snapshot_25.csv.gz"), Some(TardisFileType::Orderbook));
        assert_eq!(parse_file_type_from_filename("binance-us_BTCUSD_book_snapshot_5_2024-10-01.csv.gz"), Some(TardisFileType::Orderbook));
        assert_eq!(parse_file_type_from_filename("snapshot_data.csv"), Some(TardisFileType::Orderbook));
    }

    #[test]
    fn test_parse_file_type_from_filename_incremental_l2() {
        assert_eq!(parse_file_type_from_filename("BTCUSD_incremental_book_L2.csv.gz"), Some(TardisFileType::IncrementalL2));
        assert_eq!(parse_file_type_from_filename("binance_incremental-book-l2_2024-10-01.csv.gz"), Some(TardisFileType::IncrementalL2));
        assert_eq!(parse_file_type_from_filename("incremental_book_l2_data.csv"), Some(TardisFileType::IncrementalL2));
    }

    #[test]
    fn test_parse_file_type_from_filename_unknown() {
        // Unknown files should return None (will auto-detect from CSV headers)
        assert_eq!(parse_file_type_from_filename("BTCUSD_data.csv"), None);
        assert_eq!(parse_file_type_from_filename("random_file.txt"), None);
    }

    #[test]
    fn test_detect_file_type_from_headers() {
        // L2 snapshot headers
        let l2_header = "exchange,symbol,timestamp,local_timestamp,asks[0].price,asks[0].amount,bids[0].price,bids[0].amount";
        assert_eq!(detect_file_type_from_headers(l2_header), Some(TardisFileType::Orderbook));
        
        // Incremental L2 headers
        let incremental_header = "exchange,symbol,timestamp,local_timestamp,is_snapshot,side,price,amount";
        assert_eq!(detect_file_type_from_headers(incremental_header), Some(TardisFileType::IncrementalL2));
        
        // Trades headers
        let trades_header = "exchange,symbol,timestamp,local_timestamp,id,side,price,amount";
        assert_eq!(detect_file_type_from_headers(trades_header), Some(TardisFileType::Trades));
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
        assert_eq!(normalize_exchange(tardis_exchange), "Binance");
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
