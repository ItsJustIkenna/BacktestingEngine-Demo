//! Distributed GA Worker
//!
//! Receives chromosome evaluation requests via MessageBrokerEngine,
//! evaluates them against market data, and returns fitness results.
//!
//! # Data Sources
//!
//! Workers prefer binary files (memory-mapped, zero-copy) for evaluation.
//! Binary files are created by `backtesting-engine prepare-data` and stored
//! on a shared PVC. Falls back to database loading if binary files don't exist.
//!
//! # Memory Efficiency
//!
//! Uses `LightweightSim` which processes data via the `TickSource` trait,
//! enabling backtesting on millions of ticks with constant memory usage (~20 KB).

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use tokio::sync::{RwLock, Semaphore, Mutex};

use dataloader::MarketData;
use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::FitnessResult;

use super::broker_client::BacktestBrokerClient;
use crate::worker::calculate_comprehensive_fitness;
use crate::job_queue::FitnessWeights;
use crate::{log_info_structured, log_debug, log_error_structured};
use crate::logging_facade::MAIN_LOGGER;

/// Maximum concurrent chromosome evaluations per worker
const MAX_CONCURRENT_EVALS: usize = 4;

/// Maximum equity curve points to include in results (prevents 90MB+ payloads)
const MAX_EQUITY_CURVE_POINTS: usize = 1000;

/// Sample an equity curve down to a maximum number of points
/// Preserves first and last points, samples evenly in between
fn sample_equity_curve<T: Clone>(curve: &[T], max_points: usize) -> Vec<T> {
    if curve.len() <= max_points {
        return curve.to_vec();
    }
    
    let mut sampled = Vec::with_capacity(max_points);
    let step = (curve.len() - 1) as f64 / (max_points - 1) as f64;
    
    for i in 0..max_points {
        let idx = (i as f64 * step).round() as usize;
        let idx = idx.min(curve.len() - 1);
        sampled.push(curve[idx].clone());
    }
    
    sampled
}

/// Calculate Sortino ratio from trade returns
/// Sortino only penalizes downside volatility (negative returns)
fn calculate_sortino_ratio(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    
    let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
    
    // Downside deviation: only consider negative returns
    let negative_returns: Vec<f64> = returns.iter()
        .filter(|&&r| r < 0.0)
        .copied()
        .collect();
    
    if negative_returns.is_empty() {
        // No negative returns = very good, but cap at reasonable value
        return mean_return.max(0.0) * 10.0; // Scale up for no downside risk
    }
    
    let downside_variance = negative_returns.iter()
        .map(|r| r * r)
        .sum::<f64>() / negative_returns.len() as f64;
    let downside_deviation = downside_variance.sqrt();
    
    if downside_deviation < 1e-10 {
        return mean_return.max(0.0) * 10.0;
    }
    
    // Annualize (assuming ~252 trading days per year)
    let annualized_return = mean_return * 252.0;
    let annualized_downside_dev = downside_deviation * (252.0_f64).sqrt();
    
    annualized_return / annualized_downside_dev
}

/// Calculate Calmar ratio: annualized return / max drawdown
fn calculate_calmar_ratio(total_return_pct: f64, max_drawdown: f64, _trading_days: f64) -> f64 {
    if max_drawdown < 1e-10 {
        // No drawdown = very good, cap at reasonable value
        return total_return_pct.max(0.0);
    }
    total_return_pct / (max_drawdown * 100.0) // Drawdown is 0-1, return is percent
}

/// Calculate weighted fitness score from multiple metrics
/// Normalizes each metric to a comparable scale before weighting
fn calculate_weighted_fitness(
    sharpe: f64,
    sortino: f64,
    profit_factor: f64,
    win_rate: f64,
    net_profit: f64,
    calmar: f64,
    consistency: f64,
    max_dd: f64,
    weights: &FitnessWeights,
    initial_capital: f64,
) -> f64 {
    // Normalize metrics to 0-1 or similar comparable scales
    // Sharpe: typically -3 to +5, normalize to 0-1 with sigmoid-like mapping
    let norm_sharpe = (sharpe / 3.0).tanh().max(0.0); // Range ~0 to 0.9 for sharpe 0-3
    
    // Sortino: similar to Sharpe but can be higher
    let norm_sortino = (sortino / 4.0).tanh().max(0.0);
    
    // Profit factor: 1.0 is breakeven, typically 1-3 is good
    let norm_pf = ((profit_factor - 1.0) / 2.0).clamp(0.0, 1.0); // 1->0, 2->0.5, 3->1
    
    // Win rate: already 0-1
    let norm_wr = win_rate;
    
    // Net profit: normalize by initial capital (return %)
    let return_pct = net_profit / initial_capital;
    let norm_profit = (return_pct / 0.5).tanh().max(0.0); // 50% return = ~0.46
    
    // Calmar: typically 0.5-3 is good
    let norm_calmar = (calmar / 2.0).tanh().max(0.0);
    
    // Calculate weighted sum
    let weighted_score = 
        norm_sharpe * weights.sharpe +
        norm_sortino * weights.sortino +
        norm_pf * weights.profit_factor +
        norm_wr * weights.win_rate +
        norm_profit * weights.net_profit +
        norm_calmar * weights.calmar;
    
    // Apply consistency multiplier and drawdown penalty
    let dd_factor = 1.0 - max_dd.min(1.0);
    
    weighted_score * consistency * dd_factor
}

/// Cache entry for binary file metadata
pub struct BinaryFileInfo {
    path: PathBuf,
    #[allow(dead_code)]
    tick_count: usize,
    expires_at: Instant,
}

/// Cache entry for L3 order book binary file metadata
#[allow(dead_code)]
pub struct L3BinaryFileInfo {
    path: PathBuf,
    event_count: usize,
    expires_at: Instant,
}

/// Fallback cache for in-memory data (small datasets or no binary file)
struct DataCacheEntry {
    data: Arc<Vec<MarketData>>,
    expires_at: Instant,
}

/// Distributed GA Worker
/// 
/// Processes chromosome evaluation requests from the message broker.
/// Uses memory-mapped binary files for efficient large-dataset evaluation.
pub struct DistributedGAWorker {
    /// Unique worker ID
    worker_id: String,
    /// Broker client for communication
    broker: Arc<BacktestBrokerClient>,
    /// Binary file info by cache key (trade-only, for fast GA evaluations)
    binary_cache: Arc<RwLock<HashMap<String, BinaryFileInfo>>>,
    /// L3 order book binary file info (for realistic final evaluation)
    l3_binary_cache: Arc<RwLock<HashMap<String, L3BinaryFileInfo>>>,
    /// Fallback in-memory data cache (for DB-loaded data)
    data_cache: Arc<RwLock<HashMap<String, DataCacheEntry>>>,
    /// Track which cache keys are currently being loaded
    loading_in_progress: Arc<Mutex<HashSet<String>>>,
    /// Semaphore for limiting concurrent evaluations
    eval_semaphore: Arc<Semaphore>,
    /// Data directory for binary files
    data_dir: PathBuf,
    /// Database pool for loading data if not cached
    #[cfg(feature = "postgres")]
    db_pool: Option<Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>>,
}

impl DistributedGAWorker {
    /// Create a new GA worker
    pub fn new(worker_id: String, broker: Arc<BacktestBrokerClient>) -> Self {
        let data_dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        
        Self {
            worker_id,
            broker,
            binary_cache: Arc::new(RwLock::new(HashMap::new())),
            l3_binary_cache: Arc::new(RwLock::new(HashMap::new())),
            data_cache: Arc::new(RwLock::new(HashMap::new())),
            loading_in_progress: Arc::new(Mutex::new(HashSet::new())),
            eval_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_EVALS)),
            data_dir: PathBuf::from(data_dir),
            #[cfg(feature = "postgres")]
            db_pool: None,
        }
    }
    
    /// Create a new GA worker with shared cache (for multi-worker pods)
    pub fn new_with_shared_state(
        worker_id: String,
        broker: Arc<BacktestBrokerClient>,
        shared_binary_cache: Arc<RwLock<HashMap<String, BinaryFileInfo>>>,
        shared_loading_lock: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        let data_dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        
        Self {
            worker_id,
            broker,
            binary_cache: shared_binary_cache,
            l3_binary_cache: Arc::new(RwLock::new(HashMap::new())),
            data_cache: Arc::new(RwLock::new(HashMap::new())),
            loading_in_progress: shared_loading_lock,
            eval_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_EVALS)),
            data_dir: PathBuf::from(data_dir),
            #[cfg(feature = "postgres")]
            db_pool: None,
        }
    }
    
    /// Set database pool for fallback data loading
    #[cfg(feature = "postgres")]
    pub fn with_db_pool(
        mut self,
        pool: Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>,
    ) -> Self {
        self.db_pool = Some(pool);
        self
    }
    
    /// Get binary file path for a symbol/exchange
    fn binary_path(&self, symbol: &str, exchange: &str) -> PathBuf {
        let filename = format!("{}_{}.bin", symbol.replace('/', "_"), exchange);
        self.data_dir.join(filename)
    }
    
    /// Check if binary file exists and get tick count
    /// Uses OrderBookBinaryReader for unified L3 orderbook format
    fn probe_binary_file(&self, symbol: &str, exchange: &str) -> Option<(PathBuf, usize)> {
        use data_prep::OrderBookBinaryReader;
        
        let path = self.binary_path(symbol, exchange);
        if !path.exists() {
            return None;
        }
        
        // Try unified L3 orderbook format first (new format with all event types)
        match OrderBookBinaryReader::open(&path) {
            Ok(reader) => {
                println!("📂 [WORKER] Found L3 binary file: {} ({} events)", 
                    path.display(), reader.len());
                Some((path, reader.len()))
            }
            Err(e) => {
                println!("⚠️ [WORKER] Failed to open binary file {}: {}", path.display(), e);
                None
            }
        }
    }
    
    /// Evict expired entries from caches to prevent memory leaks
    async fn evict_expired_cache_entries(&self) {
        let now = Instant::now();
        
        // Evict expired binary cache entries
        {
            let mut binary_cache = self.binary_cache.write().await;
            let before = binary_cache.len();
            binary_cache.retain(|_k, v| v.expires_at > now);
            let evicted = before - binary_cache.len();
            if evicted > 0 {
                println!("🧹 [WORKER {}] Evicted {} expired binary cache entries", self.worker_id, evicted);
            }
        }
        
        // Evict expired data cache entries
        {
            let mut data_cache = self.data_cache.write().await;
            let before = data_cache.len();
            data_cache.retain(|_k, v| v.expires_at > now);
            let evicted = before - data_cache.len();
            if evicted > 0 {
                println!("🧹 [WORKER {}] Evicted {} expired data cache entries (freed memory)", self.worker_id, evicted);
            }
        }
    }
    
    /// Run the worker loop
    /// 
    /// Subscribes to data broadcast and chromosome evaluation topics,
    /// processes requests as they arrive.
    pub async fn run(&self) -> Result<(), WorkerError> {
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_START",
            "event" => "GA_WORKER_START",
            "worker_id" => &self.worker_id,
            "max_concurrent" => MAX_CONCURRENT_EVALS
        );
        
        // Spawn background cache eviction task (runs every 5 minutes)
        let worker_for_eviction = self.clone_for_task();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                worker_for_eviction.evict_expired_cache_entries().await;
            }
        });
        
        // Subscribe to data broadcasts (legacy - for small datasets)
        let mut data_receiver = self.broker.subscribe_data_broadcasts();
        
        // Subscribe to data load requests (new - workers query DB directly)
        let mut data_load_receiver = self.broker.subscribe_data_load_requests();
        
        // Subscribe to chromosome evaluation requests
        let mut eval_receiver = self.broker.subscribe_chromosome_requests();
        
        println!("🔄 [WORKER {}] Main loop started, waiting for messages...", self.worker_id);
        
        loop {
            tokio::select! {
                // Handle legacy data broadcast (cache the data from message)
                Ok(broadcast) = data_receiver.recv() => {
                    println!("📨 [WORKER {}] Received DATA_BROADCAST", self.worker_id);
                    self.handle_data_broadcast(broadcast).await;
                }
                
                // Handle data load request (workers query DB directly)
                Ok(load_request) = data_load_receiver.recv() => {
                    println!("📨 [WORKER {}] Received DATA_LOAD request: cache_key={}", self.worker_id, load_request.cache_key);
                    self.handle_data_load_request(load_request).await;
                }
                
                // Handle chromosome evaluation request
                Ok(request) = eval_receiver.recv() => {
                    // Log every received request for diagnostics
                    log_info_structured!(MAIN_LOGGER, "GA_WORKER_RECV_EVAL_REQUEST",
                        "event" => "GA_WORKER_RECV_EVAL_REQUEST",
                        "worker_id" => &self.worker_id,
                        "job_id" => &request.job_id,
                        "generation" => request.generation,
                        "chromosome_id" => request.chromosome_id,
                        "is_final" => request.is_final_evaluation,
                        "cache_key" => &request.data_cache_key
                    );
                    
                    // Final evaluations should always be processed - they have fallback logic
                    // to find the binary file even if cache expired
                    let should_process = if request.is_final_evaluation {
                        println!("🔬 [WORKER {}] Received FINAL evaluation request for job {}", 
                            self.worker_id, request.job_id);
                        log_info_structured!(MAIN_LOGGER, "GA_WORKER_FINAL_EVAL_RECEIVED",
                            "event" => "GA_WORKER_FINAL_EVAL_RECEIVED",
                            "worker_id" => &self.worker_id,
                            "job_id" => &request.job_id,
                            "run_monte_carlo" => request.run_monte_carlo,
                            "run_walk_forward" => request.run_walk_forward
                        );
                        true
                    } else {
                        // For regular GA evals, check if we have the required data cached
                        let binary_cache = self.binary_cache.read().await;
                        let data_cache = self.data_cache.read().await;
                        let in_binary = binary_cache.contains_key(&request.data_cache_key);
                        let in_data = data_cache.contains_key(&request.data_cache_key);
                        println!("🔍 [WORKER {}] Checking cache for {}: binary={}, data={}", 
                            self.worker_id, request.data_cache_key, in_binary, in_data);
                        in_binary || in_data
                    };
                    
                    if should_process {
                        println!("✅ [WORKER {}] Processing chromosome {}/{} for job {}{}", 
                            self.worker_id, request.chromosome_id, request.total_chromosomes, request.job_id,
                            if request.is_final_evaluation { " (FINAL)" } else { "" });
                        // Spawn evaluation task with semaphore
                        let worker = self.clone_for_task();
                        let request_clone = request.clone();
                        
                        tokio::spawn(async move {
                            worker.process_chromosome_eval(request_clone).await;
                        });
                    } else {
                        println!("⏭️ [WORKER {}] Skipping chromosome - no data for cache_key {}", 
                            self.worker_id, request.data_cache_key);
                        log_info_structured!(MAIN_LOGGER, "GA_WORKER_SKIP_NO_CACHE",
                            "event" => "GA_WORKER_SKIP_NO_CACHE",
                            "worker_id" => &self.worker_id,
                            "job_id" => &request.job_id,
                            "cache_key" => &request.data_cache_key,
                            "generation" => request.generation,
                            "is_final" => request.is_final_evaluation
                        );
                        // Don't process - another worker with the data will handle it
                    }
                }
            }
        }
    }
    
    /// Handle incoming data broadcast by caching it
    async fn handle_data_broadcast(
        &self,
        broadcast: protocol::broker::messages::DataBroadcastRequest,
    ) {
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DATA_RECEIVE",
            "event" => "GA_WORKER_DATA_RECEIVE",
            "worker_id" => &self.worker_id,
            "cache_key" => &broadcast.cache_key,
            "compressed_bytes" => broadcast.compressed_data.len()
        );
        
        let start = Instant::now();
        
        // Decompress
        let decompressed = match lz4_flex::decompress_size_prepended(&broadcast.compressed_data) {
            Ok(data) => data,
            Err(e) => {
                log_error_structured!(MAIN_LOGGER, "GA_WORKER_DECOMPRESS_FAIL",
                    "event" => "GA_WORKER_DECOMPRESS_FAIL",
                    "error" => e.to_string()
                );
                return;
            }
        };
        
        // Deserialize
        let market_data: Vec<MarketData> = match bincode::deserialize(&decompressed) {
            Ok(data) => data,
            Err(e) => {
                log_error_structured!(MAIN_LOGGER, "GA_WORKER_DESERIALIZE_FAIL",
                    "event" => "GA_WORKER_DESERIALIZE_FAIL",
                    "error" => e.to_string()
                );
                return;
            }
        };
        
        let data_len = market_data.len();
        let expires_at = Instant::now() + Duration::from_secs(broadcast.ttl_seconds as u64);
        
        // Store in cache
        {
            let mut cache = self.data_cache.write().await;
            cache.insert(broadcast.cache_key.clone(), DataCacheEntry {
                data: Arc::new(market_data),
                expires_at,
            });
        }
        
        let elapsed = start.elapsed();
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DATA_CACHED",
            "event" => "GA_WORKER_DATA_CACHED",
            "worker_id" => &self.worker_id,
            "cache_key" => &broadcast.cache_key,
            "data_points" => data_len,
            "cache_time_ms" => elapsed.as_millis()
        );
        
        // Send acknowledgment with actual tick count
        let ack = protocol::broker::messages::DataCacheAck {
            cache_key: broadcast.cache_key,
            worker_id: self.worker_id.clone(),
            success: true,
            capacity: MAX_CONCURRENT_EVALS as i32,
            tick_count: data_len as i64,
        };
        
        let _ = self.broker.publish_data_ack(ack).await;
    }
    
    /// Handle data load request
    /// 
    /// Tries to find a binary file first (zero-copy mmap). If found, caches
    /// the file path for use with LightweightSim. Falls back to database
    /// loading if no binary file exists.
    async fn handle_data_load_request(
        &self,
        request: protocol::broker::messages::DataLoadRequest,
    ) {
        let cache_key = request.cache_key.clone();
        println!("🔄 [WORKER {}] handle_data_load_request for cache_key={}", self.worker_id, cache_key);
        
        // Check if already cached
        {
            let binary_cache = self.binary_cache.read().await;
            if let Some(info) = binary_cache.get(&cache_key) {
                println!("✅ [WORKER {}] Binary file already cached for {} ({} ticks)", 
                    self.worker_id, cache_key, info.tick_count);
                let ack = protocol::broker::messages::DataCacheAck {
                    cache_key,
                    worker_id: self.worker_id.clone(),
                    success: true,
                    capacity: MAX_CONCURRENT_EVALS as i32,
                    tick_count: info.tick_count as i64,  // Send actual tick count
                };
                let _ = self.broker.publish_data_ack(ack).await;
                return;
            }
        }
        {
            let data_cache = self.data_cache.read().await;
            if let Some(entry) = data_cache.get(&cache_key) {
                let tick_count = entry.data.len() as i64;
                println!("✅ [WORKER {}] Data already cached for {} ({} ticks)", 
                    self.worker_id, cache_key, tick_count);
                let ack = protocol::broker::messages::DataCacheAck {
                    cache_key,
                    worker_id: self.worker_id.clone(),
                    success: true,
                    capacity: MAX_CONCURRENT_EVALS as i32,
                    tick_count,  // Send actual tick count
                };
                let _ = self.broker.publish_data_ack(ack).await;
                return;
            }
        }
        
        // Acquire loading lock
        {
            let mut loading = self.loading_in_progress.lock().await;
            if loading.contains(&cache_key) {
                println!("⏳ [WORKER {}] Another worker loading {}, waiting...", self.worker_id, cache_key);
                drop(loading);
                
                // Wait for completion - get tick_count from cache when available
                for _ in 0..120 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let bc = self.binary_cache.read().await;
                    let dc = self.data_cache.read().await;
                    if let Some(info) = bc.get(&cache_key) {
                        let ack = protocol::broker::messages::DataCacheAck {
                            cache_key,
                            worker_id: self.worker_id.clone(),
                            success: true,
                            capacity: MAX_CONCURRENT_EVALS as i32,
                            tick_count: info.tick_count as i64,
                        };
                        let _ = self.broker.publish_data_ack(ack).await;
                        return;
                    }
                    if let Some(entry) = dc.get(&cache_key) {
                        let ack = protocol::broker::messages::DataCacheAck {
                            cache_key,
                            worker_id: self.worker_id.clone(),
                            success: true,
                            capacity: MAX_CONCURRENT_EVALS as i32,
                            tick_count: entry.data.len() as i64,
                        };
                        let _ = self.broker.publish_data_ack(ack).await;
                        return;
                    }
                }
                println!("❌ [WORKER {}] Timeout waiting for data", self.worker_id);
                return;
            }
            loading.insert(cache_key.clone());
        }
        
        let start = Instant::now();
        let symbol = request.symbols.first().cloned().unwrap_or_default();
        let expires_at = Instant::now() + Duration::from_secs(request.ttl_seconds as u64);
        
        // Try binary file first (preferred)
        if let Some((path, tick_count)) = self.probe_binary_file(&symbol, &request.exchange) {
            println!("📂 [WORKER {}] Found binary file: {} ({} ticks)", 
                self.worker_id, path.display(), tick_count);
            
            log_info_structured!(MAIN_LOGGER, "GA_WORKER_BINARY_FOUND",
                "event" => "GA_WORKER_BINARY_FOUND",
                "worker_id" => &self.worker_id,
                "cache_key" => &cache_key,
                "tick_count" => tick_count
            );
            
            // Cache binary file info (no data loading!)
            {
                let mut cache = self.binary_cache.write().await;
                cache.insert(cache_key.clone(), BinaryFileInfo {
                    path,
                    tick_count,
                    expires_at,
                });
            }
            
            // Release lock
            {
                let mut loading = self.loading_in_progress.lock().await;
                loading.remove(&cache_key);
            }
            
            println!("✅ [WORKER {}] Registered binary file in {:?}", 
                self.worker_id, start.elapsed());
            
            let ack = protocol::broker::messages::DataCacheAck {
                cache_key,
                worker_id: self.worker_id.clone(),
                success: true,
                capacity: MAX_CONCURRENT_EVALS as i32,
                tick_count: tick_count as i64,  // Send actual tick count for data splits
            };
            let _ = self.broker.publish_data_ack(ack).await;
            return;
        }
        
        // Auto-prepare binary file from database
        #[cfg(feature = "postgres")]
        {
            if let Some(pool) = &self.db_pool {
                println!("🔧 [WORKER {}] Binary file not found, auto-preparing...", self.worker_id);
                
                match self.prepare_binary_data(pool.as_ref(), &symbol, &request.exchange).await {
                    Ok(tick_count) => {
                        // Successfully prepared - now try to use the binary file
                        if let Some((path, count)) = self.probe_binary_file(&symbol, &request.exchange) {
                            println!("✅ [WORKER {}] Auto-prepared binary file: {} ({} ticks)", 
                                self.worker_id, path.display(), count);
                            
                            // Cache binary file info
                            {
                                let mut cache = self.binary_cache.write().await;
                                cache.insert(cache_key.clone(), BinaryFileInfo {
                                    path,
                                    tick_count: count,
                                    expires_at,
                                });
                            }
                            
                            // Release lock
                            {
                                let mut loading = self.loading_in_progress.lock().await;
                                loading.remove(&cache_key);
                            }
                            
                            let ack = protocol::broker::messages::DataCacheAck {
                                cache_key,
                                worker_id: self.worker_id.clone(),
                                success: true,
                                capacity: MAX_CONCURRENT_EVALS as i32,
                                tick_count: count as i64,  // Send actual tick count
                            };
                            let _ = self.broker.publish_data_ack(ack).await;
                            return;
                        }
                        
                        println!("⚠️ [WORKER {}] Prepared {} ticks but file not found", self.worker_id, tick_count);
                    }
                    Err(e) => {
                        println!("⚠️ [WORKER {}] Auto-prepare failed: {}, falling back to DB streaming", self.worker_id, e);
                    }
                }
            }
        }
        
        // Fallback: Load from database (streaming)
        #[cfg(feature = "postgres")]
        {
            if let Some(pool) = &self.db_pool {
                println!("🔄 [WORKER {}] Loading from database...", self.worker_id);
                match self.load_data_from_db(
                    pool.as_ref(),
                    &request.symbols,
                    &request.exchange,
                    &request.start_time,
                    &request.end_time,
                ).await {
                    Ok(data) => {
                        let data_len = data.len();
                        println!("✅ [WORKER {}] Loaded {} points from DB", self.worker_id, data_len);
                        
                        {
                            let mut cache = self.data_cache.write().await;
                            cache.insert(cache_key.clone(), DataCacheEntry {
                                data: Arc::new(data),
                                expires_at,
                            });
                        }
                        
                        {
                            let mut loading = self.loading_in_progress.lock().await;
                            loading.remove(&cache_key);
                        }
                        
                        let ack = protocol::broker::messages::DataCacheAck {
                            cache_key,
                            worker_id: self.worker_id.clone(),
                            success: true,
                            capacity: MAX_CONCURRENT_EVALS as i32,
                            tick_count: data_len as i64,  // Send actual tick count
                        };
                        let _ = self.broker.publish_data_ack(ack).await;
                        return;
                    }
                    Err(e) => {
                        println!("❌ [WORKER {}] DB load failed: {}", self.worker_id, e);
                        log_error_structured!(MAIN_LOGGER, "GA_WORKER_DB_LOAD_FAIL",
                            "event" => "GA_WORKER_DB_LOAD_FAIL",
                            "worker_id" => &self.worker_id,
                            "error" => e.to_string()
                        );
                    }
                }
            }
        }
        
        // Release lock on failure
        {
            let mut loading = self.loading_in_progress.lock().await;
            loading.remove(&cache_key);
        }
        
        let ack = protocol::broker::messages::DataCacheAck {
            cache_key,
            worker_id: self.worker_id.clone(),
            success: false,
            capacity: 0,
            tick_count: 0,
        };
        let _ = self.broker.publish_data_ack(ack).await;
    }
    
    /// Load market data from database using parallel loader
    #[cfg(feature = "postgres")]
    async fn load_data_from_db(
        &self,
        pool: &diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>,
        symbols: &[String],
        exchange: &str,
        _start_time: &str,
        _end_time: &str,
    ) -> Result<Vec<MarketData>, WorkerError> {
        use crate::streaming::ParallelDataLoader;
        
        let symbol = symbols.first().cloned().unwrap_or_default();
        
        println!("🔍 [WORKER {}] Starting DB query for {} on {}", self.worker_id, symbol, exchange);
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DB_QUERY",
            "event" => "GA_WORKER_DB_QUERY",
            "worker_id" => &self.worker_id,
            "symbol" => &symbol,
            "exchange" => exchange
        );
        
        // Use parallel loader with fewer workers to avoid overwhelming DB
        const NUM_WORKERS: usize = 4;
        println!("🔍 [WORKER {}] Creating ParallelDataLoader with {} workers", self.worker_id, NUM_WORKERS);
        let loader = ParallelDataLoader::new(
            std::sync::Arc::new(pool.clone()),
            NUM_WORKERS,
        );
        
        // Load all data using time-based partitioning for parallelism
        println!("🔍 [WORKER {}] Calling loader.load_all()...", self.worker_id);
        let market_data = loader.load_all(&symbol, exchange).await
            .map_err(|e| {
                println!("❌ [WORKER {}] DB load error: {}", self.worker_id, e);
                WorkerError::DatabaseError(e.to_string())
            })?;
        
        println!("✅ [WORKER {}] DB query complete: {} rows", self.worker_id, market_data.len());
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DB_QUERY_COMPLETE",
            "event" => "GA_WORKER_DB_QUERY_COMPLETE",
            "worker_id" => &self.worker_id,
            "rows_loaded" => market_data.len()
        );
        
        Ok(market_data)
    }
    
    /// Auto-prepare binary data file from database
    /// 
    /// Creates a unified `.bin` file with all order book events (new, modify, cancel, trade).
    /// This single file is used for all evaluations - both GA training and final evaluation.
    /// 
    /// This eliminates the need for manual `prepare-data` CLI invocation.
    #[cfg(feature = "postgres")]
    async fn prepare_binary_data(
        &self,
        pool: &diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>,
        symbol: &str,
        exchange: &str,
    ) -> Result<u64, WorkerError> {
        use data_prep::DataPrepConfig;
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_AUTO_PREPARE",
            "event" => "GA_WORKER_AUTO_PREPARE",
            "worker_id" => &self.worker_id,
            "symbol" => symbol,
            "exchange" => exchange
        );
        
        // Create output directory
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| WorkerError::DatabaseError(format!("Failed to create data dir: {}", e)))?;
        
        let config = DataPrepConfig {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            output_dir: self.data_dir.clone(),
            chunk_size: 10_000,
            compute_features: false,
            start_time: None,
            end_time: None,
        };
        
        // Prepare unified binary with all order book events
        let prep_start = std::time::Instant::now();
        let result = data_prep::prepare_orderbook_data(pool, &config).await
            .map_err(|e| WorkerError::DatabaseError(e.to_string()))?;
        let prep_elapsed = prep_start.elapsed();
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_AUTO_PREPARE_COMPLETE",
            "event" => "GA_WORKER_AUTO_PREPARE_COMPLETE",
            "worker_id" => &self.worker_id,
            "symbol" => symbol,
            "exchange" => exchange,
            "events" => result.total_ticks,
            "size_mb" => result.file_size_bytes / 1_000_000,
            "elapsed_sec" => format!("{:.1}", prep_elapsed.as_secs_f64())
        );
        
        Ok(result.total_ticks)
    }

    /// Process a chromosome evaluation request
    async fn process_chromosome_eval(
        &self,
        request: protocol::broker::messages::ChromosomeEvalRequest,
    ) {
        // Acquire semaphore permit (limits concurrent evals)
        let _permit = self.eval_semaphore.acquire().await.expect("semaphore closed");
        
        let start = Instant::now();
        
        if request.is_final_evaluation {
            log_info!(MAIN_LOGGER, "Worker {} running FINAL evaluation for job {} (MC={}, WF={})",
                self.worker_id, request.job_id, request.run_monte_carlo, request.run_walk_forward);
        } else {
            log_debug!(MAIN_LOGGER, "Worker {} processing chromosome {}/{} for gen {}",
                self.worker_id, request.chromosome_id, request.total_chromosomes, request.generation);
        }
        
        // Deserialize chromosome
        let chromosome: AvellanedaStoikovChromosome = match serde_json::from_slice(&request.chromosome_params) {
            Ok(c) => c,
            Err(e) => {
                self.send_error_result(&request, &format!("Failed to deserialize chromosome: {}", e)).await;
                return;
            }
        };
        
        // For final evaluation, we need the full backtest result for Monte Carlo
        if request.is_final_evaluation {
            self.process_final_evaluation(&request, &chromosome).await;
            return;
        }
        
        // Regular GA evaluation - try binary file first (preferred: memory-efficient)
        // Use only train portion of data (0 to train_end_idx)
        let result = {
            let path_to_eval = {
                let binary_cache = self.binary_cache.read().await;
                binary_cache.get(&request.data_cache_key)
                    .filter(|info| info.expires_at > Instant::now())
                    .map(|info| info.path.clone())
            };
            
            if let Some(path) = path_to_eval {
                Some(self.evaluate_with_streaming_sim(
                    &chromosome, 
                    &path, 
                    request.initial_capital,
                    request.train_end_idx, // Use only train data for GA fitness
                    &request.exchange,
                    request.initial_30d_volume,
                ).await)
            } else {
                None
            }
        };
        
        // Fall back to in-memory data if no binary file
        let result = match result {
            Some(r) => r,
            None => {
                let data = {
                    let cache = self.data_cache.read().await;
                    match cache.get(&request.data_cache_key) {
                        Some(entry) if entry.expires_at > Instant::now() => {
                            Arc::clone(&entry.data)
                        }
                        _ => {
                            self.send_error_result(&request, "Data not in cache or expired").await;
                            return;
                        }
                    }
                };
                self.evaluate_chromosome(&chromosome, &data, request.initial_capital, request.initial_inventory_pct, request.initial_30d_volume).await
            }
        };
        
        let elapsed = start.elapsed();
        
        // Sample equity curve to avoid 90MB+ payloads
        let sampled_curve = sample_equity_curve(&result.equity_curve, MAX_EQUITY_CURVE_POINTS);
        
        // Send result with actual metrics from FitnessResult
        let response = protocol::broker::messages::ChromosomeEvalResult {
            job_id: request.job_id,
            generation: request.generation,
            chromosome_id: request.chromosome_id,
            worker_id: self.worker_id.clone(),
            success: true,
            error_message: String::new(),
            fitness: result.fitness,
            net_pnl: result.net_pnl,
            sharpe_ratio: result.sharpe_ratio,
            num_trades: result.num_trades,
            max_drawdown: result.max_drawdown,
            equity_curve: serde_json::to_vec(&sampled_curve).unwrap_or_default(),
            processing_duration_ms: elapsed.as_millis() as i64,
            win_rate: result.win_rate,
            profit_factor: result.profit_factor,
            closed_trades: result.num_trades, // Same as num_trades for now
            total_orders: 0, // Not tracked in FitnessResult
            starting_capital: request.initial_capital,
            ending_capital: request.initial_capital + result.net_pnl,
            total_return_pct: (result.net_pnl / request.initial_capital) * 100.0,
            monte_carlo_results: Vec::new(), // Only populated in final evaluation
            walk_forward_results: Vec::new(), // Only populated in final evaluation
            closed_trades_data: Vec::new(), // Only populated in final evaluation
            is_final_evaluation: false,
            statistical_significance: String::new(), // Only populated in final evaluation
        };
        
        println!("📤 [WORKER {}] Sending result for chromosome {}/{} fitness={:.4} in {:?}", 
            self.worker_id, request.chromosome_id, request.total_chromosomes, result.fitness, elapsed);
        let _ = self.broker.publish_chromosome_result(response).await;
    }
    
    /// Evaluate chromosome using REALISTIC simulation with L3 order book data
    /// 
    /// This uses full order book reconstruction from L3 events for accurate fill simulation.
    /// Falls back to trade-only data with trade-through detection if L3 not available.
    /// 
    /// If `train_end_idx` > 0, only events up to that tick index are used for training.
    async fn evaluate_with_streaming_sim(
        &self,
        chromosome: &AvellanedaStoikovChromosome,
        path: &std::path::Path,
        initial_capital: f64,
        train_end_idx: i64,
        exchange: &str,
        initial_30d_volume: f64,
    ) -> FitnessResult {
        use data_prep::OrderBookBinaryReader;
        use backtest::realistic_simulation::{RealisticSimulator, RealisticASParams, OrderBookEvent};
        
        let sim_start = std::time::Instant::now();
        
        // Load order book events from unified binary file (contains all event types)
        // The binary file is prepared by the API and contains L3 orderbook data
        let events: Vec<OrderBookEvent> = match OrderBookBinaryReader::open(path) {
            Ok(reader) => {
                let total_events = reader.len();
                
                // Calculate effective training range
                let end_timestamp = if train_end_idx > 0 {
                    // Use the provided index to get timestamp boundary
                    reader.get(train_end_idx as usize)
                        .map(|e| e.timestamp_ms)
                        .unwrap_or(i64::MAX)
                } else {
                    // Use 60% of data for training
                    let idx_60pct = (total_events as f64 * 0.6) as usize;
                    reader.get(idx_60pct).map(|e| e.timestamp_ms).unwrap_or(i64::MAX)
                };
                
                // Load events up to training boundary
                let events: Vec<OrderBookEvent> = (0..total_events)
                    .filter_map(|i| reader.get(i))
                    .take_while(|e| e.timestamp_ms <= end_timestamp)
                    .map(OrderBookEvent::from)
                    .collect();
                
                println!("🔬 [WORKER {}] REALISTIC sim on {} events (of {} total)", 
                    self.worker_id, events.len(), total_events);
                events
            }
            Err(e) => {
                println!("❌ [WORKER {}] Failed to open binary file: {}", self.worker_id, e);
                return FitnessResult::failure();
            }
        };
        
        if events.is_empty() {
            println!("❌ [WORKER {}] No events loaded for evaluation", self.worker_id);
            return FitnessResult::failure();
        }
        
        // Get exchange fee configuration with proper volume tier
        let mut exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
        if initial_30d_volume > 0.0 {
            exchange_fee_config.starting_30d_volume = initial_30d_volume;
            exchange_fee_config.maker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, true);
            exchange_fee_config.taker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, false);
        }
        
        // Convert chromosome to realistic simulation parameters
        let realistic_params = RealisticASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            max_inventory_multiple: 5.0,
            latency_mean_ms: exchange_fee_config.latency_mean_ms,
            latency_std_ms: exchange_fee_config.latency_std_ms,
            enable_partial_fills: true,
            min_fill_fraction: 0.1,
            max_orders_per_second: exchange_fee_config.max_orders_per_second,
            rate_limit_cooldown_ms: exchange_fee_config.rate_limit_cooldown_ms,
            funding_rate_8h: exchange_fee_config.funding_rate_8h,
            funding_interval_ms: exchange_fee_config.funding_interval_ms,
            adverse_selection_sensitivity: chromosome.adverse_selection_sensitivity,
        };
        
        // Run realistic simulation
        let simulator = RealisticSimulator::with_exchange_config(initial_capital, exchange_fee_config);
        let result = simulator.run(&events, &realistic_params);
        
        let sim_elapsed = sim_start.elapsed();
        println!("✅ [WORKER {}] REALISTIC sim complete in {:.2}s: PnL={:.2}, Sharpe={:.4}, Trades={}", 
            self.worker_id, sim_elapsed.as_secs_f64(), result.total_pnl, result.sharpe_ratio, result.num_trades);
        
        // Convert to standard BacktestResult and calculate fitness
        let backtest_result: backtest::types::BacktestResult = result.clone().into();
        let fitness = calculate_comprehensive_fitness(&backtest_result, initial_capital);
        
        FitnessResult::with_metrics(
            fitness,
            backtest_result.equity_curve,
            result.total_pnl,
            result.sharpe_ratio,
            result.num_trades as i32,
            result.max_drawdown,
            result.win_rate,
            result.profit_factor,
        )
    }
    
    /// Evaluate a chromosome against the cached data
    async fn evaluate_chromosome(
        &self,
        chromosome: &AvellanedaStoikovChromosome,
        data: &[MarketData],
        initial_capital: f64,
        initial_inventory_pct: f64,
        _initial_30d_volume: f64,
    ) -> FitnessResult {
        use backtest::BacktestEngine;
        use strategy::StrategyManager;
        use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovStrategy, AvellanedaStoikovParams};
        use orderbook::{OrderBook, OrderBookConfig};
        use portfoliomanager::PortfolioState;
        use riskmanager::RiskManager;
        use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};
        
        // Get reference price from data
        let initial_price = data.first()
            .and_then(|d| match d {
                MarketData::Trade(t) => t.price.to_f64(),
                MarketData::Candle(c) => c.close.to_f64(),
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.price_level.to_f64(),
                #[cfg(feature = "postgres")]
                MarketData::Order(o) => o.price_level.to_f64(),
                #[allow(unreachable_patterns)]
                _ => None,
            })
            .unwrap_or(50000.0);
        
        // Create strategy from chromosome using struct update syntax
        let order_size_units = (initial_capital * chromosome.order_size_pct) / initial_price;
        
        let params = AvellanedaStoikovParams {
            risk_aversion: chromosome.risk_aversion,
            order_size: order_size_units,
            order_size_pct: chromosome.order_size_pct,
            initial_capital,
            window_ms: chromosome.window_ms,
            min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms,
            inventory_target: chromosome.inventory_target,
            volatility_cap: chromosome.volatility_cap,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            market_depth_k: chromosome.market_depth_k,
            min_spread_bps: chromosome.min_spread_bps,
            max_trades_per_minute: chromosome.max_trades_per_minute,
            take_profit_pct: chromosome.take_profit_pct,
            stop_loss_pct: chromosome.stop_loss_pct,
            volatility_adjustment_factor: chromosome.volatility_adjustment_factor,
            inventory_penalty_factor: chromosome.inventory_penalty_factor,
            trend_sensitivity: chromosome.trend_sensitivity,
            flow_sensitivity: chromosome.flow_sensitivity,
            ..AvellanedaStoikovParams::default()
        };
        
        let strategy = AvellanedaStoikovStrategy::new(params);
        let mut strategy_manager = StrategyManager::new();
        
        let strategy_config = strategy::StrategyConfig {
            id: "ga-eval".to_string(),
            name: "Avellaneda-Stoikov".to_string(),
            strategy_type: "AvellanedaStoikov".to_string(),
            symbol: "BTC/USD".to_string(),
            exchange: "Kraken".to_string(),
            parameters: std::collections::HashMap::new(),
            risk_limits: strategy::config::RiskLimits::default(),
            enabled: true,
        };
        
        if strategy_manager.register_strategy(strategy_config, strategy).is_err() {
            return FitnessResult::failure();
        }
        
        let mut orderbook = OrderBook::new(
            "BTC/USD".to_string(),
            "Kraken".to_string(),
            OrderBookConfig::default(),
        );
        
        // Get first timestamp for portfolio
        let first_timestamp = data.first()
            .map(|d| match d {
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.timestamp,
                #[cfg(feature = "postgres")]
                MarketData::Order(o) => o.timestamp,
                #[allow(unreachable_patterns)]
                _ => chrono::Utc::now(),
            })
            .unwrap_or_else(chrono::Utc::now);
        
        let inventory_pct = if initial_inventory_pct > 0.0 { initial_inventory_pct } else { 0.5 };
        let quote_balance = initial_capital * (1.0 - inventory_pct);
        let base_value = initial_capital * inventory_pct;
        let base_quantity = base_value / initial_price;
        
        let mut portfolio = PortfolioState::with_inventory(
            "BTC/USD".to_string(),
            BigDecimal::from_f64(quote_balance).unwrap_or_default(),
            base_quantity,
            BigDecimal::from_f64(initial_price).unwrap_or_default(),
            first_timestamp,
        );
        
        let mut risk_manager = RiskManager::new();
        
        // Create minimal backtest config
        let backtest_config = config::BacktestConfig::default();
        let engine = BacktestEngine::new(backtest_config.clone());
        
        // Run simulation
        match engine.run_core_simulation_direct(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            data,
        ).await {
            Ok(result) => {
                // Calculate fitness
                let fitness = calculate_comprehensive_fitness(&result, initial_capital);
                FitnessResult::new(fitness, result.equity_curve)
            }
            Err(_) => FitnessResult::failure(),
        }
    }

    /// Send an error result for a failed evaluation
    async fn send_error_result(
        &self,
        request: &protocol::broker::messages::ChromosomeEvalRequest,
        error: &str,
    ) {
        let response = protocol::broker::messages::ChromosomeEvalResult {
            job_id: request.job_id.clone(),
            generation: request.generation,
            chromosome_id: request.chromosome_id,
            worker_id: self.worker_id.clone(),
            success: false,
            error_message: error.to_string(),
            fitness: 0.0,
            net_pnl: 0.0,
            sharpe_ratio: 0.0,
            num_trades: 0,
            max_drawdown: 0.0,
            equity_curve: Vec::new(),
            processing_duration_ms: 0,
            win_rate: 0.0,
            profit_factor: 0.0,
            closed_trades: 0,
            total_orders: 0,
            starting_capital: request.initial_capital,
            ending_capital: request.initial_capital,
            total_return_pct: 0.0,
            monte_carlo_results: Vec::new(),
            walk_forward_results: Vec::new(),
            closed_trades_data: Vec::new(),
            is_final_evaluation: false,
            statistical_significance: String::new(),
        };
        
        let _ = self.broker.publish_chromosome_result(response).await;
    }
    
    /// Load trade events from trade-only binary file as fallback
    /// 
    /// Used when L3 order book data is not available. The simulation will use
    /// trade-through detection for fills instead of actual order book reconstruction.
    async fn load_trade_events_fallback(
        &self,
        request: &protocol::broker::messages::ChromosomeEvalRequest,
    ) -> Vec<backtest::realistic_simulation::OrderBookEvent> {
        use data_prep::BinaryFileReader;
        use dataloader::TickSource;
        use backtest::realistic_simulation::ticks_to_trade_events;
        
        // Get binary file path from cache
        let path = {
            let binary_cache = self.binary_cache.read().await;
            binary_cache.get(&request.data_cache_key)
                .map(|info| info.path.clone())
        };
        
        let path: std::path::PathBuf = match path {
            Some(p) if p.exists() => p,
            _ => {
                // Fallback: try to find the file in the data directory
                let fallback_path = self.data_dir.join(format!("{}_{}.bin", 
                    request.symbol.replace("/", "_"),
                    request.exchange));
                if fallback_path.exists() {
                    fallback_path
                } else {
                    println!("❌ [WORKER {}] No trade binary file found", self.worker_id);
                    return Vec::new();
                }
            }
        };
        
        // Load binary file
        let reader = match BinaryFileReader::open(&path) {
            Ok(r) => r,
            Err(e) => {
                println!("❌ [WORKER {}] Failed to open binary file: {}", self.worker_id, e);
                return Vec::new();
            }
        };
        
        let total_ticks = reader.len();
        
        // Determine data range for final evaluation
        let (start_idx, end_idx) = if request.test_end_idx > 0 && request.total_ticks > request.test_end_idx {
            (request.test_end_idx as usize, request.total_ticks as usize)
        } else {
            (0, total_ticks)
        };
        
        println!("📊 [WORKER {}] Loading trade-only ticks [{}, {}) as fallback", 
            self.worker_id, start_idx, end_idx);
        
        let ticks: Vec<_> = (start_idx..end_idx).filter_map(|i| reader.get(i).cloned()).collect();
        ticks_to_trade_events(&ticks)
    }
    
    /// Run a single realistic simulation on all data when there's not enough for walk-forward
    /// Returns a WalkForwardSummary with a single "window" for compatibility
    fn run_single_realistic_simulation(
        &self,
        events: &[backtest::realistic_simulation::OrderBookEvent],
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
        exchange: &str,
        initial_capital: f64,
        initial_30d_volume: f64,
    ) -> walkforward::WalkForwardSummary {
        use backtest::realistic_simulation::{RealisticSimulator, RealisticASParams};
        
        if events.is_empty() {
            println!("⚠️ [WORKER {}] No events for single simulation", self.worker_id);
            return walkforward::WalkForwardSummary::new(vec![]);
        }
        
        let mut exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
        if initial_30d_volume > 0.0 {
            exchange_fee_config.starting_30d_volume = initial_30d_volume;
            exchange_fee_config.maker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, true);
            exchange_fee_config.taker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, false);
        }
        
        let realistic_params = RealisticASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            max_inventory_multiple: 5.0,
            latency_mean_ms: exchange_fee_config.latency_mean_ms,
            latency_std_ms: exchange_fee_config.latency_std_ms,
            enable_partial_fills: true,
            min_fill_fraction: 0.1,
            max_orders_per_second: exchange_fee_config.max_orders_per_second,
            rate_limit_cooldown_ms: exchange_fee_config.rate_limit_cooldown_ms,
            funding_rate_8h: exchange_fee_config.funding_rate_8h,
            funding_interval_ms: exchange_fee_config.funding_interval_ms,
            adverse_selection_sensitivity: chromosome.adverse_selection_sensitivity,
        };
        
        let simulator = RealisticSimulator::with_exchange_config(initial_capital, exchange_fee_config);
        let result = simulator.run(events, &realistic_params);
        
        println!("🔬 [WORKER {}] Single simulation: trades={}, sharpe={:.4}, pnl={:.2}",
            self.worker_id, result.num_trades, result.sharpe_ratio, result.total_pnl);
        
        // Get date range for the "window"
        let first_ts = events.first().map(|e| e.timestamp_ms).unwrap_or(0);
        let last_ts = events.last().map(|e| e.timestamp_ms).unwrap_or(0);
        let start_date = chrono::DateTime::from_timestamp_millis(first_ts)
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());
        let end_date = chrono::DateTime::from_timestamp_millis(last_ts)
            .map(|dt| dt.date_naive())
            .unwrap_or_else(|| chrono::Utc::now().date_naive());
        
        // Create a single window result using PerformanceMetrics
        let window_result = walkforward::WalkForwardWindowResult {
            train_start: start_date,
            train_end: start_date, // No training for single sim
            test_start: start_date,
            test_end: end_date,
            best_params: serde_json::json!({
                "risk_aversion": chromosome.risk_aversion,
                "order_size_pct": chromosome.order_size_pct,
                "inventory_target": chromosome.inventory_target,
                "volatility_cap": chromosome.volatility_cap,
            }),
            test_metrics: metrics::PerformanceMetrics {
                net_profit: result.total_pnl,
                sharpe_ratio: result.sharpe_ratio,
                max_drawdown: result.max_drawdown,
                win_rate: result.win_rate,
                number_of_trades: result.num_trades,
                profit_factor: result.profit_factor,
                // Default values for fields we don't have from RealisticSimResult
                gross_profit: 0.0,
                gross_loss: 0.0,
                average_trade_return: if result.num_trades > 0 { result.total_pnl / result.num_trades as f64 } else { 0.0 },
                median_trade_return: 0.0,
                volatility: 0.0,
                average_trade_duration: 0.0,
                total_commission: result.total_fees,
                total_slippage: 0.0,
                total_market_impact: 0.0,
                transaction_costs_percentage: if result.total_volume > 0.0 { result.total_fees / result.total_volume * 100.0 } else { 0.0 },
            },
            trade_returns: result.trade_returns,
        };
        
        walkforward::WalkForwardSummary::new(vec![window_result])
    }
    
    /// Fallback walk-forward using trade-only data with simplified simulation
    #[allow(dead_code)]
    async fn run_walkforward_trades_fallback(
        &self,
        request: &protocol::broker::messages::ChromosomeEvalRequest,
        chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
    ) -> Vec<u8> {
        let trade_binary_path = self.data_dir.join(format!("{}_{}.bin", 
            request.symbol.replace("/", "_"), request.exchange));
        
        if !trade_binary_path.exists() {
            log_warn!(MAIN_LOGGER, "No trade binary file found for walk-forward at {:?}", trade_binary_path);
            return Vec::new();
        }
        
        match data_prep::BinaryFileReader::open(&trade_binary_path) {
            Ok(reader) => {
                // Specify the test data range for walk-forward
                let (wf_start_idx, wf_end_idx) = if request.train_end_idx > 0 && request.test_end_idx > request.train_end_idx {
                    (Some(request.train_end_idx as usize), Some(request.test_end_idx as usize))
                } else {
                    (None, None)
                };
                
                let wf_summary = walkforward::run_walkforward_for_chromosome(
                    &reader,
                    chromosome,
                    &request.symbol,
                    &request.exchange,
                    request.initial_capital,
                    Some(30),
                    Some(7),
                    Some(7),
                    wf_start_idx,
                    wf_end_idx,
                ).await;
                
                log_info!(MAIN_LOGGER, "Walk-Forward FALLBACK complete: windows={}, avg_sharpe={:.4}",
                    wf_summary.num_windows, wf_summary.average_test_sharpe);
                
                serde_json::to_vec(&wf_summary).unwrap_or_default()
            }
            Err(e) => {
                log_warn!(MAIN_LOGGER, "Failed to open trade binary for walk-forward fallback: {}", e);
                Vec::new()
            }
        }
    }

    /// Process a final evaluation request with full backtest, Monte Carlo, and Walk-Forward
    async fn process_final_evaluation(
        &self,
        request: &protocol::broker::messages::ChromosomeEvalRequest,
        chromosome: &AvellanedaStoikovChromosome,
    ) {
        use data_prep::OrderBookBinaryReader;
        use backtest::realistic_simulation::{RealisticASParams, OrderBookEvent, EventType};
        use backtest::statistical_significance::{compute_sharpe_significance, compute_deflated_sharpe_ratio};
        
        let start = Instant::now();
        
        println!("🔬 [WORKER {}] FINAL EVALUATION started for job {}", self.worker_id, request.job_id);
        println!("🔬 [WORKER {}] Chromosome: risk_aversion={:.4}, order_size_pct={:.4}, inventory_target={:.4}",
            self.worker_id, chromosome.risk_aversion, chromosome.order_size_pct, chromosome.inventory_target);
        println!("🔬 [WORKER {}] Data splits: train_end_idx={}, test_end_idx={}, total_ticks={}",
            self.worker_id, request.train_end_idx, request.test_end_idx, request.total_ticks);
        
        // Load unified binary file with all event types (New/Modify/Cancel/Trade)
        let bin_path = self.data_dir.join(format!("{}_{}.bin", 
            request.symbol.replace("/", "_"), request.exchange));
        
        let events: Vec<OrderBookEvent> = if bin_path.exists() {
            println!("📚 [WORKER {}] Loading order book data: {:?}", self.worker_id, bin_path);
            
            match OrderBookBinaryReader::open(&bin_path) {
                Ok(reader) => {
                    let total_events = reader.len();
                    println!("📚 [WORKER {}] Binary file has {} events", self.worker_id, total_events);
                    
                    let events_start = std::time::Instant::now();
                    let events: Vec<OrderBookEvent> = (0..total_events)
                        .filter_map(|i| reader.get(i))
                        .map(OrderBookEvent::from)
                        .collect();
                    
                    println!("📚 [WORKER {}] Loaded {} events in {:.2}s", 
                        self.worker_id, events.len(), events_start.elapsed().as_secs_f64());
                    events
                }
                Err(e) => {
                    println!("⚠️ [WORKER {}] Failed to open binary file: {}, trying trade fallback", self.worker_id, e);
                    self.load_trade_events_fallback(request).await
                }
            }
        } else {
            // Fall back to trade-only data (legacy format)
            println!("⚠️ [WORKER {}] No unified binary found at {:?}, using trade-only fallback", self.worker_id, bin_path);
            self.load_trade_events_fallback(request).await
        };
        
        if events.is_empty() {
            println!("❌ [WORKER {}] No events loaded for final evaluation!", self.worker_id);
            self.send_error_result(request, "No events loaded for final evaluation").await;
            return;
        }
        
        // Log event type breakdown
        let new_count = events.iter().filter(|e| matches!(e.event_type, EventType::New)).count();
        let modify_count = events.iter().filter(|e| matches!(e.event_type, EventType::Modify)).count();
        let cancel_count = events.iter().filter(|e| matches!(e.event_type, EventType::Cancel)).count();
        let trade_count = events.iter().filter(|e| matches!(e.event_type, EventType::Trade)).count();
        println!("📊 [WORKER {}] Event breakdown: {} new, {} modify, {} cancel, {} trade (total: {})", 
            self.worker_id, new_count, modify_count, cancel_count, trade_count, events.len());
        
        // Get exchange fee configuration with proper volume tier
        let mut exchange_fee_config = config::ExchangeFeeConfig::for_exchange(&request.exchange);
        if request.initial_30d_volume > 0.0 {
            exchange_fee_config.starting_30d_volume = request.initial_30d_volume;
            exchange_fee_config.maker_fee = exchange_fee_config.get_fee_rate(request.initial_30d_volume, true);
            exchange_fee_config.taker_fee = exchange_fee_config.get_fee_rate(request.initial_30d_volume, false);
        }
        println!("📊 [WORKER {}] Using {} config: maker={:.4}%, taker={:.4}%, latency={:.1}ms±{:.1}ms, 30d_volume=${:.0}",
            self.worker_id, exchange_fee_config.exchange, 
            exchange_fee_config.maker_fee * 100.0, exchange_fee_config.taker_fee * 100.0,
            exchange_fee_config.latency_mean_ms, exchange_fee_config.latency_std_ms,
            exchange_fee_config.starting_30d_volume);
        
        // Convert chromosome to realistic simulation parameters with institutional-grade settings
        // All operational parameters come from exchange configuration - no hardcoded values
        let _realistic_params = RealisticASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            
            // Maker/taker fees from exchange configuration
            maker_fee_rate: exchange_fee_config.maker_fee,
            taker_fee_rate: exchange_fee_config.taker_fee,
            
            // Inventory limits (TODO: make configurable per exchange)
            max_inventory_multiple: 5.0,
            
            // Latency from exchange configuration
            latency_mean_ms: exchange_fee_config.latency_mean_ms,
            latency_std_ms: exchange_fee_config.latency_std_ms,
            
            // Partial fills enabled
            enable_partial_fills: true,
            min_fill_fraction: 0.1,
            
            // Rate limiting from exchange configuration
            max_orders_per_second: exchange_fee_config.max_orders_per_second,
            rate_limit_cooldown_ms: exchange_fee_config.rate_limit_cooldown_ms,
            
            // Funding rate from exchange configuration
            funding_rate_8h: exchange_fee_config.funding_rate_8h,
            funding_interval_ms: exchange_fee_config.funding_interval_ms,
            
            // Adverse selection from chromosome
            adverse_selection_sensitivity: chromosome.adverse_selection_sensitivity,
        };
        
        // WALK-FORWARD IS THE PRIMARY VALIDATION
        // We run walk-forward on ALL validation data (train_end to end) and use its metrics as the primary output
        // This provides multiple out-of-sample tests which is more robust than a single period simulation
        
        println!("🔬 [WORKER {}] Running WALK-FORWARD as PRIMARY validation on validation data", self.worker_id);
        
        // Filter events to validation period based on timestamp
        let validation_events = if request.train_end_idx > 0 {
            // Use L3 binary reader to get timestamp range for validation window
            let binary_path = self.data_dir.join(format!("{}_{}.bin", 
                request.symbol.replace("/", "_"), request.exchange));
            
            if let Ok(reader) = data_prep::OrderBookBinaryReader::open(&binary_path) {
                let start_ts = reader.get(request.train_end_idx as usize)
                    .map(|t| t.timestamp_ms).unwrap_or(0);
                
                println!("🔬 [WORKER {}] Filtering to validation period: from tick {} (ts={})", 
                    self.worker_id, request.train_end_idx, start_ts);
                
                events.iter()
                    .filter(|e| e.timestamp_ms >= start_ts)
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                // Can't determine split timestamp, use all events
                events.clone()
            }
        } else {
            events.clone()
        };
        
        println!("🔬 [WORKER {}] Validation events: {} (from {} total)", 
            self.worker_id, validation_events.len(), events.len());
        
        // Calculate data date range to determine appropriate window sizes
        let first_event_ts = validation_events.first().map(|e| e.timestamp_ms).unwrap_or(0);
        let last_event_ts = validation_events.last().map(|e| e.timestamp_ms).unwrap_or(0);
        let data_days = (last_event_ts - first_event_ts) as f64 / (1000.0 * 60.0 * 60.0 * 24.0);
        
        // PROPORTIONAL window sizing: train=60%, test=20%, step=20% of total data
        // This ensures we always get at least 2 windows (for consistency measurement)
        // and works regardless of data length
        let min_days_for_walkforward = 7.0; // Need at least 1 week for meaningful walk-forward
        
        let (train_days, test_days, step_days) = if data_days >= min_days_for_walkforward {
            // Use proportional splits: 60% train, 20% test, 20% step
            // This guarantees 2+ windows: first at 0%, second at 20%, third at 40%...
            let train = (data_days * 0.60).max(1.0) as i64;
            let test = (data_days * 0.20).max(1.0) as i64;
            let step = (data_days * 0.20).max(1.0) as i64;
            (train, test, step)
        } else {
            // Less than a week: run single simulation
            println!("⚠️ [WORKER {}] Only {:.1} days of data - running single simulation (no walk-forward)", 
                self.worker_id, data_days);
            (0i64, 0i64, 0i64) // Signal to skip walk-forward
        };
        
        // Calculate expected number of windows
        let expected_windows = if train_days > 0 && test_days > 0 && step_days > 0 {
            let total_needed = train_days + test_days;
            let remaining = data_days as i64 - total_needed;
            if remaining >= 0 {
                1 + (remaining / step_days) as usize
            } else {
                0
            }
        } else {
            0
        };
        
        println!("🔬 [WORKER {}] Data span: {:.1} days | Proportional split: train={}d ({:.0}%) test={}d ({:.0}%) step={}d | Expected windows: {}",
            self.worker_id, data_days, 
            train_days, (train_days as f64 / data_days) * 100.0,
            test_days, (test_days as f64 / data_days) * 100.0,
            step_days, expected_windows);
        
        // Run walk-forward analysis on validation data
        // This is the PRIMARY evaluation - multiple out-of-sample windows
        // Using local run_walkforward_realistic to avoid circular dependency
        let wf_summary = if train_days > 0 {
            run_walkforward_realistic(
                &validation_events,
                chromosome,
                &request.exchange,
                request.initial_capital,
                request.initial_30d_volume,
                Some(train_days),
                Some(test_days),
                Some(step_days),
            )
        } else {
            // Not enough data for walk-forward - run single realistic simulation
            self.run_single_realistic_simulation(&validation_events, chromosome, &request.exchange, request.initial_capital, request.initial_30d_volume)
        };
        
        println!("🔬 [WORKER {}] Walk-forward PRIMARY validation complete:", self.worker_id);
        println!("   📊 Windows: {}, Avg Sharpe: {:.4}, Consistency: {:.4}", 
            wf_summary.num_windows, wf_summary.average_test_sharpe, wf_summary.consistency_score);
        println!("   📊 Best window: {:.4}, Worst window: {:.4}", 
            wf_summary.best_window_performance, wf_summary.worst_window_performance);
        
        // Aggregate metrics from walk-forward windows
        // PRIMARY metrics come from walk-forward's out-of-sample tests
        let net_pnl: f64 = wf_summary.window_results.iter()
            .map(|w| w.test_metrics.net_profit)
            .sum();
        let sharpe = wf_summary.average_test_sharpe;
        let num_trades: i32 = wf_summary.window_results.iter()
            .map(|w| w.test_metrics.number_of_trades as i32)
            .sum();
        let max_dd = wf_summary.window_results.iter()
            .map(|w| w.test_metrics.max_drawdown)
            .fold(0.0_f64, |a, b| a.max(b));
        let win_rate = if wf_summary.num_windows > 0 {
            wf_summary.window_results.iter()
                .map(|w| w.test_metrics.win_rate)
                .sum::<f64>() / wf_summary.num_windows as f64
        } else {
            0.0
        };
        let profit_factor = if wf_summary.num_windows > 0 {
            wf_summary.window_results.iter()
                .filter_map(|w| {
                    let pf = w.test_metrics.profit_factor;
                    if pf.is_finite() { Some(pf) } else { None }
                })
                .sum::<f64>() / wf_summary.num_windows as f64
        } else {
            0.0
        };
        
        let ending_capital = request.initial_capital + net_pnl;
        let total_return_pct = (net_pnl / request.initial_capital) * 100.0;
        let closed_trades = num_trades;
        let total_orders = num_trades * 2; // Estimate (entry + exit)
        
        // Aggregate PER-TRADE returns from all walk-forward windows FIRST
        // (needed for sortino calculation and consistent net_pnl/equity curve)
        let all_trade_returns: Vec<f64> = wf_summary.window_results.iter()
            .flat_map(|w| w.trade_returns.iter().copied())
            .collect();
        
        // Build an aggregate equity curve from per-trade returns
        // Do this BEFORE calculating net_pnl so they are consistent
        let mut aggregate_equity_curve: Vec<f64> = Vec::with_capacity(all_trade_returns.len() + 1);
        let mut current_equity = request.initial_capital;
        aggregate_equity_curve.push(current_equity);
        for trade_pnl in &all_trade_returns {
            current_equity += trade_pnl;
            aggregate_equity_curve.push(current_equity);
        }
        
        // Calculate net_pnl from equity curve to ensure consistency with displayed curve
        // This replaces the previous method that summed net_profit from window metrics
        let net_pnl_from_curve = current_equity - request.initial_capital;
        let net_pnl_from_windows: f64 = wf_summary.window_results.iter()
            .map(|w| w.test_metrics.net_profit)
            .sum();
        
        // Use the equity curve value (more accurate as it matches what's displayed)
        // Log if there's a significant discrepancy for debugging
        if (net_pnl_from_curve - net_pnl_from_windows).abs() > 1.0 {
            println!("⚠️ [WORKER {}] net_pnl discrepancy: curve={:.2} vs windows={:.2}, using curve value",
                self.worker_id, net_pnl_from_curve, net_pnl_from_windows);
        }
        let net_pnl = net_pnl_from_curve;
        
        // Parse fitness weights from request (if provided)
        let weights: FitnessWeights = if !request.fitness_weights_json.is_empty() {
            serde_json::from_slice(&request.fitness_weights_json)
                .unwrap_or_else(|_| FitnessWeights::balanced())
        } else {
            FitnessWeights::balanced()
        };
        
        // Calculate additional metrics for weighted fitness
        let sortino = calculate_sortino_ratio(&all_trade_returns);
        let calmar = calculate_calmar_ratio(total_return_pct, max_dd, 252.0);
        
        // Calculate fitness from walk-forward metrics using custom weights
        let fitness = if wf_summary.num_windows > 0 {
            calculate_weighted_fitness(
                sharpe,
                sortino,
                profit_factor,
                win_rate,
                net_pnl,
                calmar,
                wf_summary.consistency_score,
                max_dd,
                &weights,
                request.initial_capital,
            )
        } else {
            0.0
        };
        
        println!("🔬 [WORKER {}] WALK-FORWARD VALIDATION COMPLETE: PnL={:.2}, Sharpe={:.4}, trades={}, max_dd={:.4}",
            self.worker_id, net_pnl, sharpe, num_trades, max_dd);
        println!("🔬 [WORKER {}] Sortino={:.4}, Calmar={:.4}, weights={:?}",
            self.worker_id, sortino, calmar, weights);
        println!("🔬 [WORKER {}] Consistency={:.4}, weighted_fitness={:.6}",
            self.worker_id, wf_summary.consistency_score, fitness);
        
        println!("🔬 [WORKER {}] Aggregated {} per-trade returns from {} windows for Monte Carlo",
            self.worker_id, all_trade_returns.len(), wf_summary.num_windows);
        
        // Create a synthetic BacktestResult for Monte Carlo using PER-TRADE returns
        // This provides statistically valid sample sizes (n=100-1000+ trades)
        use bigdecimal::FromPrimitive;
        let synthetic_backtest_result = backtest::types::BacktestResult {
            total_pnl: bigdecimal::BigDecimal::from_f64(net_pnl).unwrap_or_default(),
            num_trades: all_trade_returns.len(),
            closed_trades: all_trade_returns.len(),
            sharpe_ratio: Some(sharpe),
            max_drawdown: bigdecimal::BigDecimal::from_f64(max_dd).unwrap_or_default(),
            win_rate: Some(win_rate),
            profit_factor: Some(profit_factor),
            trade_returns: all_trade_returns.clone(),  // Per-TRADE returns (not per-window)
            equity_curve: aggregate_equity_curve.clone(),
            initial_capital: request.initial_capital,
            ..Default::default()
        };
        
        // Run Monte Carlo using per-trade returns (statistically valid sample size)
        let monte_carlo_results = if request.run_monte_carlo && !all_trade_returns.is_empty() {
            // Use request parameter, cap at 200 for speed (1000 is excessive for real-time use)
            let mc_runs = (request.monte_carlo_runs as usize).min(200).max(100);
            println!("🔬 [WORKER {}] Running Monte Carlo simulation ({} runs) on {} trades...", 
                self.worker_id, mc_runs, all_trade_returns.len());
            log_info!(MAIN_LOGGER, "Running Monte Carlo simulation for job {} with {} trades ({} runs)", 
                request.job_id, all_trade_returns.len(), mc_runs);
            match backtest::monte_carlo::run_monte_carlo_simulation(&synthetic_backtest_result, mc_runs) {
                Ok(mc_result) => {
                    println!("🔬 [WORKER {}] Monte Carlo complete: VaR95={:.2}, CVaR95={:.2}", 
                        self.worker_id, mc_result.var_95, mc_result.cvar_95);
                    log_info!(MAIN_LOGGER, "Monte Carlo complete: VaR95={:.2}, CVaR95={:.2}", 
                        mc_result.var_95, mc_result.cvar_95);
                    serde_json::to_vec(&mc_result).unwrap_or_default()
                }
                Err(e) => {
                    println!("❌ [WORKER {}] Monte Carlo ERROR: {}", self.worker_id, e);
                    log_error_structured!(MAIN_LOGGER, "MONTE_CARLO_ERROR",
                        "event" => "MONTE_CARLO_ERROR",
                        "job_id" => &request.job_id,
                        "error" => e.to_string()
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        
        // Compute statistical significance of Sharpe ratio using PER-TRADE returns
        // This provides statistically valid sample sizes (n=100-1000+ trades)
        let statistical_significance = if !all_trade_returns.is_empty() {
            println!("🔬 [WORKER {}] Computing statistical significance from {} trades...", 
                self.worker_id, all_trade_returns.len());
            // For trade-level returns, assume ~1000 trades/day = 252,000 trades/year
            // Annualization factor depends on trading frequency, use sqrt(252*trades_per_day)
            // Conservative estimate: sqrt(252) for daily Sharpe equivalent
            let annualization = 252.0_f64.sqrt();
            let sig_result = compute_sharpe_significance(&all_trade_returns, 0.0, annualization);
            
            // Compute deflated Sharpe (assume ~50 trials for typical GA optimization)
            let num_trials = 50; // Conservative estimate of independent trials
            // Calculate expected max Sharpe under null for the number of trials
            let expected_max_null = backtest::statistical_significance::expected_max_sharpe_under_null(
                num_trials, 
                all_trade_returns.len()
            );
            let dsr = compute_deflated_sharpe_ratio(
                sig_result.sharpe_ratio, 
                sig_result.sharpe_std_error, 
                num_trials,
                expected_max_null
            );
            
            println!("📊 [WORKER {}] Statistical significance: Sharpe={:.4}, t-stat={:.4}, p-value={:.6}, PSR={:.2}%, DSR={:.2}%",
                self.worker_id, sig_result.sharpe_ratio, sig_result.t_statistic, sig_result.p_value, 
                sig_result.probabilistic_sharpe_ratio * 100.0, dsr * 100.0);
            println!("📊 [WORKER {}] Confidence Intervals: 95%=[{:.4}, {:.4}], min track record={:.0} trades",
                self.worker_id, sig_result.ci_95_lower, sig_result.ci_95_upper,
                sig_result.min_track_record_length);
            
            // Log significance assessment
            let significance_level = if sig_result.p_value < 0.01 {
                "HIGHLY SIGNIFICANT (p<0.01)"
            } else if sig_result.p_value < 0.05 {
                "SIGNIFICANT (p<0.05)"
            } else if sig_result.p_value < 0.10 {
                "MARGINALLY SIGNIFICANT (p<0.10)"
            } else {
                "NOT STATISTICALLY SIGNIFICANT"
            };
            println!("📊 [WORKER {}] Assessment: {} (n={})", self.worker_id, significance_level, all_trade_returns.len());
            
            // Create JSON blob for storage
            serde_json::json!({
                "sharpe": sig_result.sharpe_ratio,
                "sharpe_std_error": sig_result.sharpe_std_error,
                "t_statistic": sig_result.t_statistic,
                "p_value": sig_result.p_value,
                "probabilistic_sharpe_ratio": sig_result.probabilistic_sharpe_ratio,
                "deflated_sharpe_ratio": dsr,
                "confidence_interval_95": [sig_result.ci_95_lower, sig_result.ci_95_upper],
                "min_track_record_length": sig_result.min_track_record_length,
                "is_significant_at_5pct": sig_result.is_significant_5pct,
                "is_significant_at_1pct": sig_result.is_significant_1pct,
                "skewness": sig_result.skewness,
                "kurtosis": sig_result.kurtosis,
                "num_observations": sig_result.n_observations
            }).to_string()
        } else {
            String::new()
        };
        
        // Walk-forward results are already computed as primary validation above
        // Serialize the summary for the response
        let walk_forward_results = serde_json::to_vec(&wf_summary).unwrap_or_default();
        
        // Serialize per-trade returns for storage (for downstream Monte Carlo or analysis)
        let closed_trades_data = serde_json::to_vec(&all_trade_returns).unwrap_or_default();
        
        let elapsed = start.elapsed();
        
        log_info!(MAIN_LOGGER, "Final evaluation complete for job {}: PnL={:.2}, Sharpe={:.4}, trades={}, elapsed={:.2}s",
            request.job_id, net_pnl, sharpe, num_trades, elapsed.as_secs_f64());
        
        // Sample equity curve to avoid huge payloads
        let sampled_curve = sample_equity_curve(&aggregate_equity_curve, MAX_EQUITY_CURVE_POINTS);
        
        // Send comprehensive result
        let response = protocol::broker::messages::ChromosomeEvalResult {
            job_id: request.job_id.clone(),
            generation: request.generation,
            chromosome_id: request.chromosome_id,
            worker_id: self.worker_id.clone(),
            success: true,
            error_message: String::new(),
            fitness,
            net_pnl,
            sharpe_ratio: sharpe,
            num_trades,
            max_drawdown: max_dd,
            equity_curve: serde_json::to_vec(&sampled_curve).unwrap_or_default(),
            processing_duration_ms: elapsed.as_millis() as i64,
            win_rate,
            profit_factor,
            closed_trades,
            total_orders,
            starting_capital: request.initial_capital,
            ending_capital,
            total_return_pct,
            monte_carlo_results,
            walk_forward_results,
            closed_trades_data,
            is_final_evaluation: true,
            statistical_significance,
        };
        
        println!("📤 [WORKER {}] Publishing FINAL result: job={}, pnl={:.2}, trades={}, is_final=true",
            self.worker_id, request.job_id, net_pnl, num_trades);
        
        match self.broker.publish_chromosome_result(response).await {
            Ok(_) => println!("✅ [WORKER {}] Final result published successfully", self.worker_id),
            Err(e) => println!("❌ [WORKER {}] Failed to publish final result: {}", self.worker_id, e),
        }
    }
    
    /// Create a lightweight clone for spawning tasks
    fn clone_for_task(&self) -> Self {
        Self {
            worker_id: self.worker_id.clone(),
            broker: Arc::clone(&self.broker),
            binary_cache: Arc::clone(&self.binary_cache),
            l3_binary_cache: Arc::clone(&self.l3_binary_cache),
            data_cache: Arc::clone(&self.data_cache),
            loading_in_progress: Arc::clone(&self.loading_in_progress),
            eval_semaphore: Arc::clone(&self.eval_semaphore),
            data_dir: self.data_dir.clone(),
            #[cfg(feature = "postgres")]
            db_pool: self.db_pool.clone(),
        }
    }
}

/// Errors from the GA worker
#[derive(Debug)]
pub enum WorkerError {
    BrokerError(String),
    CacheError(String),
    DatabaseError(String),
    InvalidTimestamp(String),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerError::BrokerError(e) => write!(f, "Broker error: {}", e),
            WorkerError::CacheError(e) => write!(f, "Cache error: {}", e),
            WorkerError::DatabaseError(e) => write!(f, "Database error: {}", e),
            WorkerError::InvalidTimestamp(e) => write!(f, "Invalid timestamp: {}", e),
        }
    }
}

impl std::error::Error for WorkerError {}

/// Run walk-forward analysis using REALISTIC simulation with L3 order book data
/// 
/// This is implemented here (rather than in walkforward crate) to avoid circular dependency.
/// The backtest crate depends on walkforward, so walkforward cannot depend on backtest.
fn run_walkforward_realistic(
    events: &[backtest::realistic_simulation::OrderBookEvent],
    chromosome: &genetic::chromosome::AvellanedaStoikovChromosome,
    exchange: &str,
    initial_capital: f64,
    initial_30d_volume: f64,
    train_days: Option<i64>,
    test_days: Option<i64>,
    step_days: Option<i64>,
) -> walkforward::WalkForwardSummary {
    use backtest::realistic_simulation::{RealisticSimulator, RealisticASParams, EventType};
    use chrono::NaiveDate;
    use std::collections::BTreeMap;
    
    let train_days = train_days.unwrap_or(30);
    let test_days = test_days.unwrap_or(7);
    let step_days = step_days.unwrap_or(7);
    
    if events.is_empty() {
        log_warn!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: No events provided");
        return walkforward::WalkForwardSummary::new(vec![]);
    }
    
    // Get date range from events
    let first_ts = events.first().map(|e| e.timestamp_ms).unwrap_or(0);
    let last_ts = events.last().map(|e| e.timestamp_ms).unwrap_or(0);
    let start_date = chrono::DateTime::from_timestamp_millis(first_ts)
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    let end_date = chrono::DateTime::from_timestamp_millis(last_ts)
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    
    // Log event breakdown
    let new_count = events.iter().filter(|e| matches!(e.event_type, EventType::New)).count();
    let trade_count = events.iter().filter(|e| matches!(e.event_type, EventType::Trade)).count();
    log_info!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: {} to {} | {} events ({} new, {} trades) | train={}d test={}d step={}d",
        start_date, end_date, events.len(), new_count, trade_count, train_days, test_days, step_days);
    
    // Generate windows using the same logic as walkforward crate
    let train_size = chrono::Duration::days(train_days);
    let test_size = chrono::Duration::days(test_days);
    let step_size = chrono::Duration::days(step_days);
    let windows = generate_walkforward_windows(start_date, end_date, train_size, test_size, step_size);
    
    if windows.is_empty() {
        log_warn!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: Data range too short for windows");
        return walkforward::WalkForwardSummary::new(vec![]);
    }
    
    log_info!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: {} windows generated", windows.len());
    
    // Build timestamp->index mapping for efficient window slicing
    let mut date_indices: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    let mut last_date: Option<NaiveDate> = None;
    for (i, event) in events.iter().enumerate() {
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(event.timestamp_ms) {
            let event_date = dt.date_naive();
            if last_date.map_or(true, |d| d != event_date) {
                date_indices.entry(event_date).or_insert(i);
                last_date = Some(event_date);
            }
        }
    }
    
    // Get exchange config for realistic simulation with proper fee tier
    let mut exchange_fee_config = config::ExchangeFeeConfig::for_exchange(exchange);
    if initial_30d_volume > 0.0 {
        exchange_fee_config.starting_30d_volume = initial_30d_volume;
        exchange_fee_config.maker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, true);
        exchange_fee_config.taker_fee = exchange_fee_config.get_fee_rate(initial_30d_volume, false);
    }
    
    // Build realistic params from chromosome
    let realistic_params = RealisticASParams {
        risk_aversion: chromosome.risk_aversion,
        inventory_target: chromosome.inventory_target,
        order_size_pct: chromosome.order_size_pct,
        volatility_cap: chromosome.volatility_cap,
        min_spread_bps: chromosome.min_spread_bps,
        time_horizon_minutes: chromosome.time_horizon_minutes,
        maker_fee_rate: exchange_fee_config.maker_fee,
        taker_fee_rate: exchange_fee_config.taker_fee,
        max_inventory_multiple: 5.0,
        latency_mean_ms: exchange_fee_config.latency_mean_ms,
        latency_std_ms: exchange_fee_config.latency_std_ms,
        enable_partial_fills: true,
        min_fill_fraction: 0.1,
        max_orders_per_second: exchange_fee_config.max_orders_per_second,
        rate_limit_cooldown_ms: exchange_fee_config.rate_limit_cooldown_ms,
        funding_rate_8h: exchange_fee_config.funding_rate_8h,
        funding_interval_ms: exchange_fee_config.funding_interval_ms,
        adverse_selection_sensitivity: chromosome.adverse_selection_sensitivity,
    };
    
    // Process each window using RealisticSimulator
    let mut window_results = Vec::with_capacity(windows.len());
    
    for (window_idx, window) in windows.iter().enumerate() {
        // Find event range for TEST window (we're validating on test data, not training)
        let test_start_idx = date_indices.range(..=window.test_start).next_back()
            .map(|(_, &idx)| idx).unwrap_or(0);
        let test_end_idx = date_indices.range(window.test_end..).next()
            .map(|(_, &idx)| idx).unwrap_or(events.len());
        
        let window_events = &events[test_start_idx..test_end_idx];
        
        if window_events.is_empty() {
            log_warn!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: Window {}/{} empty", window_idx + 1, windows.len());
            continue;
        }
        
        // Run RealisticSimulator on this window
        let simulator = RealisticSimulator::with_exchange_config(initial_capital, exchange_fee_config.clone());
        let result = simulator.run(window_events, &realistic_params);
        
        log_info!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: Window {}/{} | {} events | trades={} sharpe={:.4} pnl={:.2}",
            window_idx + 1, windows.len(), window_events.len(), result.num_trades, result.sharpe_ratio, result.total_pnl);
        
        let test_metrics = metrics::PerformanceMetrics {
            net_profit: result.total_pnl,
            sharpe_ratio: result.sharpe_ratio,
            max_drawdown: result.max_drawdown,
            number_of_trades: result.num_trades,
            average_trade_return: if result.num_trades > 0 { result.total_pnl / result.num_trades as f64 } else { 0.0 },
            win_rate: result.win_rate,
            profit_factor: result.profit_factor,
            ..Default::default()
        };
        
        window_results.push(walkforward::WalkForwardWindowResult {
            train_start: window.train_start,
            train_end: window.train_end,
            test_start: window.test_start,
            test_end: window.test_end,
            best_params: serde_json::json!({
                "risk_aversion": chromosome.risk_aversion,
                "order_size_pct": chromosome.order_size_pct,
                "inventory_target": chromosome.inventory_target,
                "volatility_cap": chromosome.volatility_cap,
                "time_horizon_minutes": chromosome.time_horizon_minutes,
            }),
            test_metrics,
            trade_returns: result.trade_returns,  // Collect per-trade returns for Monte Carlo
        });
    }
    
    let summary = walkforward::WalkForwardSummary::new(window_results);
    
    log_info!(MAIN_LOGGER, "WALKFORWARD_REALISTIC: Complete | windows={} | avg_sharpe={:.4} | consistency={:.4}",
        summary.num_windows, summary.average_test_sharpe, summary.consistency_score);
    
    summary
}

/// Simple window structure for walk-forward
struct WalkForwardWindow {
    train_start: chrono::NaiveDate,
    train_end: chrono::NaiveDate,
    test_start: chrono::NaiveDate,
    test_end: chrono::NaiveDate,
}

/// Generate walk-forward windows (reimplemented here to avoid circular dependency)
fn generate_walkforward_windows(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    train_size: chrono::Duration,
    test_size: chrono::Duration,
    step_size: chrono::Duration,
) -> Vec<WalkForwardWindow> {
    let mut windows = Vec::new();
    let mut current_train_start = start;
    
    loop {
        let train_end = current_train_start + train_size;
        let test_start = train_end;
        let test_end = test_start + test_size;
        
        // Stop if test window would go past end date
        if test_end > end {
            break;
        }
        
        windows.push(WalkForwardWindow {
            train_start: current_train_start,
            train_end,
            test_start,
            test_end,
        });
        
        current_train_start = current_train_start + step_size;
    }
    
    windows
}
