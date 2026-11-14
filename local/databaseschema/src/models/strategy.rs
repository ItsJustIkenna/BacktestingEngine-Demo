use crate::schema::{strategies, strategy_instances};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = strategies)]
pub struct Strategy {
    pub id: Uuid,
    pub strategy_name: String,
    pub strategy_type: String,
    pub version: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub is_active: bool,
    pub base_configuration: Option<Value>,
    pub metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = strategies)]
pub struct NewStrategy {
    pub strategy_name: String,
    pub strategy_type: String,
    pub version: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub is_active: bool,
    pub base_configuration: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = strategy_instances)]
pub struct StrategyInstance {
    pub id: Uuid,
    pub strategy_id: Uuid,
    pub instance_name: Option<String>,
    pub description: Option<String>,
    pub parameters: Value,
    pub performance_summary: Option<Value>,
    pub risk_metrics: Option<Value>,
    pub is_template: bool,
    pub tags: Option<Vec<Option<String>>>,
    pub created_by: Option<String>,
    pub optimization_run_id: Option<Uuid>,
    pub optimization_score: Option<BigDecimal>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = strategy_instances)]
pub struct NewStrategyInstance {
    pub strategy_id: Uuid,
    pub instance_name: Option<String>,
    pub description: Option<String>,
    pub parameters: Value,
    pub performance_summary: Option<Value>,
    pub risk_metrics: Option<Value>,
    pub is_template: bool,
    pub tags: Option<Vec<Option<String>>>,
    pub created_by: Option<String>,
    pub optimization_run_id: Option<Uuid>,
    pub optimization_score: Option<BigDecimal>,
}
