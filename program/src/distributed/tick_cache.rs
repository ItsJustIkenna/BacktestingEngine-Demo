//! Tick cache for efficient data loading
//!
//! Provides a unified interface for loading simulation ticks from either:
//! - Memory-mapped binary files (preferred, zero-copy)
//! - Database via streaming (fallback)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use data_prep::{BinaryFileReader, SimulationTick};
use dataloader::MarketData;

/// Source of tick data
pub enum TickSource {
    /// Memory-mapped binary file (zero-copy, preferred)
    BinaryFile(BinaryFileReader),
    /// In-memory vector (legacy, from DB load)
    InMemory(Arc<Vec<SimulationTick>>),
}

impl TickSource {
    /// Get ticks as a slice
    pub fn ticks(&self) -> &[SimulationTick] {
        match self {
            TickSource::BinaryFile(reader) => reader.ticks(),
            TickSource::InMemory(vec) => vec.as_slice(),
        }
    }
    
    /// Get tick count
    pub fn len(&self) -> usize {
        match self {
            TickSource::BinaryFile(reader) => reader.tick_count() as usize,
            TickSource::InMemory(vec) => vec.len(),
        }
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Convert ticks to MarketData for compatibility with existing simulation loop
    /// 
    /// Note: This allocates new memory. For maximum efficiency, update the
    /// simulation loop to work directly with &[SimulationTick].
    pub fn to_market_data(&self) -> Vec<MarketData> {
        self.ticks()
            .iter()
            .map(|tick| {
                let value = tick.price * tick.quantity;
                let trade = dataloader::TradeData {
                    timestamp: tick.timestamp(),
                    symbol: String::new(), // Will be set by context
                    exchange: String::new(),
                    price: bigdecimal::BigDecimal::from(tick.price as i64), // Lossy but ok for now
                    price_f64: tick.price,
                    quantity: bigdecimal::BigDecimal::from(tick.quantity as i64),
                    quantity_f64: tick.quantity,
                    side: if tick.is_sell() { "sell".to_string() } else { "buy".to_string() },
                    trade_id: String::new(),
                    value_f64: value,
                };
                MarketData::Trade(trade)
            })
            .collect()
    }
}

/// Cache for simulation tick data
/// 
/// Manages loading and caching of tick data from binary files or database.
/// Binary files are preferred as they provide zero-copy memory-mapped access.
pub struct TickCache {
    /// Directory containing binary data files
    data_dir: PathBuf,
    /// Loaded tick sources by cache key
    sources: std::collections::HashMap<String, TickSource>,
}

/// Error returned when loading fails
#[derive(Debug)]
pub enum TickCacheError {
    /// Binary file not found
    FileNotFound(String),
    /// IO error during read
    IoError(std::io::Error),
    /// File format error  
    FormatError(String),
}

impl std::fmt::Display for TickCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TickCacheError::FileNotFound(msg) => write!(f, "File not found: {}", msg),
            TickCacheError::IoError(e) => write!(f, "IO error: {}", e),
            TickCacheError::FormatError(msg) => write!(f, "Format error: {}", msg),
        }
    }
}

impl std::error::Error for TickCacheError {}

impl From<std::io::Error> for TickCacheError {
    fn from(e: std::io::Error) -> Self {
        TickCacheError::IoError(e)
    }
}

impl TickCache {
    /// Create a new tick cache
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            sources: std::collections::HashMap::new(),
        }
    }
    
    /// Create with default data directory
    pub fn with_default_dir() -> Self {
        let dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        Self::new(dir)
    }
    
    /// Generate cache key from symbol and exchange
    pub fn cache_key(symbol: &str, exchange: &str) -> String {
        format!("{}_{}", symbol.replace('/', "_"), exchange)
    }
    
    /// Generate binary file path for a symbol/exchange
    fn binary_path(&self, symbol: &str, exchange: &str) -> PathBuf {
        let filename = format!("{}_{}.bin", symbol.replace('/', "_"), exchange);
        self.data_dir.join(filename)
    }
    
    /// Get the binary file path as a string (for ChunkedSimulator)
    pub fn get_binary_path(&self, symbol: &str, exchange: &str) -> String {
        self.binary_path(symbol, exchange).to_string_lossy().to_string()
    }
    
    /// Probe a binary file to get tick count without loading data
    /// 
    /// This is useful to check file existence and size before deciding
    /// whether to use ChunkedSimulator (for large files) or load into memory.
    pub fn probe(&self, symbol: &str, exchange: &str) -> Result<usize, TickCacheError> {
        let binary_path = self.binary_path(symbol, exchange);
        
        if !binary_path.exists() {
            return Err(TickCacheError::FileNotFound(format!(
                "No binary file found for {}/{} at {}",
                symbol, exchange, binary_path.display()
            )));
        }
        
        // Read just the header to get tick count (avoid loading full file)
        let reader = BinaryFileReader::open(&binary_path)
            .map_err(|e| TickCacheError::FormatError(e.to_string()))?;
        
        Ok(reader.tick_count() as usize)
    }
    
    /// Load ticks for a symbol/exchange
    /// 
    /// Tries binary file first, falls back to database if needed.
    pub fn load(&mut self, symbol: &str, exchange: &str) -> Result<&TickSource, TickCacheError> {
        let key = Self::cache_key(symbol, exchange);
        
        // Check if already loaded
        if self.sources.contains_key(&key) {
            return Ok(self.sources.get(&key).unwrap());
        }
        
        // Try binary file first
        let binary_path = self.binary_path(symbol, exchange);
        if binary_path.exists() {
            println!("[TICK_CACHE] Loading ticks from binary file: {}", binary_path.display());
            
            let reader = BinaryFileReader::open(&binary_path)
                .map_err(|e| TickCacheError::FormatError(e.to_string()))?;
            let tick_count = reader.tick_count();
            
            println!("[TICK_CACHE] Loaded {} ticks from binary file", tick_count);
            
            self.sources.insert(key.clone(), TickSource::BinaryFile(reader));
            return Ok(self.sources.get(&key).unwrap());
        }
        
        // No binary file found
        Err(TickCacheError::FileNotFound(format!(
            "No binary file found for {}/{} at {}. Run 'prepare-data' first.",
            symbol, exchange, binary_path.display()
        )))
    }
    
    /// Load ticks from an in-memory vector (for legacy DB loading)
    pub fn load_from_memory(&mut self, symbol: &str, exchange: &str, ticks: Vec<SimulationTick>) -> &TickSource {
        let key = Self::cache_key(symbol, exchange);
        self.sources.insert(key.clone(), TickSource::InMemory(Arc::new(ticks)));
        self.sources.get(&key).unwrap()
    }
    
    /// Check if data is already loaded
    pub fn is_loaded(&self, symbol: &str, exchange: &str) -> bool {
        let key = Self::cache_key(symbol, exchange);
        self.sources.contains_key(&key)
    }
    
    /// Get loaded ticks (returns None if not loaded)
    pub fn get(&self, symbol: &str, exchange: &str) -> Option<&TickSource> {
        let key = Self::cache_key(symbol, exchange);
        self.sources.get(&key)
    }
    
    /// Clear all cached data
    pub fn clear(&mut self) {
        self.sources.clear();
    }
    
    /// List available binary files
    pub fn list_available(&self) -> std::io::Result<Vec<(String, String)>> {
        let mut result = Vec::new();
        
        if self.data_dir.exists() {
            for entry in std::fs::read_dir(&self.data_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.extension().map_or(false, |e| e == "bin") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        // Parse filename: SYMBOL_EXCHANGE.bin
                        if let Some(idx) = stem.rfind('_') {
                            let symbol = stem[..idx].replace('_', "/");
                            let exchange = stem[idx+1..].to_string();
                            result.push((symbol, exchange));
                        }
                    }
                }
            }
        }
        
        Ok(result)
    }
}
