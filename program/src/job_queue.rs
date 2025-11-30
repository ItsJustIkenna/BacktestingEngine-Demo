//! Job queue system for async backtest processing
//! 
//! Supports PostgreSQL backend
//! Workers poll the queue and execute backtests asynchronously.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Job status states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

/// Backtest job parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobParams {
    pub symbol: String,
    pub exchange: String,
    pub strategy_type: String,
    pub initial_capital: f64,
    
    // Optimization method
    #[serde(default = "default_optimization_method")]
    pub optimization_method: String, // "genetic" or "manual"
    
    // Genetic algorithm parameters
    #[serde(default = "default_population_size")]
    pub population_size: usize,
    #[serde(default = "default_generations")]
    pub generations: usize,
    
    // Manual strategy parameters (used when optimization_method == "manual")
    #[serde(default = "default_risk_aversion")]
    pub risk_aversion: f64,
    #[serde(default = "default_inventory_target")]
    pub inventory_target: f64,
    #[serde(default = "default_order_size")]
    pub order_size: f64,
    #[serde(default = "default_window_ms")]
    pub window_ms: u64,
    #[serde(default = "default_min_quote_lifetime")]
    pub min_quote_lifetime_ms: u64,
    #[serde(default)]
    pub fee_rate: f64, // Will be auto-populated from exchange
    #[serde(default = "default_latency_buffer")]
    pub latency_buffer: u64,
    #[serde(default = "default_volatility_cap")]
    pub volatility_cap: f64,
    #[serde(default = "default_time_horizon")]
    pub time_horizon_minutes: u64,
    #[serde(default = "default_market_depth_k")]
    pub market_depth_k: f64,
    
    // Phase 2 adjustment factors
    #[serde(default = "default_volatility_adjustment")]
    pub volatility_adjustment_factor: f64,
    #[serde(default = "default_inventory_penalty")]
    pub inventory_penalty_factor: f64,
    #[serde(default = "default_trend_sensitivity")]
    pub trend_sensitivity: f64,
    #[serde(default = "default_flow_sensitivity")]
    pub flow_sensitivity: f64,
    
    // Auto-populated fields
    #[serde(default)]
    pub commission_rate: f64, // Auto-populated from exchange
    #[serde(default)]
    pub start_date: Option<String>, // Auto-populated from database
    #[serde(default)]
    pub end_date: Option<String>, // Auto-populated from database
}

// Default functions for serde
fn default_optimization_method() -> String { "genetic".to_string() }
fn default_population_size() -> usize { 50 }
fn default_generations() -> usize { 20 }
fn default_risk_aversion() -> f64 { 0.5 }
fn default_inventory_target() -> f64 { 0.5 }
fn default_order_size() -> f64 { 0.01 }
fn default_window_ms() -> u64 { 5000 }
fn default_min_quote_lifetime() -> u64 { 100 }
fn default_latency_buffer() -> u64 { 10 }
fn default_volatility_cap() -> f64 { 0.05 }
fn default_time_horizon() -> u64 { 60 }
fn default_market_depth_k() -> f64 { 1.5 }
fn default_volatility_adjustment() -> f64 { 0.5 }
fn default_inventory_penalty() -> f64 { 0.3 }
fn default_trend_sensitivity() -> f64 { 0.2 }
fn default_flow_sensitivity() -> f64 { -0.1 }

/// Backtest job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: String,
    pub params: BacktestJobParams,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: f64, // 0.0 to 100.0
    pub current_generation: i32, // Current generation for genetic optimization
    pub total_generations: i32, // Total generations for genetic optimization
    pub error_message: Option<String>,
}

/// Backtest results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub job_id: String,
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub total_trades: usize,
    pub profit_factor: f64,
    pub equity_curve: Vec<f64>,
    pub completed_at: DateTime<Utc>,
}

/// Job queue interface
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Submit a new backtest job
    async fn submit_job(&self, params: BacktestJobParams) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get next job from queue
    async fn get_next_job(&self) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get job by ID
    async fn get_job(&self, job_id: &str) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job status
    async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job progress
    async fn update_progress(&self, job_id: &str, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update generation progress for genetic optimization
    async fn update_generation(&self, job_id: &str, current_generation: i32, total_generations: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job heartbeat timestamp
    async fn update_heartbeat(&self, job_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Store backtest results
    async fn store_result(&self, result: &BacktestResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get backtest results
    async fn get_result(&self, job_id: &str) -> Result<Option<BacktestResult>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get queue length
    async fn queue_length(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get database connection pool (if available)
    fn get_pool(&self) -> Option<&diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::pg::PgConnection>>> {
        None
    }
    
    /// Downcast to Any for type-specific features
    fn as_any(&self) -> &dyn std::any::Any;
}

// Re-export implementations
pub mod postgres;

pub use postgres::PostgresQueue;
