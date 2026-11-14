use crate::schema::historical_snapshots;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = historical_snapshots)]
pub struct HistoricalSnapshot {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
    pub side: String,
    pub payload: Value,
}

impl HistoricalSnapshot {
    pub fn get_timestamp(&self) -> DateTime<Utc> {
        self.timestamp
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = historical_snapshots)]
pub struct NewHistoricalSnapshot {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
    pub side: String,
    pub payload: Value,
}
