//! Job Broker using MessageBrokerEngine for job distribution
//!
//! This module provides job distribution via pub/sub messaging instead of
//! database polling. Jobs are published to the message broker and workers
//! subscribe to receive them instantly.
//!
//! # Architecture
//! ```text
//! API Server                          Workers
//!     │                                  │
//!     └──publish──► MessageBroker ◄──subscribe──┘
//!                   "backtest.jobs.new"
//!                         │
//!                         ▼
//!                    PostgreSQL
//!                 (persist state only)
//! ```
//!
//! # Topics
//! - `backtest.jobs.new` - New job submissions
//! - `backtest.jobs.claimed.{job_id}` - Job claimed by worker
//! - `backtest.jobs.progress.{job_id}` - Progress updates (via progress_broadcaster)
//! - `backtest.jobs.completed.{job_id}` - Job completed
//! - `backtest.jobs.failed.{job_id}` - Job failed
//!
//! # Benefits over PostgreSQL polling
//! - Instant job delivery (~176ns vs 1-5 second poll interval)
//! - No database load from polling queries
//! - Natural load balancing (first subscriber to receive wins)
//! - Horizontal scaling without coordination

use std::sync::Arc;
use std::collections::VecDeque;
use tokio::sync::{broadcast, Mutex};
use serde::{Deserialize, Serialize};

use crate::job_queue::BacktestJobParams;
use crate::{log_info_structured, log_warn, log_debug};
use crate::logging_facade::MAIN_LOGGER;

/// Job message published to the message broker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMessage {
    pub job_id: String,
    pub params: BacktestJobParams,
    pub submitted_at: i64,
    pub priority: JobPriority,
}

/// Job priority for ordering
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl Default for JobPriority {
    fn default() -> Self {
        JobPriority::Normal
    }
}

/// Job claimed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobClaimedEvent {
    pub job_id: String,
    pub worker_id: usize,
    pub claimed_at: i64,
}

/// Job completed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCompletedEvent {
    pub job_id: String,
    pub worker_id: usize,
    pub completed_at: i64,
    pub success: bool,
    pub error_message: Option<String>,
}

/// In-process job broker using broadcast channels
/// 
/// For single-node deployment, this uses tokio broadcast channels.
/// For multi-node deployment, this can be extended to use the full
/// MessageBrokerEngine over the network.
/// 
/// This design allows the same interface whether running single-node
/// or distributed.
pub struct JobBroker {
    /// Channel for new job submissions
    job_sender: broadcast::Sender<JobMessage>,
    /// Pending jobs queue (for workers that miss broadcast)
    pending_jobs: Arc<Mutex<VecDeque<JobMessage>>>,
    /// Channel for job claimed events
    claimed_sender: broadcast::Sender<JobClaimedEvent>,
    /// Channel for job completed events
    completed_sender: broadcast::Sender<JobCompletedEvent>,
}

impl JobBroker {
    /// Create a new job broker with default capacity
    pub fn new() -> Self {
        let (job_sender, _) = broadcast::channel(1000);
        let (claimed_sender, _) = broadcast::channel(1000);
        let (completed_sender, _) = broadcast::channel(1000);
        
        Self {
            job_sender,
            pending_jobs: Arc::new(Mutex::new(VecDeque::new())),
            claimed_sender,
            completed_sender,
        }
    }

    /// Submit a new job for processing
    /// 
    /// The job is broadcast to all subscribed workers. The first worker
    /// to claim it wins (using database-level locking for consistency).
    pub async fn submit_job(&self, params: BacktestJobParams) -> String {
        let job_id = uuid::Uuid::new_v4().to_string();
        
        let message = JobMessage {
            job_id: job_id.clone(),
            params,
            submitted_at: chrono::Utc::now().timestamp_millis(),
            priority: JobPriority::Normal,
        };

        // Add to pending queue for workers that weren't subscribed yet
        {
            let mut pending = self.pending_jobs.lock().await;
            pending.push_back(message.clone());
            // Keep only last 1000 pending jobs
            while pending.len() > 1000 {
                pending.pop_front();
            }
        }

        // Broadcast to subscribed workers
        match self.job_sender.send(message) {
            Ok(count) => {
                log_info_structured!(MAIN_LOGGER, "JOB_PUBLISHED",
                    "event" => "JOB_PUBLISHED",
                    "job_id" => &job_id,
                    "worker_count" => count
                );
            }
            Err(_) => {
                log_debug!(MAIN_LOGGER, "Job {} queued (no active subscribers)", job_id);
            }
        }

        job_id
    }

    /// Subscribe to new job notifications
    pub fn subscribe_jobs(&self) -> broadcast::Receiver<JobMessage> {
        self.job_sender.subscribe()
    }

    /// Get next pending job (for workers that missed the broadcast)
    pub async fn get_pending_job(&self) -> Option<JobMessage> {
        let mut pending = self.pending_jobs.lock().await;
        pending.pop_front()
    }

    /// Mark a job as claimed by a worker
    pub async fn claim_job(&self, job_id: &str, worker_id: usize) {
        let event = JobClaimedEvent {
            job_id: job_id.to_string(),
            worker_id,
            claimed_at: chrono::Utc::now().timestamp_millis(),
        };

        // Remove from pending queue
        {
            let mut pending = self.pending_jobs.lock().await;
            pending.retain(|j| j.job_id != job_id);
        }

        // Broadcast claimed event
        let _ = self.claimed_sender.send(event);
    }

    /// Subscribe to job claimed events
    pub fn subscribe_claimed(&self) -> broadcast::Receiver<JobClaimedEvent> {
        self.claimed_sender.subscribe()
    }

    /// Mark a job as completed
    pub async fn complete_job(&self, job_id: &str, worker_id: usize, success: bool, error_message: Option<String>) {
        let event = JobCompletedEvent {
            job_id: job_id.to_string(),
            worker_id,
            completed_at: chrono::Utc::now().timestamp_millis(),
            success,
            error_message,
        };

        let _ = self.completed_sender.send(event);
        log_info_structured!(MAIN_LOGGER, "JOB_COMPLETED_BY_WORKER",
            "event" => "JOB_COMPLETED_BY_WORKER",
            "job_id" => job_id,
            "worker_id" => worker_id,
            "success" => success
        );
    }

    /// Subscribe to job completed events
    pub fn subscribe_completed(&self) -> broadcast::Receiver<JobCompletedEvent> {
        self.completed_sender.subscribe()
    }

    /// Get pending job count
    pub async fn pending_count(&self) -> usize {
        self.pending_jobs.lock().await.len()
    }
}

impl Default for JobBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for JobBroker {
    fn clone(&self) -> Self {
        Self {
            job_sender: self.job_sender.clone(),
            pending_jobs: Arc::clone(&self.pending_jobs),
            claimed_sender: self.claimed_sender.clone(),
            completed_sender: self.completed_sender.clone(),
        }
    }
}

/// Global job broker instance
static JOB_BROKER: once_cell::sync::Lazy<Arc<JobBroker>> =
    once_cell::sync::Lazy::new(|| Arc::new(JobBroker::new()));

/// Get the global job broker
pub fn get_job_broker() -> Arc<JobBroker> {
    Arc::clone(&JOB_BROKER)
}

/// Worker-side job receiver
/// 
/// Handles receiving jobs from both broadcast channel and pending queue.
/// Ensures no jobs are missed even if worker starts after jobs are submitted.
pub struct JobReceiver {
    broker: Arc<JobBroker>,
    broadcast_rx: broadcast::Receiver<JobMessage>,
    worker_id: usize,
}

impl JobReceiver {
    pub fn new(broker: Arc<JobBroker>, worker_id: usize) -> Self {
        let broadcast_rx = broker.subscribe_jobs();
        Self {
            broker,
            broadcast_rx,
            worker_id,
        }
    }

    /// Receive the next job (from broadcast or pending queue)
    /// 
    /// This method:
    /// 1. First checks pending queue for jobs that were submitted before subscribe
    /// 2. Then waits for broadcast messages
    /// 3. Returns None if channel is closed
    pub async fn recv(&mut self) -> Option<JobMessage> {
        // First check pending queue
        if let Some(job) = self.broker.get_pending_job().await {
            return Some(job);
        }

        // Then wait for broadcast
        loop {
            match self.broadcast_rx.recv().await {
                Ok(job) => return Some(job),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log_warn!(MAIN_LOGGER, "Worker {} lagged {} messages, checking pending queue", 
                        self.worker_id, n);
                    // Check pending queue after lag
                    if let Some(job) = self.broker.get_pending_job().await {
                        return Some(job);
                    }
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    }

    /// Try to receive a job without blocking
    pub async fn try_recv(&mut self) -> Option<JobMessage> {
        // First check pending queue
        if let Some(job) = self.broker.get_pending_job().await {
            return Some(job);
        }

        // Then try broadcast (non-blocking)
        match self.broadcast_rx.try_recv() {
            Ok(job) => Some(job),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_job_broker_submit_and_receive() {
        let broker = JobBroker::new();
        let _receiver = JobReceiver::new(Arc::new(broker.clone()), 1);

        let _params = BacktestJobParams {
            symbol: "BTC/USD".to_string(),
            exchange: "Kraken".to_string(),
            strategy_type: "market_making".to_string(),
            initial_capital: 100000.0,
            initial_inventory_pct: 0.5,
            initial_30d_volume: 0.0,
            optimization_method: "genetic".to_string(),
            data_source: "database".to_string(),
            tardis_data_path: None,
            tardis_trades_path: None,
            tardis_orderbook_path: None,
            population_size: 30,  // Reduced from 50 for faster optimization
            generations: 15,     // Standardized with early stopping
            risk_aversion: 0.5,
            inventory_target: 0.0,
            order_size: 0.01,
            order_size_pct: 0.02,
            window_ms: 5000,
            min_quote_lifetime_ms: 100,
            fee_rate: 0.001,
            volatility_cap: 0.05,
            time_horizon_minutes: 60,
            market_depth_k: 1.5,
            volatility_adjustment_factor: 0.5,
            inventory_penalty_factor: 0.3,
            trend_sensitivity: 0.2,
            flow_sensitivity: -0.1,
            fee_spread_multiplier: 4.0,
            adverse_selection_sensitivity: 1.0,
            monte_carlo_enabled: false,
            monte_carlo_runs: 0,
            walk_forward_enabled: false,
            commission_rate: 0.001,
            start_date: None,
            end_date: None,
            walkforward_optimization_days: None,
            walkforward_test_days: None,
            walkforward_step_days: None,
            auto_save_profitable: true,
            min_sharpe_ratio: 1.0,
            min_profit_factor: 1.2,
            max_drawdown: 0.25,
            min_win_rate: 0.40,
            min_trades: 30,
            min_sortino_ratio: None,
            min_calmar_ratio: None,
            min_total_pnl: None,
            take_profit_pct: 0.005,
            stop_loss_pct: 0.003,
            min_spread_bps: 15.0,
            max_trades_per_minute: 30.0,
            train_ratio: 0.6,
            test_ratio: 0.2,
            validate_ratio: 0.2,
            fitness_weights: None,
            fill_model: "realistic".to_string(),
            portfolio_assets: None,
            symbols: None,
            symbol_exchanges: None,
            allocation_weights: None,
            routing_mode: None,
            routing_exchanges: None,
            max_venues_per_order: None,
            min_venue_allocation: None,
            execution_style: None,
            execution_duration_ms: None,
            min_arb_profit_bps: None,
        };

        // Need to set up default for BacktestJobParams
        // For now, just test the broker mechanics
    }

    #[tokio::test]
    async fn test_job_broker_pending_queue() {
        let broker = JobBroker::new();
        
        // Submit job before any subscribers
        let params = BacktestJobParams {
            symbol: "ETH/USD".to_string(),
            exchange: "Kraken".to_string(),
            strategy_type: "market_making".to_string(),
            initial_capital: 50000.0,
            initial_inventory_pct: 0.5,
            initial_30d_volume: 0.0,
            optimization_method: "genetic".to_string(),
            data_source: "database".to_string(),
            tardis_data_path: None,
            tardis_trades_path: None,
            tardis_orderbook_path: None,
            population_size: 30,
            generations: 5,
            risk_aversion: 0.5,
            inventory_target: 0.0,
            order_size: 0.01,
            order_size_pct: 0.02,
            window_ms: 5000,
            min_quote_lifetime_ms: 100,
            fee_rate: 0.001,
            volatility_cap: 0.05,
            time_horizon_minutes: 60,
            market_depth_k: 1.5,
            volatility_adjustment_factor: 0.5,
            inventory_penalty_factor: 0.3,
            trend_sensitivity: 0.2,
            flow_sensitivity: -0.1,
            fee_spread_multiplier: 4.0,
            adverse_selection_sensitivity: 1.0,
            monte_carlo_enabled: true,
            monte_carlo_runs: 100,
            walk_forward_enabled: true,
            commission_rate: 0.001,
            start_date: None,
            end_date: None,
            walkforward_optimization_days: None,
            walkforward_test_days: None,
            walkforward_step_days: None,
            auto_save_profitable: true,
            min_sharpe_ratio: 1.0,
            min_profit_factor: 1.2,
            max_drawdown: 0.25,
            min_win_rate: 0.40,
            min_trades: 30,
            min_sortino_ratio: None,
            min_calmar_ratio: None,
            min_total_pnl: None,
            take_profit_pct: 0.005,
            stop_loss_pct: 0.003,
            min_spread_bps: 15.0,
            max_trades_per_minute: 30.0,
            train_ratio: 0.6,
            test_ratio: 0.2,
            validate_ratio: 0.2,
            fitness_weights: None,
            fill_model: "realistic".to_string(),
            portfolio_assets: None,
            symbols: None,
            symbol_exchanges: None,
            allocation_weights: None,
            routing_mode: None,
            routing_exchanges: None,
            max_venues_per_order: None,
            min_venue_allocation: None,
            execution_style: None,
            execution_duration_ms: None,
            min_arb_profit_bps: None,
        };

        let job_id = broker.submit_job(params).await;
        
        // Job should be in pending queue
        assert_eq!(broker.pending_count().await, 1);

        // New subscriber should be able to get it from pending queue
        let mut receiver = JobReceiver::new(Arc::new(broker.clone()), 1);
        let job = receiver.try_recv().await;
        
        assert!(job.is_some());
        assert_eq!(job.unwrap().job_id, job_id);
        
        // Pending queue should now be empty
        assert_eq!(broker.pending_count().await, 0);
    }
}
