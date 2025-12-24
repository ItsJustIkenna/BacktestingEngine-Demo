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
use std::path::PathBuf;
use std::io::{Read, Write};
use tokio::sync::RwLock;
use rand::SeedableRng;
use serde::{Serialize, Deserialize};

use config::GeneticConfig;
use genetic::{Chromosome, FitnessResult};
use genetic::chromosome::AvellanedaStoikovChromosome;

use super::broker_client::BacktestBrokerClient;
use crate::{log_info_structured, log_warn, log_debug, log_error};
use crate::logging_facade::MAIN_LOGGER;

/// Checkpoint for resuming GA optimization after restart
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GACheckpoint {
    /// Job ID this checkpoint belongs to
    pub job_id: String,
    /// Current generation (0-indexed, next generation to run)
    pub generation: usize,
    /// Current population
    pub population: Vec<AvellanedaStoikovChromosome>,
    /// Best chromosome found so far
    pub best: AvellanedaStoikovChromosome,
    /// Best fitness score
    pub best_fitness: f64,
    /// Generations without improvement counter
    pub generations_without_improvement: usize,
    /// Data cache key
    pub data_cache_key: String,
    /// RNG seed for reproducibility
    pub rng_seed: u64,
    /// Timestamp when checkpoint was created
    pub created_at: i64,
}

impl GACheckpoint {
    /// Get the checkpoint directory path
    fn checkpoint_dir() -> PathBuf {
        let data_dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        PathBuf::from(data_dir).join("checkpoints")
    }
    
    /// Get the checkpoint file path for a job
    fn checkpoint_path(job_id: &str) -> PathBuf {
        Self::checkpoint_dir().join(format!("{}.checkpoint.json", job_id))
    }
    
    /// Save checkpoint to disk
    pub fn save(&self) -> Result<(), std::io::Error> {
        let dir = Self::checkpoint_dir();
        std::fs::create_dir_all(&dir)?;
        
        let path = Self::checkpoint_path(&self.job_id);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        
        // Write to temp file first, then rename for atomic operation
        let temp_path = path.with_extension("tmp");
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp_path, &path)?;
        
        eprintln!("💾 [CHECKPOINT] Saved checkpoint for job {} at generation {}", 
            self.job_id, self.generation);
        
        Ok(())
    }
    
    /// Load checkpoint from disk if it exists
    pub fn load(job_id: &str) -> Option<Self> {
        let path = Self::checkpoint_path(job_id);
        
        if !path.exists() {
            eprintln!("📂 [CHECKPOINT] No checkpoint found for job {}", job_id);
            return None;
        }
        
        match std::fs::File::open(&path) {
            Ok(mut file) => {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_err() {
                    eprintln!("❌ [CHECKPOINT] Failed to read checkpoint file");
                    return None;
                }
                
                match serde_json::from_str::<GACheckpoint>(&contents) {
                    Ok(checkpoint) => {
                        eprintln!("✅ [CHECKPOINT] Loaded checkpoint for job {} at generation {}", 
                            checkpoint.job_id, checkpoint.generation);
                        Some(checkpoint)
                    }
                    Err(e) => {
                        eprintln!("❌ [CHECKPOINT] Failed to parse checkpoint: {:?}", e);
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ [CHECKPOINT] Failed to open checkpoint file: {:?}", e);
                None
            }
        }
    }
    
    /// Delete checkpoint file after successful completion
    pub fn delete(job_id: &str) {
        let path = Self::checkpoint_path(job_id);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!("⚠️ [CHECKPOINT] Failed to delete checkpoint: {:?}", e);
            } else {
                eprintln!("🗑️ [CHECKPOINT] Deleted checkpoint for job {}", job_id);
            }
        }
    }
}

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
    /// Start time for data query (ISO 8601 format)
    pub start_time: String,
    /// End time for data query (ISO 8601 format)
    pub end_time: String,
    /// Train data ratio (default 0.6 = 60%) - GA optimization runs here
    pub train_ratio: f64,
    /// Validation data ratio (default 0.4 = 40%) - Walk-forward runs here as PRIMARY validation
    /// Note: The old test_ratio and validate_ratio are combined - walk-forward IS the final validation
    pub validation_ratio: f64,
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
    /// Train data end index (GA uses [0, train_end_idx))
    train_end_idx: i64,
    /// Test data end index (walk-forward uses [train_end_idx, test_end_idx))
    test_end_idx: i64,
    /// Total tick count in the dataset
    total_ticks: i64,
    /// Progress callback for reporting granular progress
    progress_callback: Option<Arc<dyn Fn(f64, String) + Send + Sync>>,
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
            train_end_idx: 0,
            test_end_idx: 0,
            total_ticks: 0,
            progress_callback: None,
        }
    }
    
    /// Set progress callback for granular progress updates
    /// Callback receives (progress_pct: f64, phase_detail: String)
    pub fn set_progress_callback<F>(&mut self, callback: F)
    where
        F: Fn(f64, String) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
    }
    
    /// Report progress to callback if set
    fn report_progress(&self, progress: f64, detail: String) {
        // Log to Kibana for visibility
        log_info_structured!(MAIN_LOGGER, "GA_PROGRESS_UPDATE",
            "event" => "GA_PROGRESS_UPDATE",
            "job_id" => &self.job_id,
            "progress_pct" => format!("{:.1}", progress),
            "detail" => &detail
        );
        
        if let Some(cb) = &self.progress_callback {
            cb(progress, detail);
        }
    }
    
    /// Set data split indices (called after data is loaded)
    /// 
    /// Uses simplified TRAIN/VALIDATE split:
    /// - TRAIN: [0, train_end_idx) - GA optimization uses this
    /// - VALIDATE: [train_end_idx, total_ticks) - Walk-forward validation uses this as PRIMARY output
    pub fn set_data_splits(&mut self, total_ticks: i64) {
        let train_ratio = self.config.train_ratio;
        
        self.total_ticks = total_ticks;
        self.train_end_idx = (total_ticks as f64 * train_ratio) as i64;
        // Validation runs from train_end to end of data
        self.test_end_idx = total_ticks; // Walk-forward uses ALL validation data
        
        log_info_structured!(MAIN_LOGGER, "GA_DATA_SPLIT_CONFIG",
            "event" => "GA_DATA_SPLIT_CONFIG",
            "job_id" => &self.job_id,
            "total_ticks" => total_ticks,
            "train_end_idx" => self.train_end_idx,
            "validation_start" => self.train_end_idx,
            "validation_end" => total_ticks,
            "train_ratio" => train_ratio,
            "validation_ratio" => 1.0 - train_ratio
        );
    }
    
    /// Probe binary file to get tick count for data splits
    pub fn probe_tick_count(&self) -> Option<i64> {
        let data_dir = std::env::var("DATA_DIR")
            .unwrap_or_else(|_| "/data/historical".to_string());
        let filename = format!("{}_{}.bin", 
            self.config.symbol.replace('/', "_"), 
            self.config.exchange);
        let path = std::path::PathBuf::from(&data_dir).join(&filename);
        
        eprintln!("🔍 [COORDINATOR] probe_tick_count: data_dir={}, filename={}, full_path={:?}", 
            data_dir, filename, path);
        
        if path.exists() {
            eprintln!("✅ [COORDINATOR] Binary file exists at {:?}", path);
            match data_prep::BinaryFileReader::open(&path) {
                Ok(reader) => {
                    let count = reader.tick_count() as i64;
                    eprintln!("✅ [COORDINATOR] Binary file opened, tick_count={}", count);
                    Some(count)
                },
                Err(e) => {
                    eprintln!("❌ [COORDINATOR] Failed to open binary file: {:?}", e);
                    None
                },
            }
        } else {
            eprintln!("❌ [COORDINATOR] Binary file NOT found at {:?}", path);
            // List what files ARE in the directory
            if let Ok(entries) = std::fs::read_dir(&data_dir) {
                eprintln!("📂 [COORDINATOR] Files in {}: {:?}", data_dir, 
                    entries.filter_map(|e| e.ok()).map(|e| e.file_name()).collect::<Vec<_>>());
            }
            None
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
    
    /// Request workers to load market data from database
    /// 
    /// This sends a small message (~200 bytes) with query parameters.
    /// Workers will load data directly from the database, avoiding the need
    /// to serialize/compress/transfer 500MB+ over the message broker.
    /// 
    /// Returns cache key and number of workers that acknowledged.
    pub async fn request_data_load(&mut self) -> Result<String, CoordinatorError> {
        let cache_key = format!(
            "{}:{}:{}",
            self.config.symbol,
            self.config.exchange,
            chrono::Utc::now().timestamp()
        );
        
        log_info_structured!(MAIN_LOGGER, "GA_DATA_LOAD_REQUEST",
            "event" => "GA_DATA_LOAD_REQUEST",
            "job_id" => &self.job_id,
            "cache_key" => &cache_key,
            "symbol" => &self.config.symbol,
            "exchange" => &self.config.exchange,
            "start_time" => &self.config.start_time,
            "end_time" => &self.config.end_time
        );
        
        // Create lightweight load request with query parameters only
        let request = protocol::broker::messages::DataLoadRequest {
            cache_key: cache_key.clone(),
            symbols: vec![self.config.symbol.clone()],
            exchange: self.config.exchange.clone(),
            start_time: self.config.start_time.clone(),
            end_time: self.config.end_time.clone(),
            ttl_seconds: self.config.cache_ttl_seconds,
            initial_capital: self.config.initial_capital,
            job_id: self.job_id.clone(),
        };
        
        self.data_cache_key = Some(cache_key.clone());
        
        // Subscribe to worker acknowledgments BEFORE publishing
        let mut ack_receiver = self.broker.subscribe_data_acks();
        let workers = Arc::clone(&self.workers);
        let cache_key_clone = cache_key.clone();
        
        // Wait for workers to acknowledge (with longer timeout since they're loading from DB)
        // Workers load 7M+ records which takes ~40 seconds
        let ack_timeout = Duration::from_secs(120); // Give workers time to query DB
        let ack_deadline = Instant::now() + ack_timeout;
        
        // Retry publishing every 5 seconds until we get worker acknowledgments
        // This handles the race condition where workers haven't subscribed yet
        let mut last_publish = Instant::now() - Duration::from_secs(10); // Force immediate first publish
        let publish_interval = Duration::from_secs(5);
        let min_workers_needed = 2_usize; // Default minimum workers required
        
        log_info_structured!(MAIN_LOGGER, "GA_WAITING_FOR_WORKERS",
            "event" => "GA_WAITING_FOR_WORKERS",
            "timeout_seconds" => 120,
            "min_workers" => min_workers_needed
        );
        
        // Track tick_count from worker acks - use first non-zero value received
        let mut received_tick_count: i64 = 0;
        
        while Instant::now() < ack_deadline {
            // Republish periodically to catch workers that subscribed late
            if last_publish.elapsed() >= publish_interval {
                eprintln!("📤 [COORDINATOR] Publishing data load request (attempt)...");
                if let Err(e) = self.broker.publish_data_load_request(request.clone()).await {
                    log_error!(MAIN_LOGGER, "Failed to publish data load request: {}", e);
                } else {
                    eprintln!("✅ [COORDINATOR] Data load request published");
                }
                last_publish = Instant::now();
            }
            
            match tokio::time::timeout(
                Duration::from_millis(500),
                ack_receiver.recv()
            ).await {
                Ok(Ok(ack)) => {
                    if ack.cache_key == cache_key_clone && ack.success {
                        let mut w = workers.write().await;
                        w.insert(ack.worker_id.clone(), WorkerInfo {
                            worker_id: ack.worker_id.clone(),
                            capacity: ack.capacity,
                            last_seen: Instant::now(),
                        });
                        
                        // Capture tick_count from first worker that provides it
                        if received_tick_count == 0 && ack.tick_count > 0 {
                            received_tick_count = ack.tick_count;
                            eprintln!("📊 [COORDINATOR] Received tick_count={} from worker {}", ack.tick_count, ack.worker_id);
                        }
                        
                        eprintln!("✅ [COORDINATOR] Worker {} acknowledged (capacity={}, tick_count={})", 
                            ack.worker_id, ack.capacity, ack.tick_count);
                        log_info_structured!(MAIN_LOGGER, "GA_WORKER_READY",
                            "event" => "GA_WORKER_READY",
                            "worker_id" => ack.worker_id,
                            "capacity" => ack.capacity,
                            "tick_count" => ack.tick_count
                        );
                        
                        // Check if we have enough workers now
                        if w.len() >= min_workers_needed {
                            eprintln!("✅ [COORDINATOR] Got {} workers, enough to proceed", w.len());
                            break;
                        }
                    }
                }
                Ok(Err(_)) => break, // Channel closed
                Err(_) => continue, // Timeout, keep waiting
            }
        }
        
        // Set data splits from tick_count received from workers
        if received_tick_count > 0 {
            self.set_data_splits(received_tick_count);
            eprintln!("📊 [COORDINATOR] Data splits set from worker ack: {} ticks", received_tick_count);
        } else {
            eprintln!("⚠️ [COORDINATOR] No tick_count received from workers, trying local probe...");
            // Fallback to local probe (may fail if coordinator doesn't have data access)
            if let Some(tick_count) = self.probe_tick_count() {
                self.set_data_splits(tick_count);
                eprintln!("📊 [COORDINATOR] Data splits set from local probe: {} ticks", tick_count);
            } else {
                eprintln!("⚠️ [COORDINATOR] Could not determine tick_count - data splits will be 0");
            }
        }
        
        let worker_count = self.workers.read().await.len();
        log_info_structured!(MAIN_LOGGER, "GA_DATA_LOAD_COMPLETE",
            "event" => "GA_DATA_LOAD_COMPLETE",
            "cache_key" => &cache_key,
            "workers_ready" => worker_count,
            "total_capacity" => self.total_capacity().await,
            "tick_count" => received_tick_count
        );
        
        if worker_count == 0 {
            log_warn!(MAIN_LOGGER, "No workers acknowledged data load request - falling back to local evaluation");
        }
        
        Ok(cache_key)
    }
    
    /// Run distributed genetic optimization
    /// 
    /// This runs the GA algorithm, distributing chromosome evaluations
    /// across all available workers. Supports checkpointing for restart recovery.
    pub async fn run(&self) -> Result<(AvellanedaStoikovChromosome, f64), CoordinatorError> {
        let cache_key = self.data_cache_key.as_ref()
            .ok_or(CoordinatorError::DataNotBroadcast)?;
        
        let config = &self.config.genetic_config;
        let job_tag = format!("[Job {}] ", self.job_id);
        let optimization_start = Instant::now();
        
        // Try to load checkpoint for resume
        let checkpoint = GACheckpoint::load(&self.job_id);
        let start_generation: usize;
        let mut rng: rand::rngs::StdRng;
        let mut population: Vec<AvellanedaStoikovChromosome>;
        let mut best: AvellanedaStoikovChromosome;
        let mut best_fitness: f64;
        let mut generations_without_improvement: usize;
        
        if let Some(cp) = checkpoint {
            // Resume from checkpoint
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_RESUME",
                "event" => "DISTRIBUTED_GA_RESUME",
                "job_id" => &self.job_id,
                "resume_generation" => cp.generation,
                "best_fitness" => cp.best_fitness
            );
            eprintln!("🔄 [GA] RESUMING from checkpoint at generation {}/{}", 
                cp.generation, config.generations);
            
            start_generation = cp.generation;
            rng = rand::rngs::StdRng::seed_from_u64(cp.rng_seed);
            population = cp.population;
            best = cp.best;
            best_fitness = cp.best_fitness;
            generations_without_improvement = cp.generations_without_improvement;
        } else {
            // Fresh start
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_START",
                "event" => "DISTRIBUTED_GA_START",
                "job_id" => &self.job_id,
                "population" => config.population_size,
                "generations" => config.generations
            );
            
            start_generation = 0;
            rng = rand::rngs::StdRng::from_entropy();
            
            // Initialize population
            population = (0..config.population_size)
                .map(|_| AvellanedaStoikovChromosome::random(&mut rng))
                .collect();
            
            best = population[0].clone();
            best_fitness = 0.0;
            generations_without_improvement = 0;
        }
        
        let mut _best_equity_curve = Vec::<f64>::new();
        const CONVERGENCE_THRESHOLD: usize = 5;
        
        let elite_size = (config.population_size as f64 * 0.05).ceil() as usize;
        
        // Subscribe to results channel
        let mut result_receiver = self.broker.subscribe_chromosome_results();
        
        for generation in start_generation..config.generations {
            let generation_start = Instant::now();
            
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_GEN_START",
                "event" => "DISTRIBUTED_GA_GEN_START",
                "job_id" => &self.job_id,
                "generation" => generation + 1,
                "population_size" => population.len()
            );
            
            // Report generation start progress
            // Progress range: 5% to 85% for genetic phase
            // Each generation gets an equal slice of this range
            let gen_progress_start = 5.0 + (generation as f64 / config.generations as f64) * 80.0;
            self.report_progress(
                gen_progress_start,
                format!("Generation {}/{}: evaluating chromosomes", generation + 1, config.generations)
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
            let pop_size = population.len();
            let gen_progress_range = 80.0 / config.generations as f64; // Progress range for this generation
            
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
                            
                            // Granular progress update for each chromosome result
                            let chromo_progress = gen_progress_start + 
                                (results.len() as f64 / pop_size as f64) * gen_progress_range;
                            self.report_progress(
                                chromo_progress,
                                format!("Generation {}/{}: {}/{} evaluated", 
                                    generation + 1, config.generations, results.len(), pop_size)
                            );
                            
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
            
            let fitnesses: Vec<f64> = fitness_results.iter().map(|r| r.fitness).collect();
            
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
                    _best_equity_curve = indexed[i].3.equity_curve.clone();
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
                // Delete checkpoint on convergence
                GACheckpoint::delete(&self.job_id);
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
            
            population = new_population.clone();
            
            // Save checkpoint after each generation
            let checkpoint = GACheckpoint {
                job_id: self.job_id.clone(),
                generation: generation + 1, // Next generation to run
                population: new_population,
                best: best.clone(),
                best_fitness,
                generations_without_improvement,
                data_cache_key: cache_key.clone(),
                rng_seed: rand::Rng::gen(&mut rng), // Save a seed for RNG state
                created_at: chrono::Utc::now().timestamp(),
            };
            
            if let Err(e) = checkpoint.save() {
                log_warn!(MAIN_LOGGER, "Failed to save checkpoint: {:?}", e);
            }
            
            let gen_elapsed = generation_start.elapsed().as_secs_f64();
            
            // Report generation completion with best fitness
            let gen_complete_progress = 5.0 + ((generation + 1) as f64 / config.generations as f64) * 80.0;
            self.report_progress(
                gen_complete_progress,
                format!("Generation {}/{} complete, best_fitness={:.4}", 
                    generation + 1, config.generations, best_fitness)
            );
            
            log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_GA_GEN_COMPLETE",
                "event" => "DISTRIBUTED_GA_GEN_COMPLETE",
                "generation" => generation + 1,
                "elapsed_secs" => gen_elapsed,
                "best_fitness" => best_fitness,
                "chromosomes_per_sec" => self.config.genetic_config.population_size as f64 / gen_elapsed
            );
        }
        
        // Delete checkpoint on successful completion
        GACheckpoint::delete(&self.job_id);
        
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
            is_final_evaluation: false,
            run_monte_carlo: false,
            run_walk_forward: false,
            // Pass data split indices for train/test/validate separation
            train_end_idx: self.train_end_idx,
            test_end_idx: self.test_end_idx,
            total_ticks: self.total_ticks,
        })
    }
    
    /// Send a final evaluation request for the best chromosome
    /// 
    /// This runs a full backtest with Monte Carlo and Walk-Forward analysis
    /// to get comprehensive metrics for the final result.
    pub async fn run_final_evaluation(
        &mut self,
        chromosome: &AvellanedaStoikovChromosome,
        run_monte_carlo: bool,
        run_walk_forward: bool,
    ) -> Result<protocol::broker::messages::ChromosomeEvalResult, CoordinatorError> {
        let cache_key = self.data_cache_key.clone()
            .ok_or(CoordinatorError::DataNotBroadcast)?;
        
        let params = serde_json::to_vec(chromosome)
            .map_err(|e| CoordinatorError::SerializationError(e.to_string()))?;
        
        let request = protocol::broker::messages::ChromosomeEvalRequest {
            job_id: self.job_id.clone(),
            generation: -1, // Indicates final evaluation
            chromosome_id: 0,
            total_chromosomes: 1,
            chromosome_params: params,
            symbol: self.config.symbol.clone(),
            exchange: self.config.exchange.clone(),
            initial_capital: self.config.initial_capital,
            data_cache_key: cache_key,
            is_final_evaluation: true,
            run_monte_carlo,
            run_walk_forward,
            // Pass data split indices for train/test/validate separation
            train_end_idx: self.train_end_idx,
            test_end_idx: self.test_end_idx,
            total_ticks: self.total_ticks,
        };
        
        log_info_structured!(MAIN_LOGGER, "GA_FINAL_EVAL_REQUEST",
            "event" => "GA_FINAL_EVAL_REQUEST",
            "job_id" => &self.job_id,
            "run_monte_carlo" => run_monte_carlo,
            "run_walk_forward" => run_walk_forward
        );
        
        // Subscribe to results channel
        let mut result_receiver = self.broker.subscribe_chromosome_results();
        
        println!("📤 [COORDINATOR] Publishing final evaluation request for job {}", self.job_id);
        
        // Publish the request
        self.broker.publish_chromosome_request(request).await
            .map_err(|e| CoordinatorError::BrokerError(e.to_string()))?;
        
        println!("📤 [COORDINATOR] Final evaluation request published, waiting for response...");
        
        // Wait for result with timeout
        let timeout = Duration::from_secs(EVAL_TIMEOUT_SECS);
        let start = Instant::now();
        let mut results_seen = 0u32;
        
        loop {
            match tokio::time::timeout(
                Duration::from_secs(30),
                result_receiver.recv()
            ).await {
                Ok(Ok(result)) => {
                    results_seen += 1;
                    println!("📥 [COORDINATOR] Received result #{}: job={}, is_final={}, worker={}, trades={}",
                        results_seen, result.job_id, result.is_final_evaluation, result.worker_id, result.num_trades);
                    
                    if result.job_id == self.job_id && result.is_final_evaluation {
                        println!("✅ [COORDINATOR] This is our FINAL result! PnL={:.2}", result.net_pnl);
                        log_info_structured!(MAIN_LOGGER, "GA_FINAL_EVAL_COMPLETE",
                            "event" => "GA_FINAL_EVAL_COMPLETE",
                            "job_id" => &self.job_id,
                            "net_pnl" => result.net_pnl,
                            "sharpe_ratio" => result.sharpe_ratio,
                            "num_trades" => result.num_trades
                        );
                        return Ok(result);
                    } else {
                        println!("⏭️ [COORDINATOR] Ignoring result (wrong job or not final)");
                    }
                }
                Ok(Err(_)) => {
                    println!("❌ [COORDINATOR] Channel closed while waiting for final evaluation");
                    return Err(CoordinatorError::BrokerError(
                        "Channel closed while waiting for final evaluation".to_string()
                    ));
                }
                Err(_) => {
                    // Timeout on this recv, check overall timeout
                    println!("⏳ [COORDINATOR] No result in 30s, elapsed={:.1}s/{:.1}s, seen {} results",
                        start.elapsed().as_secs_f64(), timeout.as_secs_f64(), results_seen);
                    if start.elapsed() > timeout {
                        println!("❌ [COORDINATOR] TIMEOUT waiting for final evaluation after {:.1}s",
                            start.elapsed().as_secs_f64());
                        return Err(CoordinatorError::Timeout(
                            "Timeout waiting for final evaluation result".to_string()
                        ));
                    }
                    continue;
                }
            }
        }
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
        _market_data: &[dataloader::MarketData],
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
        
        // Request workers to load data from database (sends ~200 bytes instead of 500MB+)
        eprintln!("🌐 DISTRIBUTED: Requesting workers to load data from database...");
        eprintln!("🌐 DISTRIBUTED: Query params - symbol={}, exchange={}, start={}, end={}",
            config.symbol, config.exchange, config.start_time, config.end_time);
        match coordinator.request_data_load().await {
            Ok(cache_key) => {
                eprintln!("🌐 DISTRIBUTED: Data load request sent, cache_key={}", cache_key);
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
                eprintln!("🌐 DISTRIBUTED: Data load request failed: {}. Falling back to local.", e);
                log_warn!(MAIN_LOGGER, "Data load request failed: {}. Falling back to local GA.", e);
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
