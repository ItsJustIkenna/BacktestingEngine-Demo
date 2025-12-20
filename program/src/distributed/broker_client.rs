//! Broker client for distributed backtesting
//!
//! Provides a unified interface for publishing and subscribing to
//! backtest messages via MessageBrokerEngine.
//!
//! # Network Mode
//!
//! When MESSAGE_BROKER_URL is set, the client will use TCP connections to
//! MessageBrokerEngine for cross-pod communication. This enables true
//! distributed genetic algorithm optimization.
//!
//! # In-Process Mode (Default)
//!
//! Uses tokio broadcast channels. Only works within a single process.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock, Mutex};
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;

use protocol::broker::messages::{
    BacktestChunk, BacktestChunkResult, BacktestProgress, 
    BacktestCancelRequest, ChromosomeEvalRequest, ChromosomeEvalResult,
    DataBroadcastRequest, DataLoadRequest, DataCacheAck,
};
use prost::Message;

use crate::{log_info_structured, log_debug, log_error, log_warn};
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

/// Role for broker client - determines which topics to subscribe to
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BrokerRole {
    /// Coordinator: publishes chromosome requests, subscribes to results
    Coordinator,
    /// Worker: subscribes to chromosome requests, publishes results
    Worker,
}

impl Default for BrokerMode {
    fn default() -> Self {
        // Check for MESSAGE_BROKER_URL environment variable
        if let Ok(url) = std::env::var("MESSAGE_BROKER_URL") {
            if let Some(mode) = Self::parse_url(&url) {
                return mode;
            }
        }
        BrokerMode::InProcess
    }
}

impl BrokerMode {
    /// Parse a URL like "http://host:port" or "tcp://host:port" into BrokerMode
    fn parse_url(url: &str) -> Option<BrokerMode> {
        // Strip protocol prefix
        let host_port = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("tcp://"))
            .unwrap_or(url);
        
        // Parse host:port
        let parts: Vec<&str> = host_port.split(':').collect();
        if parts.len() >= 2 {
            let host = parts[0].to_string();
            if let Ok(port) = parts[1].trim_end_matches('/').parse::<u16>() {
                return Some(BrokerMode::Network { host, port });
            }
        }
        None
    }
}

/// Topic names for distributed GA communication
pub mod topics {
    pub const CHROMOSOME_REQUESTS: &str = "ga.chromosome.requests";
    pub const CHROMOSOME_RESULTS: &str = "ga.chromosome.results";
    pub const DATA_BROADCAST: &str = "ga.data.broadcast";
    pub const DATA_LOAD: &str = "ga.data.load"; // Workers load data from DB
    pub const DATA_ACK: &str = "ga.data.ack";
    pub const BACKTEST_CHUNKS: &str = "backtest.chunks";
    pub const BACKTEST_RESULTS: &str = "backtest.results";
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
    /// Channel for data broadcast requests (deprecated - use data_load for large datasets)
    data_broadcast_sender: broadcast::Sender<DataBroadcastRequest>,
    /// Channel for data load requests (workers query DB directly)
    data_load_sender: broadcast::Sender<DataLoadRequest>,
    /// Channel for data cache acknowledgments
    data_ack_sender: broadcast::Sender<DataCacheAck>,
    
    // ========================================================================
    // NETWORK MODE - TCP Connection
    // ========================================================================
    
    /// TCP connection to MessageBrokerEngine (for Network mode)
    tcp_connection: Arc<Mutex<Option<TcpStream>>>,
    /// Whether we're connected to the broker
    is_connected: Arc<std::sync::atomic::AtomicBool>,
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
        let (data_load_sender, _) = broadcast::channel(100);
        let (data_ack_sender, _) = broadcast::channel(100);
        
        // Note: Logging skipped in constructor to avoid Lazy initialization issues during tests
        // The mode is logged when connect() is called instead
        
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
            data_load_sender,
            data_ack_sender,
            tcp_connection: Arc::new(Mutex::new(None)),
            is_connected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
    
    /// Create a client based on environment configuration
    pub fn from_env() -> Self {
        Self::new(BrokerMode::default())
    }
    
    /// Connect to the message broker (only needed for Network mode)
    pub async fn connect(&self) -> Result<(), BrokerError> {
        log_info_structured!(MAIN_LOGGER, "BROKER_CLIENT_INIT",
            "event" => "BROKER_CLIENT_INIT",
            "mode" => format!("{:?}", self.mode)
        );
        
        match &self.mode {
            BrokerMode::InProcess => {
                log_debug!(MAIN_LOGGER, "In-process mode, no connection needed");
                Ok(())
            }
            BrokerMode::Network { host, port } => {
                let addr = format!("{}:{}", host, port);
                println!("🔌 [BROKER] connect() called - establishing PUBLISH connection to {}", addr);
                log_info_structured!(MAIN_LOGGER, "BROKER_CONNECTING",
                    "event" => "BROKER_CONNECTING",
                    "address" => addr.clone()
                );
                
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    TcpStream::connect(&addr)
                ).await {
                    Ok(Ok(stream)) => {
                        // Configure for low latency
                        let _ = stream.set_nodelay(true);
                        
                        println!("✅ [BROKER] connect() - PUBLISH connection established to {}", addr);
                        
                        let mut conn = self.tcp_connection.lock().await;
                        *conn = Some(stream);
                        self.is_connected.store(true, std::sync::atomic::Ordering::Relaxed);
                        
                        log_info_structured!(MAIN_LOGGER, "BROKER_CONNECTED",
                            "event" => "BROKER_CONNECTED",
                            "address" => addr
                        );
                        Ok(())
                    }
                    Ok(Err(e)) => {
                        println!("❌ [BROKER] connect() failed: {}", e);
                        log_error!(MAIN_LOGGER, "Failed to connect to broker at {}: {}", addr, e);
                        Err(BrokerError::ConnectionFailed(e.to_string()))
                    }
                    Err(_) => {
                        println!("❌ [BROKER] connect() timed out");
                        log_error!(MAIN_LOGGER, "Connection to broker at {} timed out", addr);
                        Err(BrokerError::Timeout)
                    }
                }
            }
        }
    }
    
    /// Check if connected (for Network mode)
    pub fn is_connected(&self) -> bool {
        match &self.mode {
            BrokerMode::InProcess => true,
            BrokerMode::Network { .. } => {
                self.is_connected.load(std::sync::atomic::Ordering::Relaxed)
            }
        }
    }
    
    /// Publish raw bytes to a topic via TCP with retry and reconnection
    async fn publish_tcp(&self, topic: &str, data: &[u8]) -> Result<(), BrokerError> {
        const MAX_RETRIES: usize = 3;
        let mut last_error = None;
        
        for attempt in 0..MAX_RETRIES {
            // Try to reconnect if not connected
            if !self.is_connected.load(std::sync::atomic::Ordering::Relaxed) {
                println!("🔄 [BROKER] Reconnecting before publish (attempt {}/{})", attempt + 1, MAX_RETRIES);
                if let Err(e) = self.reconnect_tcp().await {
                    println!("❌ [BROKER] Reconnection failed: {}", e);
                    last_error = Some(e);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    continue;
                }
            }
            
            // Try to publish
            match self.publish_tcp_inner(topic, data).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    println!("❌ [BROKER] Publish failed (attempt {}/{}): {}", attempt + 1, MAX_RETRIES, e);
                    last_error = Some(e);
                    self.is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
        
        Err(last_error.unwrap_or(BrokerError::ConnectionFailed("Max retries exceeded".to_string())))
    }
    
    /// Reconnect the TCP publish connection
    async fn reconnect_tcp(&self) -> Result<(), BrokerError> {
        if let BrokerMode::Network { host, port } = &self.mode {
            let addr = format!("{}:{}", host, port);
            println!("🔌 [BROKER] Attempting TCP reconnection to {}", addr);
            
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(10),
                tokio::net::TcpStream::connect(&addr)
            ).await {
                Ok(Ok(stream)) => {
                    let mut conn = self.tcp_connection.lock().await;
                    *conn = Some(stream);
                    self.is_connected.store(true, std::sync::atomic::Ordering::Relaxed);
                    println!("✅ [BROKER] TCP reconnection successful");
                    Ok(())
                }
                Ok(Err(e)) => {
                    Err(BrokerError::ConnectionFailed(e.to_string()))
                }
                Err(_) => {
                    Err(BrokerError::Timeout)
                }
            }
        } else {
            Ok(()) // InProcess mode doesn't need reconnection
        }
    }
    
    /// Inner publish without retry logic
    async fn publish_tcp_inner(&self, topic: &str, data: &[u8]) -> Result<(), BrokerError> {
        let mut conn = self.tcp_connection.lock().await;
        if let Some(ref mut stream) = *conn {
            // Wire format: [msg_type: u8][topic_len: u32][topic: bytes][data_len: u32][data: bytes]
            // msg_type: 0x01 = PUBLISH, 0x02 = SUBSCRIBE
            let topic_bytes = topic.as_bytes();
            let mut buffer = Vec::with_capacity(1 + 8 + topic_bytes.len() + data.len());
            
            buffer.push(0x01); // PUBLISH message type
            buffer.extend_from_slice(&(topic_bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(topic_bytes);
            buffer.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buffer.extend_from_slice(data);
            
            println!("📤 [BROKER] Publishing to topic '{}' ({} bytes)", topic, data.len());
            
            stream.write_all(&buffer).await.map_err(|e| {
                println!("❌ [BROKER] Failed to write to TCP stream: {}", e);
                self.is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                BrokerError::ConnectionFailed(e.to_string())
            })?;
            
            stream.flush().await.map_err(|e| {
                println!("❌ [BROKER] Failed to flush TCP stream: {}", e);
                BrokerError::ConnectionFailed(e.to_string())
            })?;
            
            println!("✅ [BROKER] Published successfully to '{}'", topic);
            
            Ok(())
        } else {
            println!("❌ [BROKER] Cannot publish - not connected");
            Err(BrokerError::ConnectionFailed("Not connected".to_string()))
        }
    }
    
    /// Subscribe to a topic via TCP
    #[allow(dead_code)]
    async fn subscribe_tcp(&self, topic_pattern: &str) -> Result<(), BrokerError> {
        let mut conn = self.tcp_connection.lock().await;
        if let Some(ref mut stream) = *conn {
            // Wire format: [msg_type: u8][topic_len: u32][topic_pattern: bytes]
            // msg_type: 0x02 = SUBSCRIBE
            let topic_bytes = topic_pattern.as_bytes();
            let mut buffer = Vec::with_capacity(1 + 4 + topic_bytes.len());
            
            buffer.push(0x02); // SUBSCRIBE message type
            buffer.extend_from_slice(&(topic_bytes.len() as u32).to_le_bytes());
            buffer.extend_from_slice(topic_bytes);
            
            stream.write_all(&buffer).await.map_err(|e| {
                self.is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
                BrokerError::ConnectionFailed(e.to_string())
            })?;
            
            stream.flush().await.map_err(|e| {
                BrokerError::ConnectionFailed(e.to_string())
            })?;
            
            log_debug!(MAIN_LOGGER, "Subscribed to topic pattern: {}", topic_pattern);
            Ok(())
        } else {
            Err(BrokerError::ConnectionFailed("Not connected".to_string()))
        }
    }

    /// Start receiving messages from broker (for Network mode)
    /// This spawns a background task that reads messages and forwards them to local channels
    pub async fn start_receiver(&self, role: BrokerRole) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => Ok(()), // No-op for in-process
            BrokerMode::Network { host, port } => {
                // Create a separate connection for receiving (subscriptions)
                let addr = format!("{}:{}", host, port);
                
                println!("🔌 [BROKER] Connecting to {} for {:?} receiver...", addr, role);
                
                let stream = tokio::time::timeout(
                    Duration::from_secs(10),
                    TcpStream::connect(&addr)
                ).await
                    .map_err(|_| {
                        println!("❌ [BROKER] Connection timeout to {}", addr);
                        BrokerError::Timeout
                    })?
                    .map_err(|e| {
                        println!("❌ [BROKER] Connection failed to {}: {}", addr, e);
                        BrokerError::ConnectionFailed(e.to_string())
                    })?;
                
                println!("✅ [BROKER] TCP connection established to {}", addr);
                
                let _ = stream.set_nodelay(true);
                
                let (reader, mut writer) = stream.into_split();
                
                // Subscribe to ALL topics needed for this role
                let topics_to_subscribe: Vec<&str> = match role {
                    BrokerRole::Coordinator => vec![
                        topics::CHROMOSOME_RESULTS,  // Receive evaluation results from workers
                        topics::DATA_ACK,            // Receive data cache acknowledgments
                    ],
                    BrokerRole::Worker => vec![
                        topics::CHROMOSOME_REQUESTS, // Receive evaluation requests from coordinator
                        topics::DATA_BROADCAST,      // Receive market data broadcasts (legacy)
                        topics::DATA_LOAD,           // Receive data load requests (new - workers query DB)
                    ],
                };
                
                // Send SUBSCRIBE message for each topic
                for topic in &topics_to_subscribe {
                    let topic_bytes = topic.as_bytes();
                    let mut buffer = Vec::with_capacity(1 + 4 + topic_bytes.len());
                    buffer.push(0x02); // SUBSCRIBE
                    buffer.extend_from_slice(&(topic_bytes.len() as u32).to_le_bytes());
                    buffer.extend_from_slice(topic_bytes);
                    writer.write_all(&buffer).await.map_err(|e| BrokerError::ConnectionFailed(e.to_string()))?;
                    println!("📝 [BROKER] Sent SUBSCRIBE for topic: {}", topic);
                }
                writer.flush().await.map_err(|e| BrokerError::ConnectionFailed(e.to_string()))?;
                println!("✅ [BROKER] All subscriptions sent and flushed");
                
                log_info_structured!(MAIN_LOGGER, "BROKER_SUBSCRIBER_STARTED",
                    "event" => "BROKER_SUBSCRIBER_STARTED",
                    "role" => format!("{:?}", role),
                    "topics" => format!("{:?}", topics_to_subscribe)
                );
                
                // Spawn unified receiver task that routes messages based on topic
                let chromosome_request_sender = self.chromosome_request_sender.clone();
                let chromosome_result_sender = self.chromosome_result_sender.clone();
                let data_broadcast_sender = self.data_broadcast_sender.clone();
                let data_load_sender = self.data_load_sender.clone();
                let data_ack_sender = self.data_ack_sender.clone();
                
                tokio::spawn(async move {
                    Self::unified_receiver_loop(
                        reader,
                        chromosome_request_sender,
                        chromosome_result_sender,
                        data_broadcast_sender,
                        data_load_sender,
                        data_ack_sender,
                    ).await;
                });
                
                Ok(())
            }
        }
    }
    
    /// Unified receiver loop that routes messages based on topic
    async fn unified_receiver_loop(
        mut reader: tokio::net::tcp::OwnedReadHalf,
        chromosome_request_sender: broadcast::Sender<ChromosomeEvalRequest>,
        chromosome_result_sender: broadcast::Sender<ChromosomeEvalResult>,
        data_broadcast_sender: broadcast::Sender<DataBroadcastRequest>,
        data_load_sender: broadcast::Sender<DataLoadRequest>,
        data_ack_sender: broadcast::Sender<DataCacheAck>,
    ) {
        use tokio::io::AsyncReadExt;
        let mut buf_reader = tokio::io::BufReader::new(&mut reader);
        
        println!("🔄 [BROKER] Receiver loop started, waiting for messages...");
        
        loop {
            // Read incoming messages: [topic_len: u32][topic][data_len: u32][data]
            println!("🔄 [BROKER] Receiver loop: waiting to read topic_len...");
            let topic_len = match buf_reader.read_u32_le().await {
                Ok(len) => {
                    println!("🔄 [BROKER] Receiver loop: got topic_len = {}", len);
                    len as usize
                },
                Err(e) => {
                    println!("❌ [BROKER] Receiver loop error reading topic_len: {}", e);
                    break;
                }
            };
            
            if topic_len > 1024 {
                println!("❌ [BROKER] Topic length too large: {}", topic_len);
                break;
            }
            
            let mut topic_buf = vec![0u8; topic_len];
            if buf_reader.read_exact(&mut topic_buf).await.is_err() {
                println!("❌ [BROKER] Failed to read topic bytes");
                break;
            }
            let topic = String::from_utf8_lossy(&topic_buf);
            
            let data_len = match buf_reader.read_u32_le().await {
                Ok(len) => len as usize,
                Err(_) => break,
            };
            
            // Cap at 256MB for data broadcasts with 7M records
            if data_len > 256 * 1024 * 1024 {
                log_warn!(MAIN_LOGGER, "Received message too large: {} bytes on topic {}", data_len, topic);
                break;
            }
            
            let mut data = vec![0u8; data_len];
            if buf_reader.read_exact(&mut data).await.is_err() {
                break;
            }
            
            println!("📨 [BROKER] Received message on topic: {} ({} bytes)", topic, data_len);
            
            // Route based on topic
            match topic.as_ref() {
                t if t == topics::CHROMOSOME_REQUESTS => {
                    if let Ok(request) = ChromosomeEvalRequest::decode(&data[..]) {
                        println!("📥 [BROKER] Routed CHROMOSOME_REQUEST");
                        let _ = chromosome_request_sender.send(request);
                    }
                }
                t if t == topics::CHROMOSOME_RESULTS => {
                    if let Ok(result) = ChromosomeEvalResult::decode(&data[..]) {
                        println!("📥 [BROKER] Routed CHROMOSOME_RESULT");
                        let _ = chromosome_result_sender.send(result);
                    }
                }
                t if t == topics::DATA_BROADCAST => {
                    if let Ok(broadcast) = DataBroadcastRequest::decode(&data[..]) {
                        println!("📥 [BROKER] Routed DATA_BROADCAST");
                        let _ = data_broadcast_sender.send(broadcast);
                    }
                }
                t if t == topics::DATA_LOAD => {
                    if let Ok(load_request) = DataLoadRequest::decode(&data[..]) {
                        println!("📥 [BROKER] Routed DATA_LOAD");
                        let _ = data_load_sender.send(load_request);
                    }
                }
                t if t == topics::DATA_ACK => {
                    if let Ok(ack) = DataCacheAck::decode(&data[..]) {
                        println!("📥 [BROKER] Routed DATA_ACK");
                        let _ = data_ack_sender.send(ack);
                    }
                }
                _ => {
                    log_debug!(MAIN_LOGGER, "Received message on unknown topic: {}", topic);
                }
            }
        }
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
            BrokerMode::Network { .. } => {
                // Serialize and send via TCP
                let topic = format!("{}.{}", topics::BACKTEST_CHUNKS, job_id);
                let data = chunk.encode_to_vec();
                
                self.publish_tcp(&topic, &data).await?;
                
                // Also publish to in-process channels for local subscribers
                let _ = self.all_chunks_sender.send(chunk);
                
                log_debug!(MAIN_LOGGER, "Published chunk {}/{} for job {} via network", 
                    chunk_id, total_chunks, job_id);
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
                // Serialize and send via TCP
                let topic = format!("{}.{}", topics::BACKTEST_RESULTS, job_id);
                let data = result.encode_to_vec();
                
                self.publish_tcp(&topic, &data).await?;
                
                // Also publish to in-process channels for local subscribers
                let _ = self.all_results_sender.send(result);
                
                log_debug!(MAIN_LOGGER, "Published result for chunk {} of job {} via network", 
                    chunk_id, job_id);
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
    pub async fn publish_chromosome_request(&self, request: ChromosomeEvalRequest) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => {
                self.chromosome_request_sender.send(request)
                    .map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
            BrokerMode::Network { .. } => {
                let data = request.encode_to_vec();
                self.publish_tcp(topics::CHROMOSOME_REQUESTS, &data).await?;
                // Also publish locally
                let _ = self.chromosome_request_sender.send(request);
                Ok(())
            }
        }
    }
    
    /// Subscribe to chromosome evaluation requests (worker)
    pub fn subscribe_chromosome_requests(&self) -> broadcast::Receiver<ChromosomeEvalRequest> {
        self.chromosome_request_sender.subscribe()
    }
    
    /// Publish a chromosome evaluation result (worker -> coordinator)
    pub async fn publish_chromosome_result(&self, result: ChromosomeEvalResult) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => {
                self.chromosome_result_sender.send(result)
                    .map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
            BrokerMode::Network { .. } => {
                let data = result.encode_to_vec();
                self.publish_tcp(topics::CHROMOSOME_RESULTS, &data).await?;
                // Also publish locally
                let _ = self.chromosome_result_sender.send(result);
                Ok(())
            }
        }
    }
    
    /// Subscribe to chromosome evaluation results (coordinator)
    pub fn subscribe_chromosome_results(&self) -> broadcast::Receiver<ChromosomeEvalResult> {
        self.chromosome_result_sender.subscribe()
    }
    
    // ========================================================================
    // GENETIC ALGORITHM - DATA BROADCASTING
    // ========================================================================
    
    /// Publish a data broadcast request (coordinator -> workers)
    pub async fn publish_data_broadcast(&self, request: DataBroadcastRequest) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => {
                self.data_broadcast_sender.send(request)
                    .map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
            BrokerMode::Network { .. } => {
                let data = request.encode_to_vec();
                self.publish_tcp(topics::DATA_BROADCAST, &data).await?;
                // Also publish locally
                let _ = self.data_broadcast_sender.send(request);
                Ok(())
            }
        }
    }
    
    /// Subscribe to data broadcast requests (workers) - DEPRECATED: use subscribe_data_load_requests
    pub fn subscribe_data_broadcasts(&self) -> broadcast::Receiver<DataBroadcastRequest> {
        self.data_broadcast_sender.subscribe()
    }
    
    /// Publish a data load request (coordinator -> workers)
    /// Workers will load data from database directly (no large payload over broker)
    pub async fn publish_data_load_request(&self, request: DataLoadRequest) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => {
                self.data_load_sender.send(request)
                    .map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
            BrokerMode::Network { .. } => {
                let data = request.encode_to_vec();
                self.publish_tcp(topics::DATA_LOAD, &data).await?;
                // Also publish locally
                let _ = self.data_load_sender.send(request);
                Ok(())
            }
        }
    }
    
    /// Subscribe to data load requests (workers load from DB)
    pub fn subscribe_data_load_requests(&self) -> broadcast::Receiver<DataLoadRequest> {
        self.data_load_sender.subscribe()
    }
    
    /// Publish a data cache acknowledgment (worker -> coordinator)
    pub async fn publish_data_ack(&self, ack: DataCacheAck) -> Result<(), BrokerError> {
        match &self.mode {
            BrokerMode::InProcess => {
                self.data_ack_sender.send(ack)
                    .map_err(|_| BrokerError::SendFailed)?;
                Ok(())
            }
            BrokerMode::Network { .. } => {
                let data = ack.encode_to_vec();
                self.publish_tcp(topics::DATA_ACK, &data).await?;
                let _ = self.data_ack_sender.send(ack);
                Ok(())
            }
        }
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
            data_load_sender: self.data_load_sender.clone(),
            data_ack_sender: self.data_ack_sender.clone(),
            tcp_connection: Arc::clone(&self.tcp_connection),
            is_connected: Arc::clone(&self.is_connected),
        }
    }
}

/// Global broker client instance - initialized based on environment
static BROKER_CLIENT: once_cell::sync::Lazy<Arc<BacktestBrokerClient>> =
    once_cell::sync::Lazy::new(|| Arc::new(BacktestBrokerClient::from_env()));

/// Get the global broker client
pub fn get_broker_client() -> Arc<BacktestBrokerClient> {
    Arc::clone(&BROKER_CLIENT)
}

/// Initialize and connect the global broker client (call once at startup)
/// Returns the broker client for use by callers - they must call start_receiver with appropriate role
pub async fn init_broker_client() -> Result<Arc<BacktestBrokerClient>, BrokerError> {
    let client = get_broker_client();
    client.connect().await?;
    Ok(client)
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
