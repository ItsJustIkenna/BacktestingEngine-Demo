use super::*;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::pg::PgConnection;
use std::sync::Arc;

// Define the diesel table schema
table! {
    backtest_jobs (id) {
        id -> Uuid,
        job_id -> Varchar,
        symbol -> Varchar,
        exchange -> Varchar,
        strategy_type -> Varchar,
        optimization_method -> Varchar,
        population_size -> Nullable<Int4>,
        generations -> Nullable<Int4>,
        risk_aversion -> Numeric,
        inventory_target -> Numeric,
        order_size -> Numeric,
        initial_capital -> Numeric,
        commission_rate -> Numeric,
        start_date -> Nullable<Timestamptz>,
        end_date -> Nullable<Timestamptz>,
        status -> Varchar,
        progress -> Numeric,
        created_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        error_message -> Nullable<Text>,
        result_id -> Nullable<Uuid>,
        last_heartbeat -> Nullable<Timestamptz>,
        current_generation -> Nullable<Int4>,
        total_generations -> Nullable<Int4>,
        current_phase -> Varchar,
        phase_details -> Jsonb,
    }
}

allow_tables_to_appear_in_same_query!(backtest_jobs, backtest_results);

table! {
    backtest_results (id) {
        id -> Uuid,
        backtest_id -> Uuid,
        strategy_name -> Varchar,
        symbol -> Varchar,
        start_date -> Timestamptz,
        end_date -> Timestamptz,
        initial_capital -> Numeric,
        commission_rate -> Numeric,
        slippage_model_type -> Varchar,
        slippage_fixed_rate -> Nullable<Numeric>,
        slippage_sqrt_rate -> Nullable<Numeric>,
        slippage_linear_rate -> Nullable<Numeric>,
        temporary_impact -> Numeric,
        permanent_impact -> Numeric,
        participation_rate_limit -> Numeric,
        benchmark -> Nullable<Varchar>,
        rebalancing_frequency -> Varchar,
        point_in_time -> Bool,
        warmup_period_days -> Integer,
        total_return -> Numeric,
        annualized_return -> Numeric,
        volatility -> Numeric,
        sharpe_ratio -> Nullable<Numeric>,
        sortino_ratio -> Nullable<Numeric>,
        max_drawdown -> Numeric,
        calmar_ratio -> Nullable<Numeric>,
        win_rate -> Numeric,
        profit_factor -> Numeric,
        avg_trade_return -> Numeric,
        total_trades -> Integer,
        best_trade -> Nullable<Numeric>,
        worst_trade -> Nullable<Numeric>,
        avg_time_in_trade -> Nullable<Numeric>,
        value_at_risk_95 -> Nullable<Numeric>,
        expected_shortfall_95 -> Nullable<Numeric>,
        beta -> Nullable<Numeric>,
        correlation_with_benchmark -> Nullable<Numeric>,
        tracking_error -> Nullable<Numeric>,
        information_ratio -> Nullable<Numeric>,
        jensen_alpha -> Nullable<Numeric>,
        max_drawdown_duration_days -> Nullable<Integer>,
        current_drawdown -> Numeric,
        avg_drawdown -> Nullable<Numeric>,
        benchmark_return -> Nullable<Numeric>,
        excess_return -> Nullable<Numeric>,
        outperformance_periods -> Nullable<Integer>,
        underperformance_periods -> Nullable<Integer>,
        total_orders -> Integer,
        filled_orders -> Integer,
        cancelled_orders -> Integer,
        avg_slippage -> Numeric,
        total_commission_paid -> Numeric,
        avg_fill_time_seconds -> Nullable<Numeric>,
        strategy_metrics -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

#[derive(Insertable)]
#[diesel(table_name = backtest_results)]
pub struct NewBacktestResult {
    pub backtest_id: uuid::Uuid,
    pub strategy_name: String,
    pub symbol: String,
    pub start_date: chrono::DateTime<chrono::Utc>,
    pub end_date: chrono::DateTime<chrono::Utc>,
    pub initial_capital: bigdecimal::BigDecimal,
    pub commission_rate: bigdecimal::BigDecimal,
    pub slippage_model_type: String,
    pub slippage_fixed_rate: Option<bigdecimal::BigDecimal>,
    pub slippage_sqrt_rate: Option<bigdecimal::BigDecimal>,
    pub slippage_linear_rate: Option<bigdecimal::BigDecimal>,
    pub temporary_impact: bigdecimal::BigDecimal,
    pub permanent_impact: bigdecimal::BigDecimal,
    pub participation_rate_limit: bigdecimal::BigDecimal,
    pub benchmark: Option<String>,
    pub rebalancing_frequency: String,
    pub point_in_time: bool,
    pub warmup_period_days: i32,
    pub total_return: bigdecimal::BigDecimal,
    pub annualized_return: bigdecimal::BigDecimal,
    pub volatility: bigdecimal::BigDecimal,
    pub sharpe_ratio: Option<bigdecimal::BigDecimal>,
    pub sortino_ratio: Option<bigdecimal::BigDecimal>,
    pub max_drawdown: bigdecimal::BigDecimal,
    pub calmar_ratio: Option<bigdecimal::BigDecimal>,
    pub win_rate: bigdecimal::BigDecimal,
    pub profit_factor: bigdecimal::BigDecimal,
    pub avg_trade_return: bigdecimal::BigDecimal,
    pub total_trades: i32,
    pub best_trade: Option<bigdecimal::BigDecimal>,
    pub worst_trade: Option<bigdecimal::BigDecimal>,
    pub avg_time_in_trade: Option<bigdecimal::BigDecimal>,
    pub value_at_risk_95: Option<bigdecimal::BigDecimal>,
    pub expected_shortfall_95: Option<bigdecimal::BigDecimal>,
    pub beta: Option<bigdecimal::BigDecimal>,
    pub correlation_with_benchmark: Option<bigdecimal::BigDecimal>,
    pub tracking_error: Option<bigdecimal::BigDecimal>,
    pub information_ratio: Option<bigdecimal::BigDecimal>,
    pub jensen_alpha: Option<bigdecimal::BigDecimal>,
    pub max_drawdown_duration_days: Option<i32>,
    pub current_drawdown: bigdecimal::BigDecimal,
    pub avg_drawdown: Option<bigdecimal::BigDecimal>,
    pub benchmark_return: Option<bigdecimal::BigDecimal>,
    pub excess_return: Option<bigdecimal::BigDecimal>,
    pub outperformance_periods: Option<i32>,
    pub underperformance_periods: Option<i32>,
    pub total_orders: i32,
    pub filled_orders: i32,
    pub cancelled_orders: i32,
    pub avg_slippage: bigdecimal::BigDecimal,
    pub total_commission_paid: bigdecimal::BigDecimal,
    pub avg_fill_time_seconds: Option<bigdecimal::BigDecimal>,
    pub strategy_metrics: Option<String>,
}

#[derive(Queryable, Selectable, Clone, Debug)]
#[diesel(table_name = backtest_jobs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct BacktestJobRecord {
    pub id: uuid::Uuid,
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
    pub status: String,
    pub progress: bigdecimal::BigDecimal,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error_message: Option<String>,
    pub result_id: Option<uuid::Uuid>,
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
    pub current_phase: String,
    pub phase_details: serde_json::Value,
}

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
    pool: Arc<Pool<ConnectionManager<PgConnection>>>,
}

impl PostgresQueue {
    /// Create a new PostgreSQL-backed job queue
    pub fn new(database_url: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let manager = ConnectionManager::<PgConnection>::new(database_url);
        let pool = Pool::builder()
            .max_size(10)
            .build(manager)?;
        
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
    
    /// Convert BacktestJob to database record
    #[allow(dead_code)]
    fn job_to_record(job: &BacktestJob) -> Result<BacktestJobRecord, Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::{BigDecimal, FromPrimitive};
        
        Ok(BacktestJobRecord {
            id: uuid::Uuid::new_v4(),
            job_id: job.id.clone(),
            symbol: job.params.symbol.clone(),
            exchange: job.params.exchange.clone(),
            strategy_type: job.params.strategy_type.clone(),
            optimization_method: job.params.optimization_method.clone(),
            population_size: if job.params.population_size > 0 { Some(job.params.population_size as i32) } else { None },
            generations: if job.params.generations > 0 { Some(job.params.generations as i32) } else { None },
            risk_aversion: BigDecimal::from_f64(job.params.risk_aversion).ok_or("Invalid risk_aversion")?,
            inventory_target: BigDecimal::from_f64(job.params.inventory_target).ok_or("Invalid inventory_target")?,
            order_size: BigDecimal::from_f64(job.params.order_size).ok_or("Invalid order_size")?,
            initial_capital: BigDecimal::from_f64(job.params.initial_capital).ok_or("Invalid initial_capital")?,
            commission_rate: BigDecimal::from_f64(job.params.commission_rate).ok_or("Invalid commission_rate")?,
            start_date: job.params.start_date.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
            end_date: job.params.end_date.as_ref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
            status: match &job.status {
                JobStatus::Queued => "queued",
                JobStatus::Running => "running",
                JobStatus::Completed => "completed",
                JobStatus::Failed => "failed",
            }.to_string(),
            progress: BigDecimal::from_f64(job.progress).ok_or("Invalid progress")?,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            error_message: job.error_message.clone(),
            result_id: None,
            last_heartbeat: None,
            current_generation: Some(job.current_generation),
            total_generations: Some(job.total_generations),
            current_phase: "initializing".to_string(),
            phase_details: serde_json::json!({}),
        })
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
            current_phase: record.current_phase,
            phase_details: record.phase_details,
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
        let pool = self.pool.clone();
        
        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "💾 Submitting job {} to PostgreSQL: symbol={}, exchange={}", 
            job_id, params.symbol, params.exchange);
        
        // Run blocking database operation in dedicated thread pool
        let job_id_clone = job_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            let new_job = NewBacktestJob {
                job_id: job_id_clone,
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
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(job_id)
    }
    
    async fn get_next_job(&self) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        
        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "🔍 Polling PostgreSQL for next queued job");
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            // Use FOR UPDATE SKIP LOCKED for atomic job claiming
            // This prevents multiple workers from grabbing the same job
            let result = conn.transaction::<_, Box<dyn std::error::Error + Send + Sync>, _>(|conn| {
                // Calculate thresholds for orphaned job detection
                let started_threshold = Utc::now() - chrono::Duration::minutes(10); // Fallback: 10min without heartbeat update
                let heartbeat_threshold = Utc::now() - chrono::Duration::minutes(2); // Primary: 2min stale heartbeat
                
                // DEBUG: Log thresholds and check for running jobs
                crate::log_info!(
                    crate::logging_facade::MAIN_LOGGER,
                    "🕐 Orphan thresholds: heartbeat_before={:?}, started_before={:?}",
                    heartbeat_threshold, started_threshold
                );
                
                // Check for any running jobs before claiming attempt
                let running_count: i64 = backtest_jobs::table
                    .filter(backtest_jobs::status.eq("running"))
                    .count()
                    .get_result(conn)?;
                
                if running_count > 0 {
                    crate::log_info!(
                        crate::logging_facade::MAIN_LOGGER,
                        "📊 Found {} running jobs, checking if orphaned...",
                        running_count
                    );
                    
                    // Get details of running jobs
                    let running_jobs: Vec<BacktestJobRecord> = backtest_jobs::table
                        .filter(backtest_jobs::status.eq("running"))
                        .select(BacktestJobRecord::as_select())
                        .load(conn)?;
                    
                    for rj in &running_jobs {
                        let heartbeat_stale = rj.last_heartbeat.map_or(true, |hb| hb < heartbeat_threshold);
                        let started_old = rj.started_at.map_or(false, |st| st < started_threshold);
                        let should_recover = heartbeat_stale || (rj.last_heartbeat.is_none() && started_old);
                        
                        crate::log_info!(
                            crate::logging_facade::MAIN_LOGGER,
                            "  Job {}: started={:?}, heartbeat={:?}, should_recover={}",
                            rj.job_id, rj.started_at, rj.last_heartbeat, should_recover
                        );
                    }
                }
                
                // Try to find a queued job first, or recover orphaned "running" jobs
                // A job is orphaned if:
                // 1. Status is 'running' AND last_heartbeat is stale (>2 min old), OR
                // 2. Status is 'running' AND started_at >10 min ago AND no heartbeat ever set
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
                    .optional()?;
                
                if let Some(job_record) = job {
                    if job_record.status == "running" {
                        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "🔄 Recovering orphaned job {} (stale heartbeat detected)", job_record.job_id);
                    } else {
                        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "🎯 Claiming job {} from queue", job_record.job_id);
                    }
                    
                    // Update status to running and set heartbeat
                    let now = Utc::now();
                    diesel::update(backtest_jobs::table.find(job_record.id))
                        .set((
                            backtest_jobs::status.eq("running"),
                            backtest_jobs::started_at.eq(Some(now)),
                            backtest_jobs::last_heartbeat.eq(Some(now)),
                        ))
                        .execute(conn)?;
                    
                    Ok(Some(Self::record_to_job(job_record)?))
                } else {
                    crate::log_info!(crate::logging_facade::MAIN_LOGGER, "⏸️ No jobs available in queue");
                    Ok(None)
                }
            })?;
            
            Ok(result)
        }).await?
    }
    
    async fn get_job(&self, job_id: &str) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "🔎 Fetching job {} from PostgreSQL", job_id);
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            let record: Option<BacktestJobRecord> = backtest_jobs::table
                .filter(backtest_jobs::job_id.eq(&job_id))
                .select(BacktestJobRecord::as_select())
                .first(&mut conn)
                .optional()?;
            
            match record {
                Some(r) => Ok(Some(Self::record_to_job(r)?)),
                None => Ok(None),
            }
        }).await?
    }
    
    async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            let status_str = match status {
                JobStatus::Queued => "queued",
                JobStatus::Running => "running",
                JobStatus::Completed => "completed",
                JobStatus::Failed => "failed",
            };
            
            diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&job_id)))
                .set((
                    backtest_jobs::status.eq(status_str),
                    backtest_jobs::error_message.eq(error_message),
                ))
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    async fn update_progress(&self, job_id: &str, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&job_id)))
                .set(backtest_jobs::progress.eq(BigDecimal::from_str(&progress.to_string())?))
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    /// Update generation progress for genetic optimization
    async fn update_generation(&self, job_id: &str, current_generation: i32, total_generations: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&job_id)))
                .set((
                    backtest_jobs::current_generation.eq(current_generation),
                    backtest_jobs::total_generations.eq(total_generations),
                ))
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    /// Update current phase of job execution
    async fn update_phase(&self, job_id: &str, phase: &str, phase_details: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        let phase = phase.to_string();
        let phase_details_str = serde_json::to_string(&phase_details)?;
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            // Use raw SQL to update JSONB field
            diesel::sql_query(
                "UPDATE backtest_jobs SET current_phase = $1, phase_details = $2::jsonb WHERE job_id = $3"
            )
            .bind::<diesel::sql_types::Text, _>(&phase)
            .bind::<diesel::sql_types::Text, _>(&phase_details_str)
            .bind::<diesel::sql_types::Text, _>(&job_id)
            .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    /// Update job heartbeat timestamp
    /// Called every 30 seconds by worker to signal it's still alive
    async fn update_heartbeat(&self, job_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&job_id)))
                .set(backtest_jobs::last_heartbeat.eq(Utc::now()))
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    async fn store_result(&self, result: &BacktestResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        crate::log_info!(crate::logging_facade::MAIN_LOGGER, "💾 Storing result for job {}: sharpe={:.2}, pnl=${:.2}, trades={}", 
            result.job_id, result.sharpe_ratio, result.total_pnl, result.total_trades);
        
        let pool = self.pool.clone();
        let result = result.clone();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            // Get job to find dates and parameters
            let job: BacktestJobRecord = backtest_jobs::table
                .filter(backtest_jobs::job_id.eq(&result.job_id))
                .select(BacktestJobRecord::as_select())
                .first(&mut conn)?;
            
            // Create backtest result record
            let backtest_uuid = uuid::Uuid::parse_str(&result.job_id)?;
            
            // Calculate annualized metrics (simplified - using 252 trading days)
            let total_return_bd = BigDecimal::from_str(&result.total_pnl.to_string())?;
            let annualized_return = total_return_bd.clone(); // Simplified
            
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
                annualized_return,
                volatility: BigDecimal::from(0), // Would need equity curve analysis
                sharpe_ratio: Some(BigDecimal::from_str(&result.sharpe_ratio.to_string())?),
                sortino_ratio: None,
                max_drawdown: BigDecimal::from_str(&result.max_drawdown.to_string())?,
                calmar_ratio: None,
                win_rate: BigDecimal::from_str(&result.win_rate.to_string())?,
                profit_factor: BigDecimal::from_str(&result.profit_factor.to_string())?,
                avg_trade_return: if result.total_trades > 0 {
                    total_return_bd / BigDecimal::from(result.total_trades as i64)
                } else {
                    BigDecimal::from(0)
                },
                total_trades: result.total_trades as i32,
                best_trade: None,
                worst_trade: None,
                avg_time_in_trade: None,
                value_at_risk_95: None,
                expected_shortfall_95: None,
                beta: None,
                correlation_with_benchmark: None,
                tracking_error: None,
                information_ratio: None,
                jensen_alpha: None,
                max_drawdown_duration_days: None,
                current_drawdown: BigDecimal::from(0),
                avg_drawdown: None,
                benchmark_return: None,
                excess_return: None,
                outperformance_periods: None,
                underperformance_periods: None,
                total_orders: 0,
                filled_orders: 0,
                cancelled_orders: 0,
                avg_slippage: BigDecimal::from(0),
                total_commission_paid: BigDecimal::from(0),
                avg_fill_time_seconds: None,
                strategy_metrics: Some(serde_json::to_string(&serde_json::json!({
                    "risk_aversion": job.risk_aversion.to_string(),
                    "inventory_target": job.inventory_target.to_string(),
                    "order_size": job.order_size.to_string(),
                    "equity_curve": result.equity_curve
                }))?),
            };
            
            // Insert result into database
            let result_id: uuid::Uuid = diesel::insert_into(backtest_results::table)
                .values(&new_result)
                .returning(backtest_results::id)
                .get_result(&mut conn)?;
            
            // Update job with result_id
            diesel::update(backtest_jobs::table.filter(backtest_jobs::job_id.eq(&result.job_id)))
                .set(backtest_jobs::result_id.eq(Some(result_id)))
                .execute(&mut conn)?;
            
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        
        Ok(())
    }
    
    async fn get_result(&self, job_id: &str) -> Result<Option<BacktestResult>, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        let job_id = job_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            // Parse job_id as UUID
            let backtest_uuid = match uuid::Uuid::parse_str(&job_id) {
                Ok(u) => u,
                Err(_) => return Ok(None),
            };
            
            // Query backtest_results by backtest_id
            let result: Option<(bigdecimal::BigDecimal, Option<bigdecimal::BigDecimal>, bigdecimal::BigDecimal, 
                                bigdecimal::BigDecimal, i32, bigdecimal::BigDecimal, 
                                Option<String>, chrono::DateTime<chrono::Utc>)> = 
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
                    ))
                    .first(&mut conn)
                    .optional()?;
            
            match result {
                Some((total_return, sharpe_ratio, max_drawdown, win_rate, 
                      total_trades, profit_factor, strategy_metrics, created_at)) => {
                    // Extract equity curve from strategy_metrics JSON string
                    let equity_curve = if let Some(metrics_str) = strategy_metrics {
                        if let Ok(metrics) = serde_json::from_str::<serde_json::Value>(&metrics_str) {
                            metrics.get("equity_curve")
                                .and_then(|v| serde_json::from_value(v.clone()).ok())
                                .unwrap_or_else(Vec::new)
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    };
                    
                    Ok(Some(BacktestResult {
                        job_id,
                        total_pnl: total_return.to_string().parse().unwrap_or(0.0),
                        sharpe_ratio: sharpe_ratio.map(|s| s.to_string().parse().unwrap_or(0.0)).unwrap_or(0.0),
                        max_drawdown: max_drawdown.to_string().parse().unwrap_or(0.0),
                        win_rate: win_rate.to_string().parse().unwrap_or(0.0),
                        total_trades: total_trades as usize,
                        profit_factor: profit_factor.to_string().parse().unwrap_or(1.0),
                        equity_curve,
                        completed_at: created_at,
                    }))
                }
                None => Ok(None),
            }
        }).await?
    }
    
    async fn queue_length(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let pool = self.pool.clone();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            let count: i64 = backtest_jobs::table
                .filter(backtest_jobs::status.eq("queued"))
                .count()
                .get_result(&mut conn)?;
            
            Ok(count as usize)
        }).await?
    }
    
    fn get_pool(&self) -> Option<&diesel::r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::pg::PgConnection>>> {
        Some(&self.pool)
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
        
        let pool = self.pool.clone();
        let query_clone = query.clone();
        
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            
            // Build count query
            let mut count_query = backtest_jobs::table.into_boxed();
            if let Some(ref status) = query_clone.status {
                count_query = count_query.filter(backtest_jobs::status.eq(status));
            }
            if let Some(ref symbol) = query_clone.symbol {
                count_query = count_query.filter(backtest_jobs::symbol.eq(symbol));
            }
            if let Some(ref exchange) = query_clone.exchange {
                count_query = count_query.filter(backtest_jobs::exchange.eq(exchange));
            }
            if let Some(ref start_str) = query_clone.start_date {
                if let Ok(start_dt) = chrono::DateTime::parse_from_rfc3339(start_str) {
                    count_query = count_query.filter(backtest_jobs::created_at.ge(start_dt.with_timezone(&chrono::Utc)));
                }
            }
            if let Some(ref end_str) = query_clone.end_date {
                if let Ok(end_dt) = chrono::DateTime::parse_from_rfc3339(end_str) {
                    count_query = count_query.filter(backtest_jobs::created_at.le(end_dt.with_timezone(&chrono::Utc)));
                }
            }
            let total: i64 = count_query.count().get_result(&mut conn)?;
            let total = total as usize;
            
            // Build data query with same filters
            let mut data_query = backtest_jobs::table.into_boxed();
            if let Some(ref status) = query_clone.status {
                data_query = data_query.filter(backtest_jobs::status.eq(status));
            }
            if let Some(ref symbol) = query_clone.symbol {
                data_query = data_query.filter(backtest_jobs::symbol.eq(symbol));
            }
            if let Some(ref exchange) = query_clone.exchange {
                data_query = data_query.filter(backtest_jobs::exchange.eq(exchange));
            }
            if let Some(ref start_str) = query_clone.start_date {
                if let Ok(start_dt) = chrono::DateTime::parse_from_rfc3339(start_str) {
                    data_query = data_query.filter(backtest_jobs::created_at.ge(start_dt.with_timezone(&chrono::Utc)));
                }
            }
            if let Some(ref end_str) = query_clone.end_date {
                if let Ok(end_dt) = chrono::DateTime::parse_from_rfc3339(end_str) {
                    data_query = data_query.filter(backtest_jobs::created_at.le(end_dt.with_timezone(&chrono::Utc)));
                }
            }
            
            // Get paginated results
            let jobs: Vec<BacktestJobRecord> = data_query
                .order(backtest_jobs::created_at.desc())
                .limit(query_clone.limit)
                .offset(query_clone.offset)
                .select(BacktestJobRecord::as_select())
                .load(&mut conn)?;
            
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
                    if let Ok(Some(result)) = backtest_results::table
                        .filter(backtest_results::id.eq(result_uuid))
                        .select((
                            backtest_results::total_return,
                            backtest_results::sharpe_ratio,
                            backtest_results::max_drawdown,
                            backtest_results::win_rate,
                            backtest_results::total_trades,
                        ))
                        .first::<(bigdecimal::BigDecimal, Option<bigdecimal::BigDecimal>, bigdecimal::BigDecimal, bigdecimal::BigDecimal, i32)>(&mut conn)
                        .optional()
                    {
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
        }).await?
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
