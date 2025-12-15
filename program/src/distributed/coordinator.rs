//! Distributed Coordinator for orchestrating backtest jobs
//!
//! The coordinator:
//! 1. Receives a backtest job
//! 2. Calculates optimal chunks based on data size
//! 3. Publishes chunks to the message broker
//! 4. Collects results from workers
//! 5. Aggregates final result

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::timeout;
use chrono::{DateTime, Utc};

use protocol::broker::messages::{
    BacktestChunk, BacktestChunkResult, BacktestProgress, BacktestAggregatedResult
};

use super::broker_client::{BacktestBrokerClient, get_broker_client, BrokerError};
use super::chunking::{ChunkCalculator, ChunkingStrategy, BacktestChunkInfo};
use super::aggregator::ResultAggregator;
use crate::job_queue::{BacktestJob, BacktestJobParams};
use crate::{log_debug, log_info_structured, log_error_structured, log_warn_structured};
use crate::logging_facade::BACKTEST_LOGGER;

/// Timeout for waiting for all results
const RESULT_TIMEOUT_SECS: u64 = 3600; // 1 hour max

/// Progress update interval
const PROGRESS_INTERVAL_SECS: u64 = 5;

/// Coordinator for distributed backtesting
pub struct DistributedCoordinator {
    broker: Arc<BacktestBrokerClient>,
    chunk_calculator: ChunkCalculator,
}

impl DistributedCoordinator {
    /// Create a new coordinator
    pub fn new(_db_url: impl Into<String>) -> Self {
        Self {
            broker: get_broker_client(),
            chunk_calculator: ChunkCalculator::adaptive(),
        }
    }
    
    /// Create with custom broker
    pub fn with_broker(_db_url: impl Into<String>, broker: Arc<BacktestBrokerClient>) -> Self {
        Self {
            broker,
            chunk_calculator: ChunkCalculator::adaptive(),
        }
    }
    
    /// Set custom chunking strategy
    pub fn with_chunking_strategy(mut self, strategy: ChunkingStrategy) -> Self {
        self.chunk_calculator = ChunkCalculator::new(strategy);
        self
    }
    
    /// Run a distributed backtest
    /// 
    /// This is the main entry point for distributed processing.
    /// It handles the full lifecycle: chunking, distribution, collection, aggregation.
    pub async fn run_distributed_backtest(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestAggregatedResult, CoordinatorError> {
        let job_id = &job.id;
        let start_time = Instant::now();
        
        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_BACKTEST_START",
            "event" => "DISTRIBUTED_BACKTEST_START",
            "job_id" => job_id
        );
        
        // 1. Parse dates and get event count
        let (start_dt, end_dt) = self.parse_job_dates(&job.params)?;
        let event_count = self.get_event_count(&job.params, &start_dt, &end_dt).await?;
        
        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_JOB_EVENTS",
            "event" => "DISTRIBUTED_JOB_EVENTS",
            "job_id" => job_id,
            "event_count" => event_count,
            "start_time" => start_dt.to_string(),
            "end_time" => end_dt.to_string()
        );
        
        // 2. Calculate chunks
        let chunks = self.chunk_calculator.calculate_chunks(
            start_dt, 
            end_dt, 
            Some(event_count)
        );
        
        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_JOB_CHUNKS",
            "event" => "DISTRIBUTED_JOB_CHUNKS",
            "job_id" => job_id,
            "chunk_count" => chunks.len()
        );
        
        // 3. Create aggregator
        let mut aggregator = ResultAggregator::new(job_id.clone(), chunks.len() as i32);
        
        // 4. Subscribe to results before publishing chunks
        let mut result_rx = self.broker.subscribe_all_results();
        
        // 5. Publish all chunks
        let strategy_config = serde_json::to_vec(&job.params)
            .map_err(|e| CoordinatorError::SerializationError(e.to_string()))?;
        
        for chunk_info in &chunks {
            let chunk = self.create_chunk(job, chunk_info, &strategy_config);
            
            self.broker.publish_chunk(chunk).await
                .map_err(|e| CoordinatorError::BrokerError(e))?;
        }
        
        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_CHUNKS_PUBLISHED",
            "event" => "DISTRIBUTED_CHUNKS_PUBLISHED",
            "chunk_count" => chunks.len(),
            "job_id" => job_id
        );
        
        // 6. Collect results with timeout
        let collection_result = self.collect_results(
            job_id,
            &mut aggregator,
            &mut result_rx,
            chunks.len(),
        ).await;
        
        match collection_result {
            Ok(()) => {
                log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_ALL_CHUNKS_RECEIVED",
                    "event" => "DISTRIBUTED_ALL_CHUNKS_RECEIVED",
                    "chunk_count" => chunks.len(),
                    "job_id" => job_id
                );
            }
            Err(e) => {
                log_error_structured!(BACKTEST_LOGGER, "DISTRIBUTED_COLLECTION_FAILED",
                    "event" => "DISTRIBUTED_COLLECTION_FAILED",
                    "job_id" => job_id,
                    "error" => e.to_string()
                );
                // Continue with partial results
            }
        }
        
        // 7. Aggregate results
        let aggregated = aggregator.aggregate()
            .map_err(|e| CoordinatorError::AggregationError(e.to_string()))?;
        
        let total_duration = start_time.elapsed();
        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_BACKTEST_COMPLETE",
            "event" => "DISTRIBUTED_BACKTEST_COMPLETE",
            "job_id" => job_id,
            "total_trades" => aggregated.total_trades,
            "total_pnl" => format!("{:.2}", aggregated.total_net_pnl),
            "elapsed_secs" => format!("{:.1}", total_duration.as_secs_f64())
        );
        
        // 8. Cleanup
        self.broker.cleanup_job(job_id).await;
        
        Ok(aggregated)
    }
    
    /// Parse job dates from params
    fn parse_job_dates(&self, params: &BacktestJobParams) -> Result<(DateTime<Utc>, DateTime<Utc>), CoordinatorError> {
        let start = params.start_date.as_ref()
            .ok_or_else(|| CoordinatorError::MissingParameter("start_date".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|e| CoordinatorError::InvalidParameter(format!("start_date: {}", e)))?;
        
        let end = params.end_date.as_ref()
            .ok_or_else(|| CoordinatorError::MissingParameter("end_date".to_string()))?
            .parse::<DateTime<Utc>>()
            .map_err(|e| CoordinatorError::InvalidParameter(format!("end_date: {}", e)))?;
        
        Ok((start, end))
    }
    
    /// Get event count from database
    #[cfg(feature = "postgres")]
    async fn get_event_count(
        &self,
        params: &BacktestJobParams,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<i64, CoordinatorError> {
        // Use StreamingDataLoader to count events
        use crate::streaming::StreamingDataLoader;
        
        let db_url = std::env::var("DATABASE_URL")
            .map_err(|_| CoordinatorError::DatabaseError("DATABASE_URL not set".to_string()))?;
        let loader = StreamingDataLoader::new(&db_url)
            .await
            .map_err(|e| CoordinatorError::DatabaseError(e.to_string()))?;
        
        // Load events with a reasonable limit to estimate count
        // For full count, we'd need a dedicated count query
        let events = loader.load_events_with_limit(
            &params.symbol,
            &params.exchange,
            start,
            end,
            1_000_000, // Reasonable cap for counting
        ).await.map_err(|e| CoordinatorError::DatabaseError(e.to_string()))?;
        
        Ok(events.len() as i64)
    }
    
    /// Get event count - stub for non-postgres builds
    #[cfg(not(feature = "postgres"))]
    async fn get_event_count(
        &self,
        _params: &BacktestJobParams,
        _start: &DateTime<Utc>,
        _end: &DateTime<Utc>,
    ) -> Result<i64, CoordinatorError> {
        // Without postgres, we can't query the database
        // Return a default estimate
        Ok(200_000)
    }
    
    /// Create a BacktestChunk from ChunkInfo
    fn create_chunk(
        &self,
        job: &BacktestJob,
        info: &BacktestChunkInfo,
        strategy_config: &[u8],
    ) -> BacktestChunk {
        BacktestChunk {
            job_id: job.id.clone(),
            chunk_id: info.chunk_id,
            total_chunks: info.total_chunks,
            start_time: info.start_time.to_rfc3339(),
            end_time: info.end_time.to_rfc3339(),
            symbol: job.params.symbol.clone(),
            exchange: job.params.exchange.clone(),
            strategy_config: strategy_config.to_vec(),
            initial_capital: job.params.initial_capital,
            starting_position: 0.0, // First chunk starts fresh
            starting_cash: job.params.initial_capital,
            is_monte_carlo: false,
            monte_carlo_run: 0,
            priority: info.priority,
        }
    }
    
    /// Collect results from workers
    async fn collect_results(
        &self,
        job_id: &str,
        aggregator: &mut ResultAggregator,
        result_rx: &mut broadcast::Receiver<BacktestChunkResult>,
        total_chunks: usize,
    ) -> Result<(), CoordinatorError> {
        let deadline = Instant::now() + Duration::from_secs(RESULT_TIMEOUT_SECS);
        let mut last_progress = Instant::now();
        
        while !aggregator.is_complete() {
            // Check timeout
            if Instant::now() > deadline {
                return Err(CoordinatorError::Timeout {
                    received: aggregator.successful_chunks(),
                    expected: total_chunks,
                });
            }
            
            // Wait for result with short timeout for progress updates
            match timeout(Duration::from_secs(PROGRESS_INTERVAL_SECS), result_rx.recv()).await {
                Ok(Ok(result)) => {
                    // Only process results for this job
                    if result.job_id == job_id {
                        log_debug!(BACKTEST_LOGGER, "Received result for chunk {} of job {}", 
                            result.chunk_id, job_id);
                        aggregator.add_result(result);
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    log_warn_structured!(BACKTEST_LOGGER, "COORDINATOR_LAGGED",
                        "event" => "COORDINATOR_LAGGED",
                        "lagged_messages" => n
                    );
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    return Err(CoordinatorError::ChannelClosed);
                }
                Err(_) => {
                    // Timeout - publish progress update
                    if last_progress.elapsed() >= Duration::from_secs(PROGRESS_INTERVAL_SECS) {
                        let progress = BacktestProgress {
                            job_id: job_id.to_string(),
                            chunks_completed: aggregator.successful_chunks() as i32,
                            total_chunks: total_chunks as i32,
                            percent_complete: aggregator.completion_percentage(),
                            phase: "processing".to_string(),
                            estimated_remaining_secs: self.estimate_remaining(
                                aggregator.successful_chunks(),
                                total_chunks,
                                deadline,
                            ),
                            running_pnl: aggregator.running_pnl(),
                            running_trades: aggregator.running_trades(),
                        };
                        
                        let _ = self.broker.publish_progress(progress).await;
                        last_progress = Instant::now();
                        
                        log_info_structured!(BACKTEST_LOGGER, "DISTRIBUTED_JOB_PROGRESS",
                            "event" => "DISTRIBUTED_JOB_PROGRESS",
                            "job_id" => job_id,
                            "chunks_completed" => aggregator.successful_chunks(),
                            "total_chunks" => total_chunks,
                            "percent_complete" => format!("{:.1}", aggregator.completion_percentage()),
                            "running_pnl" => format!("{:.2}", aggregator.running_pnl())
                        );
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Estimate remaining time
    fn estimate_remaining(&self, completed: usize, total: usize, deadline: Instant) -> i64 {
        if completed == 0 {
            return RESULT_TIMEOUT_SECS as i64;
        }
        
        let remaining = total - completed;
        let time_per_chunk = deadline.duration_since(Instant::now()).as_secs() / completed as u64;
        (remaining as u64 * time_per_chunk) as i64
    }
}

/// Coordinator errors
#[derive(Debug)]
pub enum CoordinatorError {
    MissingParameter(String),
    InvalidParameter(String),
    DatabaseError(String),
    SerializationError(String),
    BrokerError(BrokerError),
    AggregationError(String),
    Timeout {
        received: usize,
        expected: usize,
    },
    ChannelClosed,
}

impl std::fmt::Display for CoordinatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoordinatorError::MissingParameter(p) => write!(f, "Missing parameter: {}", p),
            CoordinatorError::InvalidParameter(p) => write!(f, "Invalid parameter: {}", p),
            CoordinatorError::DatabaseError(e) => write!(f, "Database error: {}", e),
            CoordinatorError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            CoordinatorError::BrokerError(e) => write!(f, "Broker error: {}", e),
            CoordinatorError::AggregationError(e) => write!(f, "Aggregation error: {}", e),
            CoordinatorError::Timeout { received, expected } => {
                write!(f, "Timeout: received {} of {} chunks", received, expected)
            }
            CoordinatorError::ChannelClosed => write!(f, "Result channel closed"),
        }
    }
}

impl std::error::Error for CoordinatorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    
    #[test]
    fn test_coordinator_creation() {
        let _coordinator = DistributedCoordinator::new("postgres://localhost/test");
        // Should compile and create without errors
    }
    
    #[test]
    fn test_coordinator_with_custom_chunking() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test")
            .with_chunking_strategy(ChunkingStrategy::ByDuration { hours_per_chunk: 12 });
        
        // Verify it compiles with custom strategy
        let _ = coordinator;
    }
    
    #[test]
    fn test_parse_job_dates_success() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test");
        
        let params = BacktestJobParams {
            start_date: Some("2025-01-01T00:00:00Z".to_string()),
            end_date: Some("2025-01-31T00:00:00Z".to_string()),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            ..Default::default()
        };
        
        let (start, end) = coordinator.parse_job_dates(&params).unwrap();
        
        assert_eq!(start, Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(end, Utc.with_ymd_and_hms(2025, 1, 31, 0, 0, 0).unwrap());
    }
    
    #[test]
    fn test_parse_job_dates_missing_start() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test");
        
        let params = BacktestJobParams {
            start_date: None,
            end_date: Some("2025-01-31T00:00:00Z".to_string()),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            ..Default::default()
        };
        
        let result = coordinator.parse_job_dates(&params);
        assert!(matches!(result, Err(CoordinatorError::MissingParameter(_))));
    }
    
    #[test]
    fn test_parse_job_dates_invalid_format() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test");
        
        let params = BacktestJobParams {
            start_date: Some("not-a-date".to_string()),
            end_date: Some("2025-01-31T00:00:00Z".to_string()),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            ..Default::default()
        };
        
        let result = coordinator.parse_job_dates(&params);
        assert!(matches!(result, Err(CoordinatorError::InvalidParameter(_))));
    }
    
    #[test]
    fn test_create_chunk() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test");
        
        let job = BacktestJob {
            id: "job-123".to_string(),
            params: BacktestJobParams {
                symbol: "BTCUSDT".to_string(),
                exchange: "binance".to_string(),
                initial_capital: 10000.0,
                ..Default::default()
            },
            status: crate::job_queue::JobStatus::Queued,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            progress: 0.0,
            current_generation: 0,
            total_generations: 0,
            current_phase: "pending".to_string(),
            phase_details: serde_json::json!({}),
            error_message: None,
        };
        
        let chunk_info = BacktestChunkInfo {
            chunk_id: 2,
            total_chunks: 5,
            start_time: Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            end_time: Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap(),
            estimated_events: Some(100000),
            priority: 3,
        };
        
        let strategy_config = b"test config";
        let chunk = coordinator.create_chunk(&job, &chunk_info, strategy_config);
        
        assert_eq!(chunk.job_id, "job-123");
        assert_eq!(chunk.chunk_id, 2);
        assert_eq!(chunk.total_chunks, 5);
        assert_eq!(chunk.symbol, "BTCUSDT");
        assert_eq!(chunk.exchange, "binance");
        assert_eq!(chunk.initial_capital, 10000.0);
        assert_eq!(chunk.priority, 3);
    }
    
    #[test]
    fn test_estimate_remaining() {
        let coordinator = DistributedCoordinator::new("postgres://localhost/test");
        let deadline = Instant::now() + Duration::from_secs(1000);
        
        // 50% complete with 1000s remaining
        let remaining = coordinator.estimate_remaining(5, 10, deadline);
        assert!(remaining > 0);
    }
    
    #[test]
    fn test_coordinator_error_display() {
        let err = CoordinatorError::MissingParameter("start_date".to_string());
        assert_eq!(format!("{}", err), "Missing parameter: start_date");
        
        let err = CoordinatorError::Timeout { received: 5, expected: 10 };
        assert_eq!(format!("{}", err), "Timeout: received 5 of 10 chunks");
    }
}
