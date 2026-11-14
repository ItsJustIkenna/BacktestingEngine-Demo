use crate::schema::historical_orders;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = historical_orders)]
pub struct HistoricalOrder {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub unique_id: String,
    pub order_id: String,
    pub event_type: String,
    pub side: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
    pub remaining_quantity: Option<BigDecimal>,
    pub fill_quantity: Option<BigDecimal>,
    pub prev_price: Option<BigDecimal>,
    pub prev_quantity: Option<BigDecimal>,
    pub status: String,
    pub exchange: String,
    pub symbol: String,
    pub exchange_id: i32,
    pub security_id: i32,
}

impl HistoricalOrder {
    pub fn get_timestamp(&self) -> &DateTime<Utc> {
        &self.timestamp
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = historical_orders)]
pub struct NewHistoricalOrder {
    pub timestamp: DateTime<Utc>,
    pub unique_id: String,
    pub order_id: String,
    pub event_type: String,
    pub side: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
    pub remaining_quantity: Option<BigDecimal>,
    pub fill_quantity: Option<BigDecimal>,
    pub prev_price: Option<BigDecimal>,
    pub prev_quantity: Option<BigDecimal>,
    pub status: String,
    pub exchange: String,
    pub symbol: String,
    pub exchange_id: i32,
    pub security_id: i32,
}

impl NewHistoricalOrder {
    pub fn new(
        timestamp: DateTime<Utc>,
        unique_id: impl Into<String>,
        event_type: impl Into<String>,
        side: impl Into<String>,
        price_level: impl crate::IntoBigDecimal,
        quantity: impl crate::IntoBigDecimal,
        remaining_quantity: Option<BigDecimal>,
        fill_quantity: Option<BigDecimal>,
        status: impl Into<String>,
        exchange: &str,
        symbol: &str,
        exchange_id: i32,
        security_id: i32,
    ) -> Self {
        let unique_id = unique_id.into();
        Self {
            timestamp,
            order_id: unique_id.clone(),
            unique_id,
            event_type: event_type.into(),
            side: side.into(),
            price_level: crate::into_bd(price_level),
            quantity: crate::into_bd(quantity),
            remaining_quantity,
            fill_quantity,
            prev_price: None,
            prev_quantity: None,
            status: status.into(),
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            exchange_id,
            security_id,
        }
    }
}
