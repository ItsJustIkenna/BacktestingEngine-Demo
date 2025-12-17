//! Distributed GA Worker
//!
//! Receives chromosome evaluation requests via MessageBrokerEngine,
//! evaluates them against cached market data, and returns fitness results.
//!
//! # Data Caching
//! Workers maintain an in-memory cache of market data. When a DataBroadcastRequest
//! is received, the data is decompressed and stored. Subsequent chromosome evaluations
//! use the cached data (no database access needed).
//!
//! # Concurrency
//! Each worker can handle multiple concurrent evaluations (configurable via
//! MAX_CONCURRENT_EVALS). The semaphore prevents OOM by limiting parallelism.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};

use dataloader::MarketData;
use genetic::chromosome::AvellanedaStoikovChromosome;
use genetic::FitnessResult;

use super::broker_client::BacktestBrokerClient;
use crate::worker::calculate_comprehensive_fitness;
use crate::{log_info_structured, log_debug, log_error_structured};
use crate::logging_facade::MAIN_LOGGER;

/// Maximum concurrent chromosome evaluations per worker
const MAX_CONCURRENT_EVALS: usize = 6;

/// Data cache entry with TTL
struct CacheEntry {
    data: Arc<Vec<MarketData>>,
    expires_at: Instant,
}

/// Distributed GA Worker
/// 
/// Processes chromosome evaluation requests from the message broker.
/// Caches market data to avoid repeated database loads.
pub struct DistributedGAWorker {
    /// Unique worker ID
    worker_id: String,
    /// Broker client for communication
    broker: Arc<BacktestBrokerClient>,
    /// Cached market data by cache key
    data_cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Semaphore for limiting concurrent evaluations
    eval_semaphore: Arc<Semaphore>,
    /// Database pool for loading data if not cached
    #[cfg(feature = "postgres")]
    db_pool: Option<Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>>,
}

impl DistributedGAWorker {
    /// Create a new GA worker
    pub fn new(worker_id: String, broker: Arc<BacktestBrokerClient>) -> Self {
        Self {
            worker_id,
            broker,
            data_cache: Arc::new(RwLock::new(HashMap::new())),
            eval_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_EVALS)),
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
                    // Check if we have the required data cached
                    let has_data = {
                        let cache = self.data_cache.read().await;
                        cache.contains_key(&request.data_cache_key)
                    };
                    
                    if has_data {
                        // Spawn evaluation task with semaphore
                        let worker = self.clone_for_task();
                        let request_clone = request.clone();
                        
                        tokio::spawn(async move {
                            worker.process_chromosome_eval(request_clone).await;
                        });
                    } else {
                        log_debug!(MAIN_LOGGER, "Worker {} missing data for cache_key {}, skipping",
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
            cache.insert(broadcast.cache_key.clone(), CacheEntry {
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
        };
        
        let _ = self.broker.publish_data_ack(ack).await;
    }
    
    /// Handle data load request by querying the database directly
    /// 
    /// This is the preferred method for large datasets. The coordinator sends
    /// query parameters (~200 bytes) and each worker loads data from the database.
    /// This avoids serializing/transferring 500MB+ over the message broker.
    async fn handle_data_load_request(
        &self,
        request: protocol::broker::messages::DataLoadRequest,
    ) {
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DATA_LOAD_START",
            "event" => "GA_WORKER_DATA_LOAD_START",
            "worker_id" => &self.worker_id,
            "cache_key" => &request.cache_key,
            "symbols" => format!("{:?}", request.symbols),
            "exchange" => &request.exchange,
            "start_time" => &request.start_time,
            "end_time" => &request.end_time
        );
        
        let start = Instant::now();
        
        // Load data from database
        #[cfg(feature = "postgres")]
        let market_data = match &self.db_pool {
            Some(pool) => {
                match self.load_data_from_db(
                    pool.as_ref(),
                    &request.symbols,
                    &request.exchange,
                    &request.start_time,
                    &request.end_time,
                ).await {
                    Ok(data) => data,
                    Err(e) => {
                        log_error_structured!(MAIN_LOGGER, "GA_WORKER_DB_LOAD_FAIL",
                            "event" => "GA_WORKER_DB_LOAD_FAIL",
                            "worker_id" => &self.worker_id,
                            "error" => e.to_string()
                        );
                        // Send failure ack
                        let ack = protocol::broker::messages::DataCacheAck {
                            cache_key: request.cache_key,
                            worker_id: self.worker_id.clone(),
                            success: false,
                            capacity: 0,
                        };
                        let _ = self.broker.publish_data_ack(ack).await;
                        return;
                    }
                }
            }
            None => {
                log_error_structured!(MAIN_LOGGER, "GA_WORKER_NO_DB_POOL",
                    "event" => "GA_WORKER_NO_DB_POOL",
                    "worker_id" => &self.worker_id
                );
                return;
            }
        };
        
        #[cfg(not(feature = "postgres"))]
        let market_data: Vec<MarketData> = {
            log_error_structured!(MAIN_LOGGER, "GA_WORKER_NO_POSTGRES_FEATURE",
                "event" => "GA_WORKER_NO_POSTGRES_FEATURE",
                "worker_id" => &self.worker_id
            );
            return;
        };
        
        let data_len = market_data.len();
        let expires_at = Instant::now() + Duration::from_secs(request.ttl_seconds as u64);
        
        // Store in cache
        {
            let mut cache = self.data_cache.write().await;
            cache.insert(request.cache_key.clone(), CacheEntry {
                data: Arc::new(market_data),
                expires_at,
            });
        }
        
        let elapsed = start.elapsed();
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DATA_LOADED",
            "event" => "GA_WORKER_DATA_LOADED",
            "worker_id" => &self.worker_id,
            "cache_key" => &request.cache_key,
            "data_points" => data_len,
            "load_time_ms" => elapsed.as_millis()
        );
        
        // Send success acknowledgment
        let ack = protocol::broker::messages::DataCacheAck {
            cache_key: request.cache_key,
            worker_id: self.worker_id.clone(),
            success: true,
            capacity: MAX_CONCURRENT_EVALS as i32,
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
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DB_QUERY",
            "event" => "GA_WORKER_DB_QUERY",
            "worker_id" => &self.worker_id,
            "symbol" => &symbol,
            "exchange" => exchange
        );
        
        // Use parallel loader with 10 workers for faster loading
        const NUM_WORKERS: usize = 10;
        let loader = ParallelDataLoader::new(
            std::sync::Arc::new(pool.clone()),
            NUM_WORKERS,
        );
        
        // Load all data using time-based partitioning for parallelism
        let market_data = loader.load_all(&symbol, exchange).await
            .map_err(|e| WorkerError::DatabaseError(e.to_string()))?;
        
        log_info_structured!(MAIN_LOGGER, "GA_WORKER_DB_QUERY_COMPLETE",
            "event" => "GA_WORKER_DB_QUERY_COMPLETE",
            "worker_id" => &self.worker_id,
            "rows_loaded" => market_data.len()
        );
        
        Ok(market_data)
    }
    
    /// Process a chromosome evaluation request
    async fn process_chromosome_eval(
        &self,
        request: protocol::broker::messages::ChromosomeEvalRequest,
    ) {
        // Acquire semaphore permit (limits concurrent evals)
        let _permit = self.eval_semaphore.acquire().await.expect("semaphore closed");
        
        let start = Instant::now();
        
        log_debug!(MAIN_LOGGER, "Worker {} processing chromosome {}/{} for gen {}",
            self.worker_id, request.chromosome_id, request.total_chromosomes, request.generation);
        
        // Get cached data
        let data = {
            let cache = self.data_cache.read().await;
            match cache.get(&request.data_cache_key) {
                Some(entry) if entry.expires_at > Instant::now() => {
                    Arc::clone(&entry.data)
                }
                _ => {
                    // Data expired or missing, send error result
                    self.send_error_result(&request, "Data not in cache or expired").await;
                    return;
                }
            }
        };
        
        // Deserialize chromosome
        let chromosome: AvellanedaStoikovChromosome = match serde_json::from_slice(&request.chromosome_params) {
            Ok(c) => c,
            Err(e) => {
                self.send_error_result(&request, &format!("Failed to deserialize chromosome: {}", e)).await;
                return;
            }
        };
        
        // Run fitness evaluation
        let result = self.evaluate_chromosome(&chromosome, &data, request.initial_capital).await;
        
        let elapsed = start.elapsed();
        
        // Send result
        let response = protocol::broker::messages::ChromosomeEvalResult {
            job_id: request.job_id,
            generation: request.generation,
            chromosome_id: request.chromosome_id,
            worker_id: self.worker_id.clone(),
            success: true,
            error_message: String::new(),
            fitness: result.fitness,
            net_pnl: 0.0, // TODO: Extract from backtest result
            sharpe_ratio: 0.0,
            num_trades: 0,
            max_drawdown: 0.0,
            equity_curve: serde_json::to_vec(&result.equity_curve).unwrap_or_default(),
            processing_duration_ms: elapsed.as_millis() as i64,
        };
        
        let _ = self.broker.publish_chromosome_result(response).await;
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
        };
        
        let _ = self.broker.publish_chromosome_result(response).await;
    }
    
    /// Create a lightweight clone for spawning tasks
    fn clone_for_task(&self) -> Self {
        Self {
            worker_id: self.worker_id.clone(),
            broker: Arc::clone(&self.broker),
            data_cache: Arc::clone(&self.data_cache),
            eval_semaphore: Arc::clone(&self.eval_semaphore),
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
