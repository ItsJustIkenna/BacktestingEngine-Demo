//! Broker client for distributed backtesting
//!
//! Provides a unified interface for publishing and subscribing to
//! backtest messages via MessageBrokerEngine.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{broadcast, RwLock};

use protocol::broker::messages::{
    BacktestChunk, BacktestChunkResult, BacktestProgress, 
    BacktestCancelRequest, ChromosomeEvalRequest, ChromosomeEvalResult,
    DataBroadcastRequest, DataCacheAck,
};

use crate::{log_info_structured, log_debug};
use crate::logging_facade::MAIN_LOGGER;

/// Connection mode for the broker client
#[derive(Debug, Clone)]
pub enum BrokerMode {
    /// In-process broker using channels (single-node)
    InProcess,
    /// Network broker (multi-node) - connects to MessageBrokerEngine server
    Network {
        host: String,
        port: u16,
    },
}

impl Default for BrokerMode {
    fn default() -> Self {
        BrokerMode::InProcess
    }
}

/// Broker client for distributed backtest communication
pub struct BacktestBrokerClient {
    mode: BrokerMode,
    /// In-process channels for chunk distribution
    chunk_senders: Arc<RwLock<HashMap<String, broadcast::Sender<BacktestChunk>>>>,
    /// In-process channels for results
    result_senders: Arc<RwLock<HashMap<String, broadcast::Sender<BacktestChunkResult>>>>,
    /// In-process channels for progress
    progress_senders: Arc<RwLock<HashMap<String, broadcast::Sender<BacktestProgress>>>>,
    /// In-process channel for all chunks (workers subscribe to this)
    all_chunks_sender: broadcast::Sender<BacktestChunk>,
    /// In-process channel for all results (coordinator listens)
    all_results_sender: broadcast::Sender<BacktestChunkResult>,
    /// Cancel requests
    cancel_senders: Arc<RwLock<HashMap<String, broadcast::Sender<BacktestCancelRequest>>>>,
    
    // ========================================================================
    // GENETIC ALGORITHM CHANNELS
    // ========================================================================
    
    /// Channel for chromosome evaluation requests (coordinator -> workers)
    chromosome_request_sender: broadcast::Sender<ChromosomeEvalRequest>,
    /// Channel for chromosome evaluation results (workers -> coordinator)
    chromosome_result_sender: broadcast::Sender<ChromosomeEvalResult>,
    /// Channel for data broadcast requests
    data_broadcast_sender: broadcast::Sender<DataBroadcastRequest>,
    /// Channel for data cache acknowledgments
    data_ack_sender: broadcast::Sender<DataCacheAck>,
}

impl BacktestBrokerClient {
    /// Create a new broker client
    pub fn new(mode: BrokerMode) -> Self {
        let (all_chunks_sender, _) = broadcast::channel(10_000);
        let (all_results_sender, _) = broadcast::channel(10_000);
        
        // GA channels
        let (chromosome_request_sender, _) = broadcast::channel(1_000);
        let (chromosome_result_sender, _) = broadcast::channel(1_000);
        let (data_broadcast_sender, _) = broadcast::channel(100);
        let (data_ack_sender, _) = broadcast::channel(100);
        
        Self {
            mode,
            chunk_senders: Arc::new(RwLock::new(HashMap::new())),
            result_senders: Arc::new(RwLock::new(HashMap::new())),
            progress_senders: Arc::new(RwLock::new(HashMap::new())),
            all_chunks_sender,
            all_results_sender,
            cancel_senders: Arc::new(RwLock::new(HashMap::new())),
            chromosome_request_sender,
            chromosome_result_sender,
            data_broadcast_sender,
            data_ack_sender,
        }
    }
    
    /// Create an in-process client (for single-node deployment)
    pub fn in_process() -> Self {
        Self::new(BrokerMode::InProcess)
    }
    
    /// Create a network client (for multi-node deployment)
    pub fn network(host: &str, port: u16) -> Self {
        Self::new(BrokerMode::Network {
            host: host.to_string(),
            port,
        })
    }
    
    // ========================================================================
    // CHUNK PUBLISHING & SUBSCRIBING
    // ========================================================================
    
    /// Publish a chunk for processing
    pub async fn publish_chunk(&self, chunk: BacktestChunk) -> Result<(), BrokerError> {
        let job_id = chunk.job_id.clone();
        let chunk_id = chunk.chunk_id;
        let total_chunks = chunk.total_chunks;
        
        match &self.mode {
            BrokerMode::InProcess => {
                // Send to job-specific channel
                {
                    let senders = self.chunk_senders.read().await;
                    if let Some(sender) = senders.get(&job_id) {
                        let _ = sender.send(chunk.clone());
                    }
                }
                
                // Also send to all-chunks channel (for workers)
                let _ = self.all_chunks_sender.send(chunk);
                
                log_debug!(MAIN_LOGGER, "Published chunk {}/{} for job {}", 
                    chunk_id, total_chunks, job_id);
                Ok(())
            }
            BrokerMode::Network { host: _, port: _ } => {
                // TODO: Implement network publishing via MessageBrokerEngine
                // For now, fall back to in-process
                log_debug!(MAIN_LOGGER, "Network mode not yet implemented, using in-process for job {}", job_id);
                self.all_chunks_sender.send(chunk).map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
        }
    }
    
    /// Subscribe to chunks for a specific job
    pub async fn subscribe_chunks(&self, job_id: &str) -> broadcast::Receiver<BacktestChunk> {
        let mut senders = self.chunk_senders.write().await;
        
        let sender = senders.entry(job_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(1000);
                sender
            });
        
        sender.subscribe()
    }
    
    /// Subscribe to all chunks (for workers)
    pub fn subscribe_all_chunks(&self) -> broadcast::Receiver<BacktestChunk> {
        self.all_chunks_sender.subscribe()
    }
    
    // ========================================================================
    // RESULT PUBLISHING & SUBSCRIBING
    // ========================================================================
    
    /// Publish a chunk result
    pub async fn publish_result(&self, result: BacktestChunkResult) -> Result<(), BrokerError> {
        let job_id = result.job_id.clone();
        let chunk_id = result.chunk_id;
        
        match &self.mode {
            BrokerMode::InProcess => {
                // Send to job-specific channel
                {
                    let senders = self.result_senders.read().await;
                    if let Some(sender) = senders.get(&job_id) {
                        let _ = sender.send(result.clone());
                    }
                }
                
                // Also send to all-results channel (for coordinator)
                let _ = self.all_results_sender.send(result);
                
                log_debug!(MAIN_LOGGER, "Published result for chunk {} of job {}", 
                    chunk_id, job_id);
                Ok(())
            }
            BrokerMode::Network { .. } => {
                // TODO: Network publishing
                self.all_results_sender.send(result).map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
        }
    }
    
    /// Subscribe to results for a specific job
    pub async fn subscribe_results(&self, job_id: &str) -> broadcast::Receiver<BacktestChunkResult> {
        let mut senders = self.result_senders.write().await;
        
        let sender = senders.entry(job_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(1000);
                sender
            });
        
        sender.subscribe()
    }
    
    /// Subscribe to all results (for coordinator)
    pub fn subscribe_all_results(&self) -> broadcast::Receiver<BacktestChunkResult> {
        self.all_results_sender.subscribe()
    }
    
    // ========================================================================
    // PROGRESS PUBLISHING & SUBSCRIBING
    // ========================================================================
    
    /// Publish progress update
    pub async fn publish_progress(&self, progress: BacktestProgress) -> Result<(), BrokerError> {
        let job_id = progress.job_id.clone();
        
        let senders = self.progress_senders.read().await;
        if let Some(sender) = senders.get(&job_id) {
            sender.send(progress).map_err(|_| BrokerError::SendFailed)?;
        }
        
        Ok(())
    }
    
    /// Subscribe to progress for a job
    pub async fn subscribe_progress(&self, job_id: &str) -> broadcast::Receiver<BacktestProgress> {
        let mut senders = self.progress_senders.write().await;
        
        let sender = senders.entry(job_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(100);
                sender
            });
        
        sender.subscribe()
    }
    
    // ========================================================================
    // CANCELLATION
    // ========================================================================
    
    /// Request cancellation of a job
    pub async fn cancel_job(&self, job_id: &str, reason: &str) -> Result<(), BrokerError> {
        let request = BacktestCancelRequest {
            job_id: job_id.to_string(),
            reason: reason.to_string(),
        };
        
        let senders = self.cancel_senders.read().await;
        if let Some(sender) = senders.get(job_id) {
            sender.send(request).map_err(|_| BrokerError::SendFailed)?;
        }
        
        log_info_structured!(MAIN_LOGGER, "JOB_CANCELLATION_REQUESTED",
            "event" => "JOB_CANCELLATION_REQUESTED",
            "job_id" => job_id,
            "reason" => reason
        );
        Ok(())
    }
    
    /// Subscribe to cancellation requests for a job
    pub async fn subscribe_cancel(&self, job_id: &str) -> broadcast::Receiver<BacktestCancelRequest> {
        let mut senders = self.cancel_senders.write().await;
        
        let sender = senders.entry(job_id.to_string())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(10);
                sender
            });
        
        sender.subscribe()
    }
    
    // ========================================================================
    // CLEANUP
    // ========================================================================
    
    /// Clean up channels for a completed job
    pub async fn cleanup_job(&self, job_id: &str) {
        self.chunk_senders.write().await.remove(job_id);
        self.result_senders.write().await.remove(job_id);
        self.progress_senders.write().await.remove(job_id);
        self.cancel_senders.write().await.remove(job_id);
        
        log_debug!(MAIN_LOGGER, "Cleaned up broker channels for job {}", job_id);
    }
    
    // ========================================================================
    // GENETIC ALGORITHM - CHROMOSOME EVALUATION
    // ========================================================================
    
    /// Publish a chromosome evaluation request (coordinator -> workers)
    pub fn publish_chromosome_request(&self, request: ChromosomeEvalRequest) -> Result<(), BrokerError> {
        self.chromosome_request_sender.send(request)
            .map_err(|_| BrokerError::SendFailed)?;
        Ok(())
    }
    
    /// Subscribe to chromosome evaluation requests (worker)
    pub fn subscribe_chromosome_requests(&self) -> broadcast::Receiver<ChromosomeEvalRequest> {
        self.chromosome_request_sender.subscribe()
    }
    
    /// Publish a chromosome evaluation result (worker -> coordinator)
    pub fn publish_chromosome_result(&self, result: ChromosomeEvalResult) -> Result<(), BrokerError> {
        self.chromosome_result_sender.send(result)
            .map_err(|_| BrokerError::SendFailed)?;
        Ok(())
    }
    
    /// Subscribe to chromosome evaluation results (coordinator)
    pub fn subscribe_chromosome_results(&self) -> broadcast::Receiver<ChromosomeEvalResult> {
        self.chromosome_result_sender.subscribe()
    }
    
    // ========================================================================
    // GENETIC ALGORITHM - DATA BROADCASTING
    // ========================================================================
    
    /// Publish a data broadcast request (coordinator -> workers)
    pub fn publish_data_broadcast(&self, request: DataBroadcastRequest) -> Result<(), BrokerError> {
        self.data_broadcast_sender.send(request)
            .map_err(|_| BrokerError::SendFailed)?;
        Ok(())
    }
    
    /// Subscribe to data broadcast requests (workers)
    pub fn subscribe_data_broadcasts(&self) -> broadcast::Receiver<DataBroadcastRequest> {
        self.data_broadcast_sender.subscribe()
    }
    
    /// Publish a data cache acknowledgment (worker -> coordinator)
    pub fn publish_data_ack(&self, ack: DataCacheAck) -> Result<(), BrokerError> {
        self.data_ack_sender.send(ack)
            .map_err(|_| BrokerError::SendFailed)?;
        Ok(())
    }
    
    /// Subscribe to data cache acknowledgments (coordinator)
    pub fn subscribe_data_acks(&self) -> broadcast::Receiver<DataCacheAck> {
        self.data_ack_sender.subscribe()
    }
}

impl Clone for BacktestBrokerClient {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode.clone(),
            chunk_senders: Arc::clone(&self.chunk_senders),
            result_senders: Arc::clone(&self.result_senders),
            progress_senders: Arc::clone(&self.progress_senders),
            all_chunks_sender: self.all_chunks_sender.clone(),
            all_results_sender: self.all_results_sender.clone(),
            cancel_senders: Arc::clone(&self.cancel_senders),
            chromosome_request_sender: self.chromosome_request_sender.clone(),
            chromosome_result_sender: self.chromosome_result_sender.clone(),
            data_broadcast_sender: self.data_broadcast_sender.clone(),
            data_ack_sender: self.data_ack_sender.clone(),
        }
    }
}

/// Global broker client instance
static BROKER_CLIENT: once_cell::sync::Lazy<Arc<BacktestBrokerClient>> =
    once_cell::sync::Lazy::new(|| Arc::new(BacktestBrokerClient::in_process()));

/// Get the global broker client
pub fn get_broker_client() -> Arc<BacktestBrokerClient> {
    Arc::clone(&BROKER_CLIENT)
}

/// Broker error types
#[derive(Debug, Clone)]
pub enum BrokerError {
    SendFailed,
    ReceiveFailed,
    ConnectionFailed(String),
    SerializationFailed(String),
    Timeout,
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerError::SendFailed => write!(f, "Failed to send message"),
            BrokerError::ReceiveFailed => write!(f, "Failed to receive message"),
            BrokerError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            BrokerError::SerializationFailed(msg) => write!(f, "Serialization failed: {}", msg),
            BrokerError::Timeout => write!(f, "Operation timed out"),
        }
    }
}

impl std::error::Error for BrokerError {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_in_process_chunk_pubsub() {
        let client = BacktestBrokerClient::in_process();
        
        // Subscribe first
        let mut rx = client.subscribe_all_chunks();
        
        // Publish a chunk
        let chunk = BacktestChunk {
            job_id: "test-job-1".to_string(),
            chunk_id: 0,
            total_chunks: 5,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            end_time: "2025-01-02T00:00:00Z".to_string(),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            strategy_config: vec![],
            initial_capital: 10000.0,
            starting_position: 0.0,
            starting_cash: 10000.0,
            is_monte_carlo: false,
            monte_carlo_run: 0,
            priority: 5,
        };
        
        client.publish_chunk(chunk.clone()).await.unwrap();
        
        // Should receive the chunk
        let received = rx.recv().await.unwrap();
        assert_eq!(received.job_id, "test-job-1");
        assert_eq!(received.chunk_id, 0);
    }
    
    #[tokio::test]
    async fn test_in_process_result_pubsub() {
        let client = BacktestBrokerClient::in_process();
        
        let mut rx = client.subscribe_all_results();
        
        let result = BacktestChunkResult {
            job_id: "test-job-1".to_string(),
            chunk_id: 0,
            worker_id: "worker-1".to_string(),
            success: true,
            error_message: String::new(),
            net_pnl: 150.0,
            gross_profit: 200.0,
            gross_loss: 50.0,
            num_trades: 10,
            winning_trades: 7,
            max_drawdown: 25.0,
            transaction_costs: 15.0,
            ending_position: 0.1,
            ending_cash: 9850.0,
            daily_returns: vec![],
            events_processed: 50000,
            processing_duration_ms: 1500,
            peak_memory_bytes: 256_000_000,
            monte_carlo_run: 0,
        };
        
        client.publish_result(result.clone()).await.unwrap();
        
        let received = rx.recv().await.unwrap();
        assert_eq!(received.job_id, "test-job-1");
        assert!(received.success);
        assert_eq!(received.net_pnl, 150.0);
    }
}
