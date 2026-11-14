use crate::schema::backtest_jobs;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = backtest_jobs)]
pub struct BacktestJobRecord {
    pub id: Uuid,
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub strategy_type: String,
    pub optimization_method: String,
    pub population_size: Option<i32>,
    pub generations: Option<i32>,
    pub risk_aversion: BigDecimal,
    pub inventory_target: BigDecimal,
    pub order_size: BigDecimal,
    pub initial_capital: BigDecimal,
    pub commission_rate: BigDecimal,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub params_json: Value,
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub progress: BigDecimal,
    pub current_phase: Option<String>,
    pub phase_details: Option<Value>,
    pub error_message: Option<String>,
    pub result_id: Option<Uuid>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = backtest_jobs)]
pub struct NewBacktestJob {
    pub job_id: String,
    pub symbol: String,
    pub exchange: String,
    pub strategy_type: String,
    pub optimization_method: String,
    pub population_size: Option<i32>,
    pub generations: Option<i32>,
    pub risk_aversion: BigDecimal,
    pub inventory_target: BigDecimal,
    pub order_size: BigDecimal,
    pub initial_capital: BigDecimal,
    pub commission_rate: BigDecimal,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub params_json: Value,
    pub current_generation: Option<i32>,
    pub total_generations: Option<i32>,
}
