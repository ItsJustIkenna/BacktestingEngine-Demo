//! Distributed Genetic Algorithm Coordinator
//!
//! Coordinates chromosome evaluation across multiple worker pods using
//! MessageBrokerEngine for communication. This enables horizontal scaling
//! of genetic optimization.
//!
//! # Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      GA COORDINATOR (this module)                       │
//! │  1. Broadcasts data to workers (once, cached)                           │
//! │  2. Publishes ChromosomeEvalRequest for each chromosome                 │
//! │  3. Collects ChromosomeEvalResult from workers                          │
//! │  4. Runs GA selection/crossover/mutation locally                        │
//! └───────────────────────┬───────────────────────────────────────────────┬─┘
//!                         │                                               │
//!                         ▼                                               ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                       MESSAGE BROKER ENGINE                             │
//! │  Topics:                                                                │
//! │    ga.data.broadcast.{cache_key}  → Workers cache market data           │
//! │    ga.chromosome.eval.{job_id}    → Workers evaluate chromosomes        │
//! │    ga.chromosome.result.{job_id}  → Coordinator receives results        │
//! └─────────────────────────────────────────────────────────────────────────┘
//!                    ▲                ▲                ▲
//!               ┌────┴───┐       ┌────┴───┐       ┌────┴───┐
//!               │Worker 1│       │Worker 2│       │Worker N│
//!               │(cached)│       │(cached)│       │(cached)│
//!               └────────┘       └────────┘       └────────┘
//! ```
//!
//! # Performance
//! - Data broadcast: ~36s once (vs 36s × N jobs currently)
//! - Chromosome distribution: ~176ns per message
//! - With 4 workers × 6 concurrent each = 24 parallel evaluations
//! - Generation time: 45 min → ~9 min (5x speedup)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use rand::SeedableRng;

use config::GeneticConfig;
use genetic::{Chromosome, FitnessResult};
use genetic::chromosome::AvellanedaStoikovChromosome;

use super::broker_client::BacktestBrokerClient;
use crate::{log_info_structured, log_warn, log_debug};
use crate::logging_facade::MAIN_LOGGER;

/// Timeout for waiting for all chromosome results
const EVAL_TIMEOUT_SECS: u64 = 900; // 15 minutes per generation max

/// Configuration for distributed GA
#[derive(Debug, Clone)]
pub struct DistributedGAConfig {
    /// GA configuration
    pub genetic_config: GeneticConfig,
    /// Trading symbol
    pub symbol: String,
    /// Exchange
    pub exchange: String,
    /// Initial capital
    pub initial_capital: f64,
    /// Data cache TTL in seconds
    pub cache_ttl_seconds: i64,
}

/// Worker capacity tracking
#[derive(Debug, Clone)]
struct WorkerInfo {
    #[allow(dead_code)]
    worker_id: String,
    capacity: i32,
    #[allow(dead_code)]
    last_seen: Instant,
}

/// Distributed GA Coordinator
/// 
/// Runs the GA algorithm locally but distributes chromosome fitness
/// evaluations across multiple worker pods via MessageBrokerEngine.
pub struct DistributedGACoordinator {
    /// Broker client for communication
    broker: Arc<BacktestBrokerClient>,
    /// Configuration
    config: DistributedGAConfig,
    /// Job ID for this optimization
    job_id: String,
    /// Known workers and their capacities
    workers: Arc<RwLock<HashMap<String, WorkerInfo>>>,
    /// Data cache key (set after broadcasting data)
    data_cache_key: Option<String>,
}

impl DistributedGACoordinator {
    /// Create a new distributed GA coordinator
    pub fn new(
        broker: Arc<BacktestBrokerClient>,
        config: DistributedGAConfig,
        job_id: String,
    ) -> Self {
        Self {
            broker,
            config,
            job_id,
            workers: Arc::new(RwLock::new(HashMap::new())),
            data_cache_key: None,
        }
    }
    
    /// Get the number of registered workers
    pub async fn worker_count(&self) -> usize {
        self.workers.read().await.len()
    }
    
    /// Get total evaluation capacity across all workers
    pub async fn total_capacity(&self) -> i32 {
        self.workers.read().await.values().map(|w| w.capacity).sum()
    }
    
    /// Broadcast market data to all workers for caching
    /// 
    /// This should be called once before running the GA.
    /// Workers will cache the data and subsequent chromosome evaluations
    /// will be fast. Returns cache key and number of workers that acknowledged.
    pub async fn broadcast_data(
        &mut self,
        data: &[dataloader::MarketData],
    ) -> Result<String, CoordinatorError> {
        let cache_key = format!(
            "{}:{}:{}",
            self.config.symbol,
            self.config.exchange,
            chrono::Utc::now().timestamp()
        );
        
        eprintln!("📡 BROADCAST: Starting data broadcast, {} data points", data.len());
        
        log_info_structured!(MAIN_LOGGER, "GA_DATA_BROADCAST_START",
            "event" => "GA_DATA_BROADCAST_START",
            "job_id" => &self.job_id,
            "cache_key" => &cache_key,
            "data_points" => data.len()
        );
        
        // Serialize and compress the data
        eprintln!("📡 BROADCAST: Serializing {} data points with bincode...", data.len());
        let serialize_start = std::time::Instant::now();
        let serialized = bincode::serialize(data)
            .map_err(|e| CoordinatorError::SerializationError(e.to_string()))?;
        eprintln!("📡 BROADCAST: Serialized to {} bytes in {:.2}s", serialized.len(), serialize_start.elapsed().as_secs_f64());
        
        // Use LZ4 compression for speed
        eprintln!("📡 BROADCAST: Compressing with LZ4...");
        let compress_start = std::time::Instant::now();
        let compressed = lz4_flex::compress_prepend_size(&serialized);
        eprintln!("📡 BROADCAST: Compressed to {} bytes in {:.2}s", compressed.len(), compress_start.elapsed().as_secs_f64());
        
        let compression_ratio = serialized.len() as f64 / compressed.len() as f64;
        log_info_structured!(MAIN_LOGGER, "GA_DATA_COMPRESSED",
            "event" => "GA_DATA_COMPRESSED",
            "original_bytes" => serialized.len(),
            "compressed_bytes" => compressed.len(),
            "ratio" => format!("{:.2}x", compression_ratio)
        );
        
        // Create broadcast request
        let request = protocol::broker::messages::DataBroadcastRequest {
            cache_key: cache_key.clone(),
            symbol: self.config.symbol.clone(),
            exchange: self.config.exchange.clone(),
            data_point_count: data.len() as i64,
            compressed_data: compressed,
            compression: "lz4".to_string(),
            ttl_seconds: self.config.cache_ttl_seconds,
        };
        
        // Publish to data broadcast topic
        // Workers subscribed to ga.data.broadcast will receive and cache
        eprintln!("📡 BROADCAST: Publishing to broker...");
        self.broker.publish_data_broadcast(request).await
            .map_err(|e| CoordinatorError::BrokerError(e.to_string()))?;
        eprintln!("📡 BROADCAST: Published successfully!");
        
        self.data_cache_key = Some(cache_key.clone());
        
        // Subscribe to worker acknowledgments
        let mut ack_receiver = self.broker.subscribe_data_acks();
        let workers = Arc::clone(&self.workers);
        let cache_key_clone = cache_key.clone();
        
        // Wait for workers to acknowledge (with timeout)
        eprintln!("📡 BROADCAST: Waiting for worker acknowledgments (10s timeout)...");
        let ack_timeout = Duration::from_secs(10);
        let ack_deadline = Instant::now() + ack_timeout;
        
        while Instant::now() < ack_deadline {
            match tokio::time::timeout(
                Duration::from_millis(500),
                ack_receiver.recv()
            ).await {
                Ok(Ok(ack)) => {
                    if ack.cache_key == cache_key_clone && ack.success {
                        let mut w = workers.write().await;
                        w.insert(ack.worker_id.clone(), WorkerInfo {
                            worker_id: ack.worker_id,
                            capacity: ack.capacity,
                            last_seen: Instant::now(),
                        });
                    }
                }
                Ok(Err(_)) => break, // Channel closed
                Err(_) => continue, // Timeout, keep waiting
            }
        }
        
        let worker_count = self.workers.read().await.len();
        log_info_structured!(MAIN_LOGGER, "GA_DATA_BROADCAST_COMPLETE",
            "event" => "GA_DATA_BROADCAST_COMPLETE",
            "cache_key" => &cache_key,
            "workers_ready" => worker_count,
            "total_capacity" => self.total_capacity().await
        );
        
        if worker_count == 0 {
            log_warn!(MAIN_LOGGER, "No workers acknowledged data broadcast - falling back to local evaluation");
        }
        
        Ok(cache_key)
    }
    
    /// Run distributed genetic optimization
    /// 
    /// This runs the GA algorithm, distributing chromosome evaluations
    /// across all available workers.
    pub async fn run(&self) -> Result<(AvellanedaStoikovChromosome, f64), CoordinatorError> {
        let cache_key = self.data_cache_key.as_ref()
            .ok_or(CoordinatorError::DataNotBroadcast)?;
        
        let config = &self.config.genetic_config;
        let job_tag = format!("[Job {}] ", self.job_id);
        let optimization_start = Instant::now();
        
        log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_START",
            "event" => "DISTRIBUTED_GA_START",
            "job_id" => &self.job_id,
            "population" => config.population_size,
            "generations" => config.generations
        );
        
        let mut rng = rand::rngs::StdRng::from_entropy();
        
        // Initialize population
        let mut population: Vec<AvellanedaStoikovChromosome> = (0..config.population_size)
            .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
            .collect();
        
        let mut best = population[0].clone();
        let mut best_fitness = 0.0;
        let mut best_equity_curve = Vec::new();
        let mut generations_without_improvement = 0;
        const CONVERGENCE_THRESHOLD: usize = 5;
        
        let elite_size = (config.population_size as f64 * 0.05).ceil() as usize;
        
        // Subscribe to results channel
        let mut result_receiver = self.broker.subscribe_chromosome_results();
        
        for generation in 0..config.generations {
            let generation_start = Instant::now();
            
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_GEN_START",
                "event" => "DISTRIBUTED_GA_GEN_START",
                "job_id" => &self.job_id,
                "generation" => generation + 1,
                "population_size" => population.len()
            );
            
            // Publish chromosome evaluation requests
            for (idx, chromo) in population.iter().enumerate() {
                let request = self.create_eval_request(generation as i32, idx as i32, chromo, cache_key)?;
                self.broker.publish_chromosome_request(request).await
                    .map_err(|e| CoordinatorError::BrokerError(e.to_string()))?;
            }
            
            // Collect results with timeout
            let mut results: HashMap<i32, FitnessResult> = HashMap::new();
            let deadline = Instant::now() + Duration::from_secs(EVAL_TIMEOUT_SECS);
            
            while results.len() < population.len() && Instant::now() < deadline {
                match tokio::time::timeout(
                    Duration::from_secs(30),
                    result_receiver.recv()
                ).await {
                    Ok(Ok(result)) => {
                        if result.generation == generation as i32 {
                            let fitness_result = FitnessResult::new(
                                result.fitness,
                                serde_json::from_slice(&result.equity_curve).unwrap_or_default()
                            );
                            results.insert(result.chromosome_id, fitness_result);
                            
                            if results.len() % 5 == 0 {
                                log_debug!(MAIN_LOGGER, "{}Gen {}: {}/{} results collected",
                                    job_tag, generation + 1, results.len(), population.len());
                            }
                        }
                    }
                    Ok(Err(_)) => break, // Channel closed
                    Err(_) => continue, // Timeout, keep waiting
                }
            }
            
            if results.len() < population.len() {
                log_warn!(MAIN_LOGGER, "{}Gen {}: Only received {}/{} results (timeout)",
                    job_tag, generation + 1, results.len(), population.len());
            }
            
            // Build fitness vector (use 0.0 for missing results)
            let fitness_results: Vec<FitnessResult> = (0..population.len() as i32)
                .map(|i| results.remove(&i).unwrap_or_else(FitnessResult::failure))
                .collect();
            
            let mut fitnesses: Vec<f64> = fitness_results.iter().map(|r| r.fitness).collect();
            
            // Sort population by fitness
            let mut indexed: Vec<(usize, &AvellanedaStoikovChromosome, f64, &FitnessResult)> = 
                population.iter()
                    .enumerate()
                    .zip(fitnesses.iter())
                    .zip(fitness_results.iter())
                    .map(|(((idx, c), &fit), result)| (idx, c, fit, result))
                    .collect();
            indexed.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            
            let previous_best_fitness = best_fitness;
            
            // Elitism
            let mut new_population = Vec::with_capacity(population.len());
            for i in 0..elite_size {
                new_population.push(indexed[i].1.clone());
                if indexed[i].2 > best_fitness {
                    best = indexed[i].1.clone();
                    best_fitness = indexed[i].2;
                    best_equity_curve = indexed[i].3.equity_curve.clone();
                }
            }
            
            // Check improvement
            let improvement = if previous_best_fitness > 0.0 {
                (best_fitness - previous_best_fitness) / previous_best_fitness
            } else {
                best_fitness - previous_best_fitness
            };
            
            if improvement > 0.001 {
                generations_without_improvement = 0;
            } else {
                generations_without_improvement += 1;
            }
            
            if generations_without_improvement >= CONVERGENCE_THRESHOLD {
                log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_CONVERGED",
                    "event" => "DISTRIBUTED_GA_CONVERGED",
                    "generation" => generation + 1,
                    "best_fitness" => best_fitness
                );
                break;
            }
            
            // Selection, crossover, mutation (same as local GA)
            while new_population.len() < population.len() {
                use rand::Rng;
                let (a, b) = (rng.gen_range(0..population.len()), rng.gen_range(0..population.len()));
                let winner: &AvellanedaStoikovChromosome = if fitnesses[a] > fitnesses[b] { &population[a] } else { &population[b] };
                new_population.push(winner.clone());
            }
            
            for i in (elite_size..new_population.len()).step_by(2) {
                use rand::Rng;
                if rng.gen_bool(config.crossover_rate) && i + 1 < new_population.len() {
                    let child = new_population[i].crossover(&new_population[i + 1], &mut rng);
                    new_population[i] = child;
                }
            }
            
            let adaptive_mutation_rate = config.mutation_rate * 
                (1.0 - (generation as f64 / config.generations as f64));
            
            for i in elite_size..new_population.len() {
                use rand::Rng;
                if rng.gen_bool(adaptive_mutation_rate) {
                    new_population[i].mutate(&mut rng, adaptive_mutation_rate);
                }
            }
            
            population = new_population;
            
            let gen_elapsed = generation_start.elapsed().as_secs_f64();
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_GEN_COMPLETE",
                "event" => "DISTRIBUTED_GA_GEN_COMPLETE",
                "generation" => generation + 1,
                "elapsed_secs" => gen_elapsed,
                "best_fitness" => best_fitness,
                "chromosomes_per_sec" => self.config.genetic_config.population_size as f64 / gen_elapsed
            );
        }
        
        let total_time = optimization_start.elapsed().as_secs_f64();
        log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_COMPLETE",
            "event" => "DISTRIBUTED_GA_COMPLETE",
            "job_id" => &self.job_id,
            "total_secs" => total_time,
            "best_fitness" => best_fitness
        );
        
        Ok((best, best_fitness))
    }
    
    /// Create a chromosome evaluation request message
    fn create_eval_request(
        &self,
        generation: i32,
        chromosome_id: i32,
        chromo: &AvellanedaStoikovChromosome,
        cache_key: &str,
    ) -> Result<protocol::broker::messages::ChromosomeEvalRequest, CoordinatorError> {
        let params = serde_json::to_vec(chromo)
            .map_err(|e| CoordinatorError::SerializationError(e.to_string()))?;
        
        Ok(protocol::broker::messages::ChromosomeEvalRequest {
            job_id: self.job_id.clone(),
            generation,
            chromosome_id,
            total_chromosomes: self.config.genetic_config.population_size as i32,
            chromosome_params: params,
            symbol: self.config.symbol.clone(),
            exchange: self.config.exchange.clone(),
            initial_capital: self.config.initial_capital,
            data_cache_key: cache_key.to_string(),
        })
    }
}

/// Errors from the distributed GA coordinator
#[derive(Debug)]
pub enum CoordinatorError {
    DataNotBroadcast,
    SerializationError(String),
    BrokerError(String),
    Timeout(String),
}

impl std::fmt::Display for CoordinatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoordinatorError::DataNotBroadcast => write!(f, "Data not broadcast to workers"),
            CoordinatorError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            CoordinatorError::BrokerError(e) => write!(f, "Broker error: {}", e),
            CoordinatorError::Timeout(e) => write!(f, "Timeout: {}", e),
        }
    }
}

impl std::error::Error for CoordinatorError {}

/// Configuration for whether to use distributed GA
#[derive(Debug, Clone, Default)]
pub struct DistributedMode {
    /// Enable distributed GA across worker pods
    pub enabled: bool,
    /// Minimum number of workers required (fallback to local if fewer)
    pub min_workers: usize,
    /// Broker connection string (from environment if not set)
    pub broker_url: Option<String>,
    /// Timeout for waiting for workers to respond to data broadcast
    pub worker_discovery_timeout_secs: u64,
}

impl DistributedMode {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            min_workers: 1,
            broker_url: None,
            worker_discovery_timeout_secs: 30,
        }
    }
    
    pub fn disabled() -> Self {
        Self::default()
    }
    
    /// Create from environment variables
    pub fn from_env() -> Self {
        let enabled = std::env::var("DISTRIBUTED_GA_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);
        
        let min_workers = std::env::var("DISTRIBUTED_GA_MIN_WORKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        
        let broker_url = std::env::var("MESSAGE_BROKER_URL").ok();
        
        let worker_discovery_timeout_secs = std::env::var("DISTRIBUTED_GA_WORKER_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        
        Self {
            enabled,
            min_workers,
            broker_url,
            worker_discovery_timeout_secs,
        }
    }
}

/// Helper to run GA with automatic distributed/local selection
pub struct DistributedGARunner;

impl DistributedGARunner {
    /// Run genetic optimization with automatic mode selection
    /// 
    /// If distributed mode is enabled and enough workers are available,
    /// uses distributed GA. Otherwise falls back to local GA.
    pub async fn run_with_fallback<F>(
        mode: &DistributedMode,
        config: &DistributedGAConfig,
        market_data: &[dataloader::MarketData],
        local_fitness_fn: F,
    ) -> (AvellanedaStoikovChromosome, f64)
    where
        F: Fn(&AvellanedaStoikovChromosome) -> std::pin::Pin<Box<dyn std::future::Future<Output = genetic::FitnessResult> + Send>> + Send + Sync + 'static,
    {
        if !mode.enabled {
            log_info_structured!(MAIN_LOGGER, "GA_MODE_LOCAL",
                "event" => "GA_MODE_LOCAL",
                "reason" => "distributed_disabled"
            );
            return Self::run_local(config, local_fitness_fn).await;
        }
        
        eprintln!("🌐 DISTRIBUTED: Mode enabled, attempting broker connection...");
        
        // Try to connect to broker and discover workers
        let broker_url = mode.broker_url.clone()
            .or_else(|| std::env::var("MESSAGE_BROKER_URL").ok())
            .unwrap_or_else(|| "http://messagebrokerengine.default.svc.cluster.local:8080".to_string());
        
        eprintln!("🌐 DISTRIBUTED: broker_url = {}", broker_url);
        
        log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_ATTEMPTING",
            "event" => "DISTRIBUTED_GA_ATTEMPTING",
            "broker_url" => &broker_url,
            "min_workers" => mode.min_workers
        );
        
        // Try to create broker client and coordinator
        // Parse the broker URL to get host and port
        let (host, port) = if let Ok(url) = url::Url::parse(&broker_url) {
            let host = url.host_str().unwrap_or("localhost").to_string();
            let port = url.port().unwrap_or(8080);
            (host, port)
        } else {
            // Fallback parsing for tcp:// URLs
            let parts: Vec<&str> = broker_url.trim_start_matches("tcp://")
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .split(':')
                .collect();
            let host = parts.get(0).unwrap_or(&"localhost").to_string();
            let port: u16 = parts.get(1)
                .and_then(|p| p.split('/').next())
                .and_then(|p| p.parse().ok())
                .unwrap_or(8081);
            (host, port)
        };
        
        eprintln!("🌐 DISTRIBUTED: Parsed host={} port={}", host, port);
        
        let broker = Arc::new(BacktestBrokerClient::network(&host, port));
        
        eprintln!("🌐 DISTRIBUTED: Connecting to broker...");
        // Connect to broker and start receiver for results
        if let Err(e) = broker.connect().await {
            eprintln!("🌐 DISTRIBUTED: Failed to connect: {}. Falling back to local.", e);
            log_warn!(MAIN_LOGGER, "Failed to connect to broker: {}. Falling back to local GA.", e);
            return Self::run_local(config, local_fitness_fn).await;
        }
        eprintln!("🌐 DISTRIBUTED: Connected to broker!");
        
        eprintln!("🌐 DISTRIBUTED: Starting receiver for coordinator role...");
        // Start receiver to get chromosome results from workers
        use super::broker_client::BrokerRole;
        if let Err(e) = broker.start_receiver(BrokerRole::Coordinator).await {
            eprintln!("🌐 DISTRIBUTED: Failed to start receiver: {}. Falling back to local.", e);
            log_warn!(MAIN_LOGGER, "Failed to start broker receiver: {}. Falling back to local GA.", e);
            return Self::run_local(config, local_fitness_fn).await;
        }
        eprintln!("🌐 DISTRIBUTED: Receiver started successfully!");
        
        let job_id = format!("distributed-{}", uuid::Uuid::new_v4());
        eprintln!("🌐 DISTRIBUTED: Creating coordinator for job_id={}", job_id);
        let mut coordinator = DistributedGACoordinator::new(
            Arc::clone(&broker),
            config.clone(),
            job_id,
        );
        
        // Broadcast data and wait for workers
        eprintln!("🌐 DISTRIBUTED: Broadcasting {} data points to workers...", market_data.len());
        match coordinator.broadcast_data(market_data).await {
            Ok(cache_key) => {
                eprintln!("🌐 DISTRIBUTED: Data broadcast complete, cache_key={}", cache_key);
                let worker_count = coordinator.worker_count().await;
                eprintln!("🌐 DISTRIBUTED: worker_count={}, min_workers={}", worker_count, mode.min_workers);
                if worker_count >= mode.min_workers {
                    log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_STARTING",
                        "event" => "DISTRIBUTED_GA_STARTING",
                        "worker_count" => worker_count,
                        "min_workers" => mode.min_workers,
                        "cache_key" => &cache_key
                    );
                    
                    eprintln!("🌐 DISTRIBUTED: Running distributed GA...");
                    // Run distributed GA
                    match coordinator.run().await {
                        Ok((best, fitness)) => {
                            eprintln!("🌐 DISTRIBUTED: GA completed! fitness={}", fitness);
                            return (best, fitness);
                        }
                        Err(e) => {
                            eprintln!("🌐 DISTRIBUTED: GA failed: {}. Falling back to local.", e);
                            log_warn!(MAIN_LOGGER, "Distributed GA failed: {}. Falling back to local.", e);
                        }
                    }
                } else {
                    eprintln!("🌐 DISTRIBUTED: Not enough workers ({}), need {}. Falling back to local.", worker_count, mode.min_workers);
                    log_warn!(MAIN_LOGGER,
                        "Only {} workers available (need {}). Falling back to local GA.",
                        worker_count, mode.min_workers
                    );
                }
            }
            Err(e) => {
                eprintln!("🌐 DISTRIBUTED: Data broadcast failed: {}. Falling back to local.", e);
                log_warn!(MAIN_LOGGER, "Data broadcast failed: {}. Falling back to local GA.", e);
            }
        }
        
        // Fallback to local GA
        log_info_structured!(MAIN_LOGGER, "GA_MODE_LOCAL_FALLBACK",
            "event" => "GA_MODE_LOCAL",
            "reason" => "distributed_fallback"
        );
        Self::run_local(config, local_fitness_fn).await
    }
    
    /// Run local GA optimization
    async fn run_local<F>(
        config: &DistributedGAConfig,
        fitness_fn: F,
    ) -> (AvellanedaStoikovChromosome, f64)
    where
        F: Fn(&AvellanedaStoikovChromosome) -> std::pin::Pin<Box<dyn std::future::Future<Output = genetic::FitnessResult> + Send>> + Send + Sync + 'static,
    {
        use genetic::GeneticOptimizer;
        
        let optimizer = GeneticOptimizer {
            config: config.genetic_config.clone(),
            fitness_fn: Arc::new(fitness_fn),
            progress_callback: None,
            job_id: Some(format!("local-{}", uuid::Uuid::new_v4())),
        };
        
        optimizer.run().await
    }
}
