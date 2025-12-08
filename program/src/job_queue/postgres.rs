//! PostgreSQL-backed job queue implementation with async support
//!
//! # Refactored to use databaseschema
//!
//! This module now imports schema and models from the shared `databaseschema` crate
//! instead of duplicating definitions. This ensures:
//! - Single source of truth for database schema
//! - Consistent model definitions across all engines
//! - Easier maintenance when schema changes
//!
//! # Thread Safety & Deadlock Prevention
//!
//! This module is designed to prevent deadlocks and data races through several mechanisms:
//!
//! ## Connection Pool Management
//! - **Pool Size**: 30 connections to handle concurrent genetic optimization + API + workers
//! - **Timeout Protection**: All connection acquisitions have 10-second timeout
//! - **Logging**: Timeout events logged with `[DEADLOCK WARNING]` prefix for debugging
//!
//! ## Transaction Patterns
//! - **FOR UPDATE SKIP LOCKED**: Used for job claiming to prevent blocking on locked rows
//! - **Single Connection Per Operation**: Each method acquires/releases its own connection
//! - **No Nested Transactions**: Avoids deadlocks from nested lock acquisition
//!
//! ## Lock Ordering Convention
//! When multiple tables are accessed in a single operation, always access in this order:
//! 1. `backtest_jobs` (primary job table)
//! 2. `backtest_results` (result storage)
//!
//! ## Async/Sync Boundaries
//! - All public methods are `async` and use tokio runtime
//! - Database operations use `diesel_async` for non-blocking I/O
//! - CPU-intensive work should use `tokio::task::spawn_blocking`

use super::*;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, AsyncConnection, RunQueryDsl};
use diesel_async::pooled_connection::{AsyncDieselConnectionManager, deadpool};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

// Import schema from databaseschema crate - single source of truth
use databaseschema::schema::{backtest_jobs, backtest_results};

/// Default timeout for acquiring database connections (10 seconds)
/// This prevents indefinite blocking if the pool is exhausted
const CONNECTION_TIMEOUT_SECS: u64 = 10;

/// Timeout for long-running transactions (5 minutes)
/// This prevents transactions from holding connections indefinitely
#[allow(dead_code)]
const TRANSACTION_TIMEOUT_SECS: u64 = 300;

// Re-export NewBacktestResult from databaseschema for result storage
pub use databaseschema::models::backtest_result::NewBacktestResult;

/// Queryable record for backtest_jobs table
/// 
/// Note: We define this locally rather than using databaseschema because
/// the field order must match the table! macro column order exactly.
#[derive(Queryable, Selectable, Clone, Debug)]
#[diesel(table_name = backtest_jobs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BacktestJobRecord {
    pub id: uuid::Uuid,
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub risk_aversion: bigdecimal::BigDecimal,
    pub inventory_target: bigdecimal::BigDecimal,
    pub order_size: bigdecimal::BigDecimal,
    pub initial_capital: bigdecimal::BigDecimal,
    pub commission_rate: bigdecimal::BigDecimal,
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
    pub status: String,
    pub progress: bigdecimal::BigDecimal,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error_message: Option<String>,
    pub result_id: Option<uuid::Uuid>,
    pub strategy_type: String,
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
    pub optimization_method: String,
    pub population_size: Option<i32>,
    pub generations: Option<i32>,
    pub current_phase: Option<String>,
    pub phase_details: Option<serde_json::Value>,
}

/// Insertable record for new backtest jobs
#[derive(Insertable)]
#[diesel(table_name = backtest_jobs)]
pub struct NewBacktestJob {
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub strategy_type: String,
    pub optimization_method: String,
    pub population_size: Option<i32>,
    pub generations: Option<i32>,
    pub risk_aversion: bigdecimal::BigDecimal,
    pub inventory_target: bigdecimal::BigDecimal,
    pub order_size: bigdecimal::BigDecimal,
    pub initial_capital: bigdecimal::BigDecimal,
    pub commission_rate: bigdecimal::BigDecimal,
    pub start_date: Option<chrono::DateTime<chrono::Utc>>,
    pub end_date: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct PostgresQueue {
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
}

impl PostgresQueue {
    /// Create a new PostgreSQL-backed job queue with async connection pool
    /// 
    /// # Pool Configuration
    /// - max_size: 30 connections to handle concurrent genetic optimization + API requests
    /// - Timeout: 10 seconds to prevent indefinite blocking on pool exhaustion
    pub fn new(database_url: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
        let pool = deadpool::Pool::builder(config)
            .max_size(30) // Increased for concurrent genetic optimization + API + workers
            .build()?;
        
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
    
    /// Acquire a connection from the pool with timeout to prevent deadlocks
    /// 
    /// # Deadlock Prevention
    /// - Timeout prevents indefinite waiting if pool is exhausted
    /// - Returns descriptive error for debugging
    async fn get_conn_with_timeout(&self) -> Result<deadpool::Object<AsyncPgConnection>, Box<dyn std::error::Error + Send + Sync>> {
        match timeout(Duration::from_secs(CONNECTION_TIMEOUT_SECS), self.pool.get()).await {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(e)) => {
                log_warn!(crate::MAIN_LOGGER, "DEADLOCK WARNING: Pool error - possible pool exhaustion: {}", e);
                Err(format!("Database pool error: {}", e).into())
            }
            Err(_) => {
                log_warn!(crate::MAIN_LOGGER, "DEADLOCK WARNING: Connection acquisition timeout after {}s - pool may be exhausted or deadlocked", CONNECTION_TIMEOUT_SECS);
                Err(format!("Connection pool timeout after {}s - possible deadlock or pool exhaustion", CONNECTION_TIMEOUT_SECS).into())
            }
        }
    }
    
    /// Downsample equity curve to max_points using largest-triangle-three-buckets algorithm
    fn downsample_equity_curve(curve: &[f64], max_points: usize) -> Vec<f64> {
        if curve.len() <= max_points {
            return curve.to_vec();
        }
        
        let mut result = Vec::with_capacity(max_points);
        let bucket_size = (curve.len() - 2) as f64 / (max_points - 2) as f64;
        
        // Always keep first point
        result.push(curve[0]);
        
        let mut a = 0;
        for i in 0..(max_points - 2) {
            let avg_range_start = ((i + 1) as f64 * bucket_size).floor() as usize + 1;
            let avg_range_end = ((i + 2) as f64 * bucket_size).floor() as usize + 1;
            let avg_range_end = avg_range_end.min(curve.len());
            
            let avg_x = (avg_range_start + avg_range_end) as f64 / 2.0;
            let avg_y: f64 = curve[avg_range_start..avg_range_end].iter().sum::<f64>() 
                / (avg_range_end - avg_range_start) as f64;
            
            let range_start = (a + 1).max(1);
            let range_end = ((i + 1) as f64 * bucket_size).floor() as usize + 1;
            let range_end = range_end.min(curve.len());
            
            let mut max_area = -1.0;
            let mut max_area_point = range_start;
            
            for idx in range_start..range_end {
                let area = ((idx as f64 - a as f64) * (avg_y - curve[a]) - (avg_x - a as f64) * (curve[idx] - curve[a])).abs();
                if area > max_area {
                    max_area = area;
                    max_area_point = idx;
                }
            }
            
            result.push(curve[max_area_point]);
            a = max_area_point;
        }
        
        // Always keep last point
        result.push(curve[curve.len() - 1]);
        
        result
    }
    
    /// Get the async connection pool (for data loading)
    pub fn get_async_pool(&self) -> Option<Arc<deadpool::Pool<AsyncPgConnection>>> {
        Some(Arc::clone(&self.pool))
    }
    
    /// Legacy sync pool getter - no longer used
    pub fn get_pool(&self) -> Option<Arc<deadpool::Pool<AsyncPgConnection>>> {
        Some(Arc::clone(&self.pool))
    }
    
    /// Convert database record to BacktestJob
    fn record_to_job(record: BacktestJobRecord) -> Result<BacktestJob, Box<dyn std::error::Error + Send + Sync>> {
        use std::str::FromStr;
        
        let status = match record.status.as_str() {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "completed" => JobStatus::Completed,
            "failed" => JobStatus::Failed,
            _ => JobStatus::Queued,
        };
        
        Ok(BacktestJob {
            id: record.job_id,
            params: BacktestJobParams {
                symbol: record.symbol,
                exchange: record.exchange,
                strategy_type: record.strategy_type,
                initial_capital: f64::from_str(&record.initial_capital.to_string())?,
                optimization_method: record.optimization_method,
                population_size: record.population_size.unwrap_or(50) as usize,
                generations: record.generations.unwrap_or(20) as usize,
                // Walk-forward params - default to None when loading from DB
                walkforward_optimization_days: None,
                walkforward_test_days: None,
                walkforward_step_days: None,
                // Monte Carlo and Walk-Forward settings - default to enabled
                monte_carlo_enabled: true,
                monte_carlo_runs: 1000,
                walk_forward_enabled: true,
                // Profitability criteria - use defaults when loading from DB
                auto_save_profitable: true,
                min_sharpe_ratio: 1.0,
                min_profit_factor: 1.2,
                max_drawdown: 0.25,
                min_win_rate: 0.40,
                min_trades: 30,
                min_sortino_ratio: None,
                min_calmar_ratio: None,
                min_total_pnl: None,
                risk_aversion: f64::from_str(&record.risk_aversion.to_string())?,
                inventory_target: f64::from_str(&record.inventory_target.to_string())?,
                order_size: f64::from_str(&record.order_size.to_string())?,
                window_ms: 5000,
                min_quote_lifetime_ms: 100,
                fee_rate: f64::from_str(&record.commission_rate.to_string())?,
                latency_buffer: 10,
                volatility_cap: 0.05,
                time_horizon_minutes: 60,
                market_depth_k: 1.5,
                volatility_adjustment_factor: 0.5,
                inventory_penalty_factor: 0.3,
                trend_sensitivity: 0.2,
                flow_sensitivity: -0.1,
                take_profit_pct: 0.005,
                stop_loss_pct: 0.003,
                min_spread_bps: 15.0,
                max_trades_per_minute: 30.0,
                commission_rate: f64::from_str(&record.commission_rate.to_string())?,
                start_date: record.start_date.map(|dt| dt.to_rfc3339()),
                end_date: record.end_date.map(|dt| dt.to_rfc3339()),
            },
            status,
            created_at: record.created_at,
            started_at: record.started_at,
            completed_at: record.completed_at,
            progress: f64::from_str(&record.progress.to_string())?,
            current_generation: record.current_generation.unwrap_or(0),
            total_generations: record.total_generations.unwrap_or(0),
            current_phase: record.current_phase.unwrap_or_else(|| "initializing".to_string()),
            phase_details: record.phase_details.unwrap_or_else(|| serde_json::json!({})),
            error_message: record.error_message,
        })
    }
}

#[async_trait]
impl JobQueue for PostgresQueue {
    async fn submit_job(&self, params: BacktestJobParams) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        let job_id = Uuid::new_v4().to_string();
        
        let mut conn = self.get_conn_with_timeout().await?;
        
        let new_job = NewBacktestJob {
            job_id: job_id.clone(),
            symbol: params.symbol,
            exchange: params.exchange,
            strategy_type: params.strategy_type,
            optimization_method: params.optimization_method,
            population_size: if params.population_size > 0 { Some(params.population_size as i32) } else { None },
            generations: if params.generations > 0 { Some(params.generations as i32) } else { None },
            risk_aversion: BigDecimal::from_str(&params.risk_aversion.to_string())?,
            inventory_target: BigDecimal::from_str(&params.inventory_target.to_string())?,
            order_size: BigDecimal::from_str(&params.order_size.to_string())?,
            initial_capital: BigDecimal::from_str(&params.initial_capital.to_string())?,
            commission_rate: BigDecimal::from_str(&params.commission_rate.to_string())?,
            start_date: params.start_date.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
            end_date: params.end_date.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
        };
        
        diesel::insert_into(backtest_jobs::table)
            .values(&new_job)
            .execute(&mut conn)
            .await?;
        
        Ok(job_id)
    }
    
    async fn get_next_job(&self) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        // Use FOR UPDATE SKIP LOCKED for atomic job claiming
        let result = conn.transaction::<_, Box<dyn std::error::Error + Send + Sync>, _>(|conn| {
            Box::pin(async move {
                // Calculate thresholds for orphaned job detection
                // Use conservative timeouts to avoid recovering jobs that are still processing
                // Heartbeat interval is 30s, so 10 minutes means ~20 missed heartbeats
                let started_threshold = Utc::now() - chrono::Duration::minutes(30);
                let heartbeat_threshold = Utc::now() - chrono::Duration::minutes(10);
                
                // Find queued job or recover orphaned running jobs
                let job: Option<BacktestJobRecord> = backtest_jobs::table
                    .filter(
                        backtest_jobs::status.eq("queued")
                            .or(
                                backtest_jobs::status.eq("running")
                                    .and(
                                        backtest_jobs::last_heartbeat.lt(heartbeat_threshold)
                                            .or(
                                                backtest_jobs::last_heartbeat.is_null()
                                                    .and(backtest_jobs::started_at.lt(started_threshold))
                                            )
                                    )
                            )
                    )
                    .order(backtest_jobs::created_at.asc())
                    .for_update()
                    .skip_locked()
                    .select(BacktestJobRecord::as_select())
                    .first(conn)
                    .await
                    .optional()?;
                
                if let Some(job_record) = job {
                    let now = Utc::now();
                    diesel::update(backtest_jobs::table.find(job_record.id))
                        .set((
                            backtest_jobs::status.eq("running"),
                            backtest_jobs::started_at.eq(Some(now)),
                            backtest_jobs::last_heartbeat.eq(Some(now)),
                        ))
                        .execute(conn)
                        .await?;
                    
                    // Create updated record with started_at set
                    let mut updated_record = job_record;
                    updated_record.status = "running".to_string();
                    updated_record.started_at = Some(now);
                    
                    Ok(Some(Self::record_to_job(updated_record)?))
                } else {
                    Ok(None)
                }
            })
        }).await?;
        
        Ok(result)
    }
    
    async fn get_job(&self, job_id: &str) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        let record: Option<BacktestJobRecord> = backtest_jobs::table
            .filter(backtest_jobs::job_id.eq(job_id))
            .select(BacktestJobRecord::as_select())
            .first(&mut conn)
            .await
            .optional()?;
        
        match record {
            Some(r) => Ok(Some(Self::record_to_job(r)?)),
            None => Ok(None),
        }
    }
    
    async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        let status_str = match status {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
        };
        
        diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(job_id)))
            .set((
                backtest_jobs::status.eq(status_str),
                backtest_jobs::error_message.eq(error_message),
            ))
            .execute(&mut conn)
            .await?;
        
        Ok(())
    }
    
    async fn update_progress(&self, job_id: &str, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        log_info!(crate::MAIN_LOGGER, "Job {} progress: {:.1}%", job_id, progress);
        
        let mut conn = self.get_conn_with_timeout().await?;
        
        let rows = diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(job_id)))
            .set(backtest_jobs::progress.eq(BigDecimal::from_str(&progress.to_string())?))
            .execute(&mut conn)
            .await?;
        
        if rows == 0 {
            log_warn!(crate::MAIN_LOGGER, "Job {} progress update affected 0 rows", job_id);
        }
        
        Ok(())
    }
    
    /// Update generation progress for genetic optimization
    async fn update_generation(&self, job_id: &str, current_generation: i32, total_generations: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        log_info!(crate::MAIN_LOGGER, "Job {} generation {}/{}", job_id, current_generation, total_generations);
        
        let mut conn = self.get_conn_with_timeout().await?;
        
        diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(job_id)))
            .set((
                backtest_jobs::current_generation.eq(current_generation),
                backtest_jobs::total_generations.eq(total_generations),
            ))
            .execute(&mut conn)
            .await?;
        
        Ok(())
    }
    
    /// Update current phase of job execution
    async fn update_phase(&self, job_id: &str, phase: &str, phase_details: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        log_info!(crate::MAIN_LOGGER, "Job {} phase: '{}'", job_id, phase);
        
        let phase_details_str = serde_json::to_string(&phase_details)?;
        let mut conn = self.get_conn_with_timeout().await?;
        
        // Use raw SQL to update JSONB field
        diesel::sql_query(
            "UPDATE backtest_jobs SET current_phase = $1, phase_details = $2::jsonb WHERE job_id = $3"
        )
        .bind::<diesel::sql_types::Text, _>(phase)
        .bind::<diesel::sql_types::Text, _>(&phase_details_str)
        .bind::<diesel::sql_types::Text, _>(job_id)
        .execute(&mut conn)
        .await?;
        
        Ok(())
    }
    
    /// Update job heartbeat timestamp
    /// Called every 30 seconds by worker to signal it's still alive
    async fn update_heartbeat(&self, job_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(job_id)))
            .set(backtest_jobs::last_heartbeat.eq(Utc::now()))
            .execute(&mut conn)
            .await?;
        
        Ok(())
    }
    
    async fn store_result(&self, result: &BacktestResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        let start = std::time::Instant::now();
        
        let mut conn = self.get_conn_with_timeout().await?;
        
        // Get job to find dates and parameters
        let job: BacktestJobRecord = backtest_jobs::table
            .filter(backtest_jobs::job_id.eq(&result.job_id))
            .select(BacktestJobRecord::as_select())
            .first(&mut conn)
            .await?;
        
        // Create backtest result record
        let backtest_uuid = uuid::Uuid::parse_str(&result.job_id)?;
        
        // Cap numeric values to prevent overflow in database
        // DECIMAL(20,8) = max 12 digits before decimal = 999,999,999,999
        // DECIMAL(10,8) = max 2 digits before decimal = 99.99999999
        // DECIMAL(10,6) = max 4 digits before decimal = 9999.999999
        // DECIMAL(10,2) = max 8 digits before decimal = 99999999.99
        const MAX_VALUE_20_8: f64 = 999_999_999_999.0; // For DECIMAL(20,8)
        const MAX_VALUE_10_8: f64 = 99.0;              // For DECIMAL(10,8) - slippage, rates
        const MAX_VALUE_10_6: f64 = 9999.0;            // For DECIMAL(10,6) - beta, correlations
        const MAX_VALUE_10_2: f64 = 99_999_999.0;      // For DECIMAL(10,2) - time in trade
        
        // Helper to sanitize f64 values (handle NaN, Inf, and overflow)
        let sanitize = |v: f64, max_val: f64| -> f64 {
            if v.is_nan() || v.is_infinite() {
                0.0
            } else {
                v.clamp(-max_val, max_val)
            }
        };
        
        // Helper to cap f64 values for DECIMAL(20,8) and convert to BigDecimal
        let cap_value = |v: f64| -> BigDecimal {
            let safe_val = sanitize(v, MAX_VALUE_20_8);
            BigDecimal::from_str(&safe_val.to_string()).unwrap_or(BigDecimal::from(0))
        };
        
        // Helper for DECIMAL(10,8) fields (slippage, rates)
        let cap_small = |v: f64| -> BigDecimal {
            let safe_val = sanitize(v, MAX_VALUE_10_8);
            BigDecimal::from_str(&safe_val.to_string()).unwrap_or(BigDecimal::from(0))
        };
        
        // Helper for optional DECIMAL(10,8) fields
        let cap_small_opt = |v: Option<f64>| -> Option<BigDecimal> {
            v.map(|x| cap_small(x))
        };
        
        // Helper to cap optional f64 values for DECIMAL(20,8)
        let cap_opt = |v: Option<f64>| -> Option<BigDecimal> {
            v.map(|x| cap_value(x))
        };
        
        // Helper for DECIMAL(10,2) fields (time in trade)
        let cap_time = |v: f64| -> BigDecimal {
            let safe_val = sanitize(v, MAX_VALUE_10_2);
            BigDecimal::from_str(&safe_val.to_string()).unwrap_or(BigDecimal::from(0))
        };
        
        let capped_pnl = sanitize(result.total_pnl, MAX_VALUE_20_8);
        
        // Calculate annualized metrics (simplified - using 252 trading days)
        let total_return_bd = cap_value(capped_pnl);
        let annualized_return = total_return_bd.clone(); // Simplified
        
        // Downsample equity curve to 500 points max (reduced from 1000 for faster serialization)
        let equity_curve_downsampled = Self::downsample_equity_curve(&result.equity_curve, 500);
        
        let now = chrono::Utc::now();
        let new_result = NewBacktestResult {
            backtest_id: backtest_uuid,
            strategy_name: "Avellaneda-Stoikov".to_string(),
            symbol: job.symbol.clone(),
            start_date: job.start_date.unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(30)),
            end_date: job.end_date.unwrap_or_else(|| chrono::Utc::now()),
            initial_capital: job.initial_capital.clone(),
            commission_rate: job.commission_rate.clone(),
            slippage_model_type: "None".to_string(),
            slippage_fixed_rate: None,
            slippage_sqrt_rate: None,
            slippage_linear_rate: None,
            temporary_impact: BigDecimal::from(0),
            permanent_impact: BigDecimal::from(0),
            participation_rate_limit: BigDecimal::from_str("0.1")?,
            benchmark: None,
            rebalancing_frequency: "Never".to_string(),
            point_in_time: true,
            warmup_period_days: 30,
            total_return: total_return_bd.clone(),
            annualized_return: cap_opt(result.annualized_return)
                .unwrap_or_else(|| annualized_return.clone()),
            volatility: cap_opt(result.volatility)
                .unwrap_or(BigDecimal::from(0)),
            sharpe_ratio: Some(cap_value(result.sharpe_ratio)),
            sortino_ratio: cap_opt(result.sortino_ratio),
            max_drawdown: cap_value(result.max_drawdown),
            calmar_ratio: cap_opt(result.calmar_ratio),
            win_rate: cap_small(result.win_rate),  // DECIMAL(10,8)
            profit_factor: cap_value(result.profit_factor),
            avg_trade_return: cap_opt(result.avg_trade_return)
                .unwrap_or_else(|| if result.total_trades > 0 {
                    total_return_bd.clone() / BigDecimal::from(result.total_trades as i64)
                } else {
                    BigDecimal::from(0)
                }),
            total_trades: result.total_trades as i32,
            best_trade: cap_opt(result.best_trade),
            worst_trade: cap_opt(result.worst_trade),
            avg_time_in_trade: result.avg_time_in_trade.map(|v| cap_time(v)),  // DECIMAL(10,2)
            value_at_risk_95: cap_opt(result.value_at_risk_95),
            expected_shortfall_95: cap_opt(result.expected_shortfall_95),
            beta: None,
            correlation_with_benchmark: None,
            tracking_error: None,
            information_ratio: None,
            jensen_alpha: None,
            max_drawdown_duration_days: result.max_drawdown_duration_days,
            current_drawdown: cap_opt(result.current_drawdown)
                .unwrap_or(BigDecimal::from(0)),
            avg_drawdown: cap_opt(result.avg_drawdown),
            benchmark_return: None,
            excess_return: None,
            outperformance_periods: None,
            underperformance_periods: None,
            total_orders: result.total_orders.unwrap_or(0),
            filled_orders: result.filled_orders.unwrap_or(0),
            cancelled_orders: result.cancelled_orders.unwrap_or(0),
            avg_slippage: cap_small_opt(result.avg_slippage)  // DECIMAL(10,8)
                .unwrap_or(BigDecimal::from(0)),
            total_commission_paid: cap_opt(result.total_commission_paid)
                .unwrap_or(BigDecimal::from(0)),
            avg_fill_time_seconds: None,
            strategy_metrics: Some(serde_json::json!({
                "risk_aversion": job.risk_aversion.to_string(),
                "inventory_target": job.inventory_target.to_string(),
                "order_size": job.order_size.to_string(),
                "equity_curve": equity_curve_downsampled,
                "monte_carlo": result.monte_carlo.as_ref().map(|mc| serde_json::json!({
                    "num_runs": mc.num_runs,
                    "mean_sharpe": mc.mean_sharpe,
                    "stddev_sharpe": mc.stddev_sharpe,
                    "var_95": mc.var_95,
                    "cvar_95": mc.cvar_95,
                    "confidence_lower": mc.confidence_lower,
                    "confidence_upper": mc.confidence_upper
                })),
                "walk_forward": result.walk_forward.as_ref().map(|wf| serde_json::json!({
                    "periods_tested": wf.periods_tested,
                    "average_performance": wf.average_performance,
                    "consistency_score": wf.consistency_score,
                    "out_of_sample_degradation": wf.out_of_sample_degradation
                }))
            })),
            created_at: now,
            updated_at: now,
            strategy_instance_id: None,
        };
        
        // Debug log all numeric values before insert to diagnose overflow issues
        log_info!(crate::MAIN_LOGGER, "Job {} storing result | total_return={} annualized={} volatility={} sharpe={} max_dd={} win_rate={} profit_factor={}",
            result.job_id,
            new_result.total_return,
            new_result.annualized_return,
            new_result.volatility,
            new_result.sharpe_ratio.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            new_result.max_drawdown,
            new_result.win_rate,
            new_result.profit_factor
        );
        
        // Log additional fields that might overflow
        log_info!(crate::MAIN_LOGGER, "Job {} more fields | avg_trade_return={} best_trade={} worst_trade={} sortino={} calmar={} current_dd={} avg_slippage={} total_commission={}",
            result.job_id,
            new_result.avg_trade_return,
            new_result.best_trade.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            new_result.worst_trade.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            new_result.sortino_ratio.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            new_result.calmar_ratio.as_ref().map(|v| v.to_string()).unwrap_or_default(),
            new_result.current_drawdown,
            new_result.avg_slippage,
            new_result.total_commission_paid
        );
        
        // Insert result into database
        let result_id: uuid::Uuid = diesel::insert_into(backtest_results::table)
            .values(&new_result)
            .returning(backtest_results::id)
            .get_result(&mut conn)
            .await?;
        
        // Update job with result_id
        diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&result.job_id)))
            .set(backtest_jobs::result_id.eq(Some(result_id)))
            .execute(&mut conn)
            .await?;
        
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 1000 {
            log_warn!(crate::MAIN_LOGGER, "PERF: store_result took {}ms for job {}", elapsed.as_millis(), result.job_id);
        }
        
        Ok(())
    }
    
    async fn get_result(&self, job_id: &str) -> Result<Option<BacktestResult>, Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        // Parse job_id as UUID
        let backtest_uuid = match uuid::Uuid::parse_str(job_id) {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };
        
        // Query backtest_results by backtest_id - fetch all relevant fields
        #[allow(clippy::type_complexity)]
        let result: Option<(
            bigdecimal::BigDecimal,                     // total_return
            Option<bigdecimal::BigDecimal>,             // sharpe_ratio
            bigdecimal::BigDecimal,                     // max_drawdown
            bigdecimal::BigDecimal,                     // win_rate
            i32,                                         // total_trades
            bigdecimal::BigDecimal,                     // profit_factor
            Option<serde_json::Value>,                  // strategy_metrics
            chrono::DateTime<chrono::Utc>,              // created_at
            bigdecimal::BigDecimal,                     // annualized_return
            bigdecimal::BigDecimal,                     // volatility
            Option<bigdecimal::BigDecimal>,             // sortino_ratio
            Option<bigdecimal::BigDecimal>,             // calmar_ratio
            bigdecimal::BigDecimal,                     // avg_trade_return
            Option<bigdecimal::BigDecimal>,             // best_trade
            Option<bigdecimal::BigDecimal>,             // worst_trade
            Option<bigdecimal::BigDecimal>,             // avg_time_in_trade
            Option<bigdecimal::BigDecimal>,             // value_at_risk_95
            Option<bigdecimal::BigDecimal>,             // expected_shortfall_95
            Option<i32>,                                // max_drawdown_duration_days
            bigdecimal::BigDecimal,                     // current_drawdown
            Option<bigdecimal::BigDecimal>,             // avg_drawdown
            i32,                                         // total_orders
            i32,                                         // filled_orders
            i32,                                         // cancelled_orders
            bigdecimal::BigDecimal,                     // avg_slippage
            bigdecimal::BigDecimal,                     // total_commission_paid
        )> = 
            backtest_results::table
                .filter(backtest_results::backtest_id.eq(backtest_uuid))
                .select((
                    backtest_results::total_return,
                    backtest_results::sharpe_ratio,
                    backtest_results::max_drawdown,
                    backtest_results::win_rate,
                    backtest_results::total_trades,
                    backtest_results::profit_factor,
                    backtest_results::strategy_metrics,
                    backtest_results::created_at,
                    backtest_results::annualized_return,
                    backtest_results::volatility,
                    backtest_results::sortino_ratio,
                    backtest_results::calmar_ratio,
                    backtest_results::avg_trade_return,
                    backtest_results::best_trade,
                    backtest_results::worst_trade,
                    backtest_results::avg_time_in_trade,
                    backtest_results::value_at_risk_95,
                    backtest_results::expected_shortfall_95,
                    backtest_results::max_drawdown_duration_days,
                    backtest_results::current_drawdown,
                    backtest_results::avg_drawdown,
                    backtest_results::total_orders,
                    backtest_results::filled_orders,
                    backtest_results::cancelled_orders,
                    backtest_results::avg_slippage,
                    backtest_results::total_commission_paid,
                ))
                .first(&mut conn)
                .await
                .optional()?;
        
        match result {
            Some((total_return, sharpe_ratio, max_drawdown, win_rate, 
                  total_trades, profit_factor, strategy_metrics, created_at,
                  annualized_return, volatility, sortino_ratio, calmar_ratio,
                  avg_trade_return, best_trade, worst_trade, avg_time_in_trade,
                  value_at_risk_95, expected_shortfall_95, max_drawdown_duration_days,
                  current_drawdown, avg_drawdown, total_orders, filled_orders,
                  cancelled_orders, avg_slippage, total_commission_paid)) => {
                
                // Extract equity curve and advanced analysis from strategy_metrics JSON
                let (equity_curve, monte_carlo, walk_forward, strategy_params) = if let Some(ref metrics) = strategy_metrics {
                    let equity = metrics.get("equity_curve")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_else(Vec::new);
                    
                    // Extract Monte Carlo results
                    let mc = metrics.get("monte_carlo")
                        .and_then(|v| {
                            if v.is_null() { return None; }
                            Some(MonteCarloSummary {
                                num_runs: v.get("num_runs")?.as_u64()? as usize,
                                mean_sharpe: v.get("mean_sharpe")?.as_f64()?,
                                stddev_sharpe: v.get("stddev_sharpe")?.as_f64().unwrap_or(0.0),
                                var_95: v.get("var_95")?.as_f64()?,
                                cvar_95: v.get("cvar_95")?.as_f64()?,
                                confidence_lower: v.get("confidence_lower")?.as_f64().unwrap_or(0.0),
                                confidence_upper: v.get("confidence_upper")?.as_f64().unwrap_or(0.0),
                                // Percentile distributions
                                p5_total_pnl: v.get("p5_total_pnl").and_then(|x| x.as_f64()),
                                p50_total_pnl: v.get("p50_total_pnl").and_then(|x| x.as_f64()),
                                p95_total_pnl: v.get("p95_total_pnl").and_then(|x| x.as_f64()),
                                p5_max_drawdown: v.get("p5_max_drawdown").and_then(|x| x.as_f64()),
                                p50_max_drawdown: v.get("p50_max_drawdown").and_then(|x| x.as_f64()),
                                p95_max_drawdown: v.get("p95_max_drawdown").and_then(|x| x.as_f64()),
                                p50_sharpe: v.get("p50_sharpe").and_then(|x| x.as_f64()),
                                p5_win_rate: v.get("p5_win_rate").and_then(|x| x.as_f64()),
                                p50_win_rate: v.get("p50_win_rate").and_then(|x| x.as_f64()),
                                p95_win_rate: v.get("p95_win_rate").and_then(|x| x.as_f64()),
                                // VaR reliability indicators
                                data_points: v.get("data_points").and_then(|x| x.as_u64()).map(|x| x as usize).unwrap_or(0),
                                var_unreliable: v.get("var_unreliable").and_then(|x| x.as_bool()).unwrap_or(true),
                            })
                        });
                    
                    // Extract Walk-Forward results
                    let wf = metrics.get("walk_forward")
                        .and_then(|v| {
                            if v.is_null() { return None; }
                            Some(WalkForwardSummary {
                                periods_tested: v.get("periods_tested")?.as_u64()? as usize,
                                average_performance: v.get("average_performance")?.as_f64()?,
                                consistency_score: v.get("consistency_score")?.as_f64()?,
                                out_of_sample_degradation: v.get("out_of_sample_degradation")?.as_f64()?,
                            })
                        });
                    
                    // Extract strategy parameters
                    let params = serde_json::json!({
                        "risk_aversion": metrics.get("risk_aversion"),
                        "inventory_target": metrics.get("inventory_target"),
                        "order_size": metrics.get("order_size")
                    });
                    
                    (equity, mc, wf, Some(params))
                } else {
                    (Vec::new(), None, None, None)
                };
                
                // Helper to convert BigDecimal to f64
                let bd_to_f64 = |bd: &bigdecimal::BigDecimal| -> f64 {
                    bd.to_string().parse().unwrap_or(0.0)
                };
                let opt_bd_to_f64 = |opt: &Option<bigdecimal::BigDecimal>| -> Option<f64> {
                    opt.as_ref().map(|bd| bd.to_string().parse().unwrap_or(0.0))
                };
                
                Ok(Some(BacktestResult {
                    job_id: job_id.to_string(),
                    total_pnl: bd_to_f64(&total_return),
                    sharpe_ratio: opt_bd_to_f64(&sharpe_ratio).unwrap_or(0.0),
                    max_drawdown: bd_to_f64(&max_drawdown),
                    win_rate: bd_to_f64(&win_rate),
                    total_trades: total_trades as usize,
                    profit_factor: bd_to_f64(&profit_factor),
                    equity_curve,
                    completed_at: created_at,
                    // Trade breakdown not stored in DB (legacy), default to None
                    closed_trades: None,
                    open_positions: None,
                    realized_pnl: None,
                    unrealized_pnl: None,
                    annualized_return: Some(bd_to_f64(&annualized_return)),
                    volatility: Some(bd_to_f64(&volatility)),
                    sortino_ratio: opt_bd_to_f64(&sortino_ratio),
                    calmar_ratio: opt_bd_to_f64(&calmar_ratio),
                    avg_trade_return: Some(bd_to_f64(&avg_trade_return)),
                    best_trade: opt_bd_to_f64(&best_trade),
                    worst_trade: opt_bd_to_f64(&worst_trade),
                    avg_time_in_trade: opt_bd_to_f64(&avg_time_in_trade),
                    value_at_risk_95: opt_bd_to_f64(&value_at_risk_95),
                    expected_shortfall_95: opt_bd_to_f64(&expected_shortfall_95),
                    max_drawdown_duration_days,
                    current_drawdown: Some(bd_to_f64(&current_drawdown)),
                    avg_drawdown: opt_bd_to_f64(&avg_drawdown),
                    total_orders: Some(total_orders),
                    filled_orders: Some(filled_orders),
                    cancelled_orders: Some(cancelled_orders),
                    avg_slippage: Some(bd_to_f64(&avg_slippage)),
                    total_commission_paid: Some(bd_to_f64(&total_commission_paid)),
                    strategy_params,
                    monte_carlo,
                    walk_forward,
                    start_date: None,
                    end_date: None,
                }))
            }
            None => Ok(None),
        }
    }
    
    async fn queue_length(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut conn = self.get_conn_with_timeout().await?;
        
        let count: i64 = backtest_jobs::table
            .filter(backtest_jobs::status.eq("queued"))
            .count()
            .get_result(&mut conn)
            .await?;
        
        Ok(count as usize)
    }
    
    fn get_pool(&self) -> Option<Arc<deadpool::Pool<AsyncPgConnection>>> {
        Some(Arc::clone(&self.pool))
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl PostgresQueue {
    /// Get historical backtests with filtering and pagination
    pub async fn get_historical_backtests(
        &self,
        query: &crate::api::history::HistoryQuery,
    ) -> Result<(Vec<crate::api::history::BacktestSummary>, usize), Box<dyn std::error::Error + Send + Sync>> {
        use diesel::prelude::*;
        
        let mut conn = self.get_conn_with_timeout().await?;
        
        // Build count query
        let mut count_query = backtest_jobs::table.into_boxed();
        if let Some(ref status) = query.status {
            count_query = count_query.filter(backtest_jobs::status.eq(status));
        }
        if let Some(ref symbol) = query.symbol {
            count_query = count_query.filter(backtest_jobs::symbol.eq(symbol));
        }
        if let Some(ref exchange) = query.exchange {
            count_query = count_query.filter(backtest_jobs::exchange.eq(exchange));
        }
        if let Some(ref start_str) = query.start_date {
            if let Ok(start_dt) = chrono::DateTime::parse_from_rfc3339(start_str) {
                count_query = count_query.filter(backtest_jobs::created_at.ge(start_dt.with_timezone(&chrono::Utc)));
            }
        }
        if let Some(ref end_str) = query.end_date {
            if let Ok(end_dt) = chrono::DateTime::parse_from_rfc3339(end_str) {
                count_query = count_query.filter(backtest_jobs::created_at.le(end_dt.with_timezone(&chrono::Utc)));
            }
        }
        let total: i64 = diesel_async::RunQueryDsl::get_result(count_query.count(), &mut conn).await?;
        let total = total as usize;
        
        // Build data query with same filters
        let mut data_query = backtest_jobs::table.into_boxed();
        if let Some(ref status) = query.status {
            data_query = data_query.filter(backtest_jobs::status.eq(status));
        }
        if let Some(ref symbol) = query.symbol {
            data_query = data_query.filter(backtest_jobs::symbol.eq(symbol));
        }
        if let Some(ref exchange) = query.exchange {
            data_query = data_query.filter(backtest_jobs::exchange.eq(exchange));
        }
        if let Some(ref start_str) = query.start_date {
            if let Ok(start_dt) = chrono::DateTime::parse_from_rfc3339(start_str) {
                data_query = data_query.filter(backtest_jobs::created_at.ge(start_dt.with_timezone(&chrono::Utc)));
            }
        }
        if let Some(ref end_str) = query.end_date {
            if let Ok(end_dt) = chrono::DateTime::parse_from_rfc3339(end_str) {
                data_query = data_query.filter(backtest_jobs::created_at.le(end_dt.with_timezone(&chrono::Utc)));
            }
        }
        
        // Get paginated results
        let jobs: Vec<BacktestJobRecord> = diesel_async::RunQueryDsl::load(
            data_query
                .order(backtest_jobs::created_at.desc())
                .limit(query.limit)
                .offset(query.offset)
                .select(BacktestJobRecord::as_select()),
            &mut conn
        ).await?;
            
        // Convert to summaries and fetch results if completed
        let mut summaries = Vec::new();
        for job in jobs {
            let mut summary = crate::api::history::BacktestSummary {
                job_id: job.job_id.clone(),
                symbol: job.symbol,
                exchange: job.exchange,
                status: job.status,
                progress: job.progress.to_string().parse().unwrap_or(0.0),
                created_at: job.created_at.to_rfc3339(),
                started_at: job.started_at.map(|dt| dt.to_rfc3339()),
                completed_at: job.completed_at.map(|dt| dt.to_rfc3339()),
                total_pnl: None,
                sharpe_ratio: None,
                max_drawdown: None,
                win_rate: None,
                total_trades: None,
            };
            
            // If completed, fetch results
            if let Some(result_uuid) = job.result_id {
                let results: Vec<(bigdecimal::BigDecimal, Option<bigdecimal::BigDecimal>, bigdecimal::BigDecimal, bigdecimal::BigDecimal, i32)> = 
                    diesel_async::RunQueryDsl::load(
                        backtest_results::table
                            .filter(backtest_results::id.eq(result_uuid))
                            .select((
                                backtest_results::total_return,
                                backtest_results::sharpe_ratio,
                                backtest_results::max_drawdown,
                                backtest_results::win_rate,
                                backtest_results::total_trades,
                            )),
                        &mut conn
                    ).await?;
                
                if let Some(result) = results.into_iter().next() {
                    summary.total_pnl = Some(result.0.to_string().parse().unwrap_or(0.0));
                    summary.sharpe_ratio = result.1.map(|v| v.to_string().parse().unwrap_or(0.0));
                    summary.max_drawdown = Some(result.2.to_string().parse().unwrap_or(0.0));
                    summary.win_rate = Some(result.3.to_string().parse().unwrap_or(0.0));
                    summary.total_trades = Some(result.4 as usize);
                }
            }
            
            summaries.push(summary);
        }
        
        Ok((summaries, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // Requires PostgreSQL instance
    async fn test_postgres_queue() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let queue = PostgresQueue::new(&database_url).unwrap();
        
        let params = BacktestJobParams {
            symbol: "XBTUSD".to_string(),
            exchange: "kraken".to_string(),
            strategy_type: "market_making".to_string(),
            risk_aversion: 0.5,
            inventory_target: 0.0,
            order_size: 0.1,
            initial_capital: 100000.0,
            commission_rate: 0.001,
            start_date: None,
            end_date: None,
            optimization_method: "manual".to_string(),
            population_size: 50,
            generations: 20,
            walkforward_optimization_days: None,
            walkforward_test_days: None,
            walkforward_step_days: None,
            window_ms: 5000,
            min_quote_lifetime_ms: 100,
            fee_rate: 0.001,
            latency_buffer: 10,
            volatility_cap: 0.05,
            time_horizon_minutes: 60,
            market_depth_k: 1.5,
            volatility_adjustment_factor: 0.5,
            inventory_penalty_factor: 0.3,
            trend_sensitivity: 0.2,
            flow_sensitivity: -0.1,
            monte_carlo_enabled: true,
            monte_carlo_runs: 1000,
            walk_forward_enabled: true,
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
        };
        
        let job_id = queue.submit_job(params).await.unwrap();
        assert!(!job_id.is_empty());
        
        let job = queue.get_job(&job_id).await.unwrap().unwrap();
        assert_eq!(job.id, job_id);
        assert_eq!(job.status, JobStatus::Queued);
        
        let next_job = queue.get_next_job().await.unwrap().unwrap();
        assert_eq!(next_job.id, job_id);
        assert_eq!(next_job.status, JobStatus::Running);
    }
}


