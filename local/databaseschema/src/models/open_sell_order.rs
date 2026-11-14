use crate::schema::open_sell_orders;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = open_sell_orders)]
pub struct OpenSellOrder {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub order_book_id: i32,
    pub unique_id: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = open_sell_orders)]
pub struct NewOpenSellOrder {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub order_book_id: i32,
    pub unique_id: String,
    pub price_level: BigDecimal,
    pub quantity: BigDecimal,
}

impl NewOpenSellOrder {
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: &str,
        exchange: &str,
        security_id: i32,
        exchange_id: i32,
        order_book_id: i32,
        unique_id: impl Into<String>,
        price_level: impl crate::IntoBigDecimal,
        quantity: impl crate::IntoBigDecimal,
    ) -> Self {
        Self {
            timestamp,
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            security_id,
            exchange_id,
            order_book_id,
            unique_id: unique_id.into(),
            price_level: crate::into_bd(price_level),
            quantity: crate::into_bd(quantity),
        }
    }
}
