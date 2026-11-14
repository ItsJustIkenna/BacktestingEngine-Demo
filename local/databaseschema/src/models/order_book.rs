use crate::schema::order_books;
use bigdecimal::BigDecimal;
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = order_books)]
pub struct OrderBook {
    pub order_book_id: i32,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub buy_order_book_id: i32,
    pub sell_order_book_id: i32,
    pub total_volume: BigDecimal,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = order_books)]
pub struct NewOrderBook {
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub buy_order_book_id: i32,
    pub sell_order_book_id: i32,
    pub total_volume: BigDecimal,
}

impl NewOrderBook {
    pub fn new(symbol: &str, exchange: &str, security_id: i32, exchange_id: i32) -> Self {
        Self {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            security_id,
            exchange_id,
            buy_order_book_id: 0,
            sell_order_book_id: 0,
            total_volume: BigDecimal::from(0),
        }
    }
}
