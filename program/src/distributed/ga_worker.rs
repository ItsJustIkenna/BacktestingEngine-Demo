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
        
        // Subscribe to data broadcasts
        let mut data_receiver = self.broker.subscribe_data_broadcasts();
        
        // Subscribe to chromosome evaluation requests
        let mut eval_receiver = self.broker.subscribe_chromosome_requests();
        
        loop {
            tokio::select! {
                // Handle data broadcast (cache the data)
                Ok(broadcast) = data_receiver.recv() => {
                    self.handle_data_broadcast(broadcast).await;
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
        
        let _ = self.broker.publish_data_ack(ack);
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
        
        let _ = self.broker.publish_chromosome_result(response);
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
        
        let _ = self.broker.publish_chromosome_result(response);
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
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerError::BrokerError(e) => write!(f, "Broker error: {}", e),
            WorkerError::CacheError(e) => write!(f, "Cache error: {}", e),
        }
    }
}

impl std::error::Error for WorkerError {}
