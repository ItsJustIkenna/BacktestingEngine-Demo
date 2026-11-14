use crate::schema::trades;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = trades)]
pub struct Trade {
    pub id: i64,
    pub trade_id: String,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub side: String,
    pub price: BigDecimal,
    pub quantity: BigDecimal,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = trades)]
pub struct NewTrade {
    pub trade_id: String,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub side: String,
    pub price: BigDecimal,
    pub quantity: BigDecimal,
}

impl NewTrade {
    pub fn new(
        symbol: &str,
        exchange: &str,
        trade_id: &str,
        security_id: i32,
        exchange_id: i32,
        side: &str,
        price: impl crate::IntoBigDecimal,
        quantity: impl crate::IntoBigDecimal,
    ) -> Self {
        use std::str::FromStr;
        Self {
            trade_id: trade_id.to_string(),
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            security_id,
            exchange_id,
            side: side.to_string(),
            price: crate::into_bd(price),
            quantity: crate::into_bd(quantity),
        }
    }
}
