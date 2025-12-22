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
use dataloader::Tick; // Needed for price() method
use bigdecimal::ToPrimitive; // Needed for to_f64()
use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::FitnessResult;

use super::broker_client::BacktestBrokerClient;
use crate::worker::calculate_comprehensive_fitness;
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

/// Cache entry for binary file metadata
pub struct BinaryFileInfo {
    path: PathBuf,
    #[allow(dead_code)]
    tick_count: usize,
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
    /// Binary file info by cache key (preferred path)
    binary_cache: Arc<RwLock<HashMap<String, BinaryFileInfo>>>,
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
    fn probe_binary_file(&self, symbol: &str, exchange: &str) -> Option<(PathBuf, usize)> {
        use data_prep::BinaryFileReader;
        
        let path = self.binary_path(symbol, exchange);
        if !path.exists() {
            return None;
        }
        
        match BinaryFileReader::open(&path) {
            Ok(reader) => Some((path, reader.tick_count() as usize)),
            Err(_) => None,
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
                    // Check if we have the required data cached (either binary file path or in-memory data)
                    let has_data = {
                        let binary_cache = self.binary_cache.read().await;
                        let data_cache = self.data_cache.read().await;
                        let in_binary = binary_cache.contains_key(&request.data_cache_key);
                        let in_data = data_cache.contains_key(&request.data_cache_key);
                        println!("🔍 [WORKER {}] Checking cache for {}: binary={}, data={}", 
                            self.worker_id, request.data_cache_key, in_binary, in_data);
                        in_binary || in_data
                    };
                    
                    if has_data {
                        println!("✅ [WORKER {}] Processing chromosome {}/{} for job {}", 
                            self.worker_id, request.chromosome_id, request.total_chromosomes, request.job_id);
                        // Spawn evaluation task with semaphore
                        let worker = self.clone_for_task();
                        let request_clone = request.clone();
                        
                        tokio::spawn(async move {
                            worker.process_chromosome_eval(request_clone).await;
                        });
                    } else {
                        println!("⏭️ [WORKER {}] Skipping chromosome - no data for cache_key {}", 
                            self.worker_id, request.data_cache_key);
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
        
        // Send acknowledgment
        let ack = protocol::broker::messages::DataCacheAck {
            cache_key: broadcast.cache_key,
            worker_id: self.worker_id.clone(),
            success: true,
            capacity: MAX_CONCURRENT_EVALS as i32,
            tick_count: 0,
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
            if binary_cache.contains_key(&cache_key) {
                println!("✅ [WORKER {}] Binary file already cached for {}", self.worker_id, cache_key);
                let ack = protocol::broker::messages::DataCacheAck {
                    cache_key,
                    worker_id: self.worker_id.clone(),
                    success: true,
                    capacity: MAX_CONCURRENT_EVALS as i32,
                    tick_count: 0,
                };
                let _ = self.broker.publish_data_ack(ack).await;
                return;
            }
        }
        {
            let data_cache = self.data_cache.read().await;
            if data_cache.contains_key(&cache_key) {
                println!("✅ [WORKER {}] Data already cached for {}", self.worker_id, cache_key);
                let ack = protocol::broker::messages::DataCacheAck {
                    cache_key,
                    worker_id: self.worker_id.clone(),
                    success: true,
                    capacity: MAX_CONCURRENT_EVALS as i32,
                    tick_count: 0,
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
                
                // Wait for completion
                for _ in 0..120 {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let bc = self.binary_cache.read().await;
                    let dc = self.data_cache.read().await;
                    if bc.contains_key(&cache_key) || dc.contains_key(&cache_key) {
                        let ack = protocol::broker::messages::DataCacheAck {
                            cache_key,
                            worker_id: self.worker_id.clone(),
                            success: true,
                            capacity: MAX_CONCURRENT_EVALS as i32,
                            tick_count: 0,
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
                tick_count: 0,
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
                                tick_count: 0,
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
                            tick_count: 0,
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
    /// This eliminates the need for manual `prepare-data` CLI invocation.
    /// Creates a memory-mapped binary file for efficient tick access.
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
        
        let prep_start = std::time::Instant::now();
        let result = data_prep::prepare_data(pool, &config).await
            .map_err(|e| WorkerError::DatabaseError(e.to_string()))?;
        let prep_elapsed = prep_start.elapsed();
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_AUTO_PREPARE_COMPLETE",
            "event" => "GA_WORKER_AUTO_PREPARE_COMPLETE",
            "worker_id" => &self.worker_id,
            "symbol" => symbol,
            "exchange" => exchange,
            "ticks" => result.total_ticks,
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
                self.evaluate_chromosome(&chromosome, &data, request.initial_capital).await
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
        };
        
        println!("📤 [WORKER {}] Sending result for chromosome {}/{} fitness={:.4} in {:?}", 
            self.worker_id, request.chromosome_id, request.total_chromosomes, result.fitness, elapsed);
        let _ = self.broker.publish_chromosome_result(response).await;
    }
    
    /// Evaluate chromosome using FAST vectorized simulation
    /// 
    /// This uses pre-computed market features for 50-100x speedup over event-by-event simulation.
    /// Memory-efficient: reads from mmap'd binary file without loading into RAM.
    /// 
    /// If `train_end_idx` > 0, only the first `train_end_idx` ticks are used for training.
    async fn evaluate_with_streaming_sim(
        &self,
        chromosome: &AvellanedaStoikovChromosome,
        path: &std::path::Path,
        initial_capital: f64,
        train_end_idx: i64,
    ) -> FitnessResult {
        use data_prep::BinaryFileReader;
        use dataloader::{Tick, TickSource};
        use backtest::fast_simulation::{FastSimulator, FastASParams, MarketFeatures};
        
        let sim_start = std::time::Instant::now();
        
        // Load binary file as TickSource (mmap, zero-copy)
        let reader = match BinaryFileReader::open(path) {
            Ok(r) => r,
            Err(e) => {
                println!("❌ [WORKER {}] Binary file error: {}", self.worker_id, e);
                return FitnessResult::failure();
            }
        };
        
        let total_ticks = reader.len();
        
        // Calculate effective training range
        let effective_train_end_idx = if train_end_idx == 0 {
            (total_ticks as f64 * 0.6) as usize
        } else {
            train_end_idx as usize
        };
        
        println!("🚀 [WORKER {}] FAST sim on {} ticks (of {} total)", 
            self.worker_id, effective_train_end_idx, total_ticks);
        
        // Build market features from tick data (vectorized, fast)
        let features_start = std::time::Instant::now();
        let features = MarketFeatures::from_tick_source(&reader, effective_train_end_idx);
        println!("📊 [WORKER {}] Features computed in {:.2}s for {} points", 
            self.worker_id, features_start.elapsed().as_secs_f64(), features.len());
        
        // Convert chromosome to fast simulation parameters
        let fast_params = FastASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            fee_rate: 0.001, // Default 0.1% fee
        };
        
        // Run vectorized simulation (synchronous, fast!)
        let simulator = FastSimulator::new(&features, initial_capital);
        let fast_result = simulator.evaluate(&fast_params);
        
        let sim_elapsed = sim_start.elapsed();
        println!("✅ [WORKER {}] FAST sim complete in {:.2}s: PnL={:.2}, Sharpe={:.4}, Trades={}", 
            self.worker_id, sim_elapsed.as_secs_f64(), fast_result.total_pnl, fast_result.sharpe_ratio, fast_result.num_trades);
        
        // Convert to standard BacktestResult and calculate fitness
        let backtest_result: backtest::types::BacktestResult = fast_result.clone().into();
        let fitness = calculate_comprehensive_fitness(&backtest_result, initial_capital);
        
        FitnessResult::with_metrics(
            fitness,
            fast_result.equity_curve,
            fast_result.total_pnl,
            fast_result.sharpe_ratio,
            fast_result.num_trades as i32,
            fast_result.max_drawdown,
            fast_result.win_rate,
            fast_result.profit_factor,
        )
    }
    /// Evaluate a chromosome against the cached data
    async fn evaluate_chromosome(
        &self,
        chromosome: &AvellanedaStoikovChromosome,
        data: &[MarketData],
        initial_capital: f64,
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
        
        let inventory_pct = 0.5;
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
        };
        
        let _ = self.broker.publish_chromosome_result(response).await;
    }

    /// Process a final evaluation request with full backtest, Monte Carlo, and Walk-Forward
    async fn process_final_evaluation(
        &self,
        request: &protocol::broker::messages::ChromosomeEvalRequest,
        chromosome: &AvellanedaStoikovChromosome,
    ) {
        use data_prep::BinaryFileReader;
        use dataloader::{Tick, TickSource};
        use backtest::fast_simulation::{FastSimulator, FastASParams, MarketFeatures};
        use bigdecimal::ToPrimitive;
        
        let start = Instant::now();
        
        println!("🔬 [WORKER {}] FINAL EVALUATION started for job {}", self.worker_id, request.job_id);
        println!("🔬 [WORKER {}] Chromosome: risk_aversion={:.4}, order_size_pct={:.4}, inventory_target={:.4}",
            self.worker_id, chromosome.risk_aversion, chromosome.order_size_pct, chromosome.inventory_target);
        println!("🔬 [WORKER {}] Data splits: train_end_idx={}, test_end_idx={}, total_ticks={}",
            self.worker_id, request.train_end_idx, request.test_end_idx, request.total_ticks);
        
        // Get binary file path - for final evaluation, also check if file exists even if cache expired
        let path = {
            let binary_cache = self.binary_cache.read().await;
            // First try to get from cache (may be expired)
            binary_cache.get(&request.data_cache_key)
                .map(|info| info.path.clone())
        };
        
        let path: std::path::PathBuf = match path {
            Some(p) => {
                // Verify the file still exists on disk
                if p.exists() {
                    println!("🔬 [WORKER {}] Binary file path: {:?} (file exists)", self.worker_id, p);
                    p
                } else {
                    println!("❌ [WORKER {}] Binary file {:?} no longer exists on disk!", self.worker_id, p);
                    self.send_error_result(request, "Binary file no longer exists on disk").await;
                    return;
                }
            }
            None => {
                // Fallback: try to find the file in the data directory
                let fallback_path = self.data_dir.join("historical")
                    .join(format!("{}_{}_Kraken.bin", 
                        request.symbol.replace("/", "_"),
                        request.exchange));
                if fallback_path.exists() {
                    println!("🔬 [WORKER {}] Using fallback binary path: {:?}", self.worker_id, fallback_path);
                    fallback_path
                } else {
                    println!("❌ [WORKER {}] Binary data NOT in cache and fallback {:?} not found!", 
                        self.worker_id, fallback_path);
                    self.send_error_result(request, "Binary data not in cache for final evaluation").await;
                    return;
                }
            }
        };
        
        // Load binary file as TickSource
        let reader = match BinaryFileReader::open(&path) {
            Ok(r) => {
                println!("🔬 [WORKER {}] Binary file opened, {} ticks available", self.worker_id, r.len());
                r
            }
            Err(e) => {
                println!("❌ [WORKER {}] Failed to open binary file: {}", self.worker_id, e);
                self.send_error_result(request, &format!("Failed to open binary file: {}", e)).await;
                return;
            }
        };
        
        let total_ticks = reader.len();
        
        // Determine data range for final evaluation
        // Use VALIDATE portion: [test_end_idx, total_ticks) if configured, else full data
        let (start_idx, end_idx) = if request.test_end_idx > 0 && request.total_ticks > request.test_end_idx {
            (request.test_end_idx as usize, request.total_ticks as usize)
        } else {
            (0, total_ticks)
        };
        
        println!("🔬 [WORKER {}] Final backtest on ticks [{}, {}) - {} ticks", 
            self.worker_id, start_idx, end_idx, end_idx - start_idx);
        
        // Build market features for the target data range
        let features_start = std::time::Instant::now();
        let features = MarketFeatures::from_tick_source_range(&reader, start_idx, end_idx);
        println!("📊 [WORKER {}] Features computed in {:.2}s for {} points", 
            self.worker_id, features_start.elapsed().as_secs_f64(), features.len());
        
        // Convert chromosome to fast simulation parameters
        let fast_params = FastASParams {
            risk_aversion: chromosome.risk_aversion,
            inventory_target: chromosome.inventory_target,
            order_size_pct: chromosome.order_size_pct,
            volatility_cap: chromosome.volatility_cap,
            min_spread_bps: chromosome.min_spread_bps,
            time_horizon_minutes: chromosome.time_horizon_minutes,
            fee_rate: 0.001, // Default 0.1% fee
        };
        
        // Run vectorized simulation (fast!)
        let simulator = FastSimulator::new(&features, request.initial_capital);
        let fast_result = simulator.evaluate(&fast_params);
        
        // Convert to standard BacktestResult for fitness calculation
        let backtest_result: backtest::types::BacktestResult = fast_result.clone().into();
        
        // Extract metrics
        let net_pnl = fast_result.total_pnl;
        let sharpe = fast_result.sharpe_ratio;
        let num_trades = fast_result.num_trades as i32;
        let max_dd = fast_result.max_drawdown;
        let win_rate = fast_result.win_rate;
        let profit_factor = fast_result.profit_factor;
        let closed_trades = fast_result.num_trades as i32;
        let total_orders = (backtest_result.num_trades * 2) as i32; // Estimate
        let ending_capital = request.initial_capital + net_pnl;
        let total_return_pct = (net_pnl / request.initial_capital) * 100.0;
        let fitness = calculate_comprehensive_fitness(&backtest_result, request.initial_capital);
        
        println!("🔬 [WORKER {}] FINAL BACKTEST COMPLETE: PnL={:.2}, Sharpe={:.4}, trades={}, max_dd={:.4}",
            self.worker_id, net_pnl, sharpe, num_trades, max_dd);
        println!("🔬 [WORKER {}] equity_curve length={}, fitness={:.6}",
            self.worker_id, backtest_result.equity_curve.len(), fitness);
        
        // Run Monte Carlo if requested
        let monte_carlo_results = if request.run_monte_carlo {
            println!("🔬 [WORKER {}] Running Monte Carlo simulation...", self.worker_id);
            log_info!(MAIN_LOGGER, "Running Monte Carlo simulation for job {}", request.job_id);
            match backtest::monte_carlo::run_monte_carlo_simulation(&backtest_result, 1000) {
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
        
        // Run Walk-Forward analysis if requested
        // Walk-forward runs on the TEST portion of data: [train_end_idx, test_end_idx)
        let walk_forward_results = if request.run_walk_forward {
            log_info!(MAIN_LOGGER, "Running Walk-Forward analysis for job {} on TEST data [{}, {})", 
                request.job_id, request.train_end_idx, request.test_end_idx);
            let wf_start = Instant::now();
            
            // Specify the test data range for walk-forward
            let (wf_start_idx, wf_end_idx) = if request.train_end_idx > 0 && request.test_end_idx > request.train_end_idx {
                (Some(request.train_end_idx as usize), Some(request.test_end_idx as usize))
            } else {
                (None, None) // Use full data (backward compatibility)
            };
            
            // Use 30-day train, 7-day test windows with 7-day steps
            let wf_summary = walkforward::run_walkforward_for_chromosome(
                &reader,
                chromosome,
                &request.symbol,
                &request.exchange,
                request.initial_capital,
                Some(30), // train_days
                Some(7),  // test_days
                Some(7),  // step_days
                wf_start_idx,
                wf_end_idx,
            ).await;
            
            log_info!(MAIN_LOGGER, "Walk-Forward complete: windows={}, avg_sharpe={:.4}, consistency={:.4}, elapsed={:.2}s",
                wf_summary.num_windows, wf_summary.average_test_sharpe, wf_summary.consistency_score, wf_start.elapsed().as_secs_f64());
            
            serde_json::to_vec(&wf_summary).unwrap_or_default()
        } else {
            Vec::new()
        };
        
        // Serialize trade returns for storage
        let closed_trades_data = serde_json::to_vec(&backtest_result.trade_returns).unwrap_or_default();
        
        let elapsed = start.elapsed();
        
        log_info!(MAIN_LOGGER, "Final evaluation complete for job {}: PnL={:.2}, Sharpe={:.4}, trades={}, elapsed={:.2}s",
            request.job_id, net_pnl, sharpe, num_trades, elapsed.as_secs_f64());
        
        // Sample equity curve to avoid huge payloads
        let sampled_curve = sample_equity_curve(&backtest_result.equity_curve, MAX_EQUITY_CURVE_POINTS);
        
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
