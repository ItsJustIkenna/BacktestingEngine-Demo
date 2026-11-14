use crate::schema::l2_orderbook_snapshots;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = l2_orderbook_snapshots)]
pub struct L2OrderbookSnapshot {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub bid_prices: Vec<BigDecimal>,
    pub bid_quantities: Vec<BigDecimal>,
    pub ask_prices: Vec<BigDecimal>,
    pub ask_quantities: Vec<BigDecimal>,
    pub best_bid_price: Option<BigDecimal>,
    pub best_bid_quantity: Option<BigDecimal>,
    pub best_ask_price: Option<BigDecimal>,
    pub best_ask_quantity: Option<BigDecimal>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = l2_orderbook_snapshots)]
pub struct NewL2OrderbookSnapshot {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub security_id: i32,
    pub exchange_id: i32,
    pub bid_prices: Vec<BigDecimal>,
    pub bid_quantities: Vec<BigDecimal>,
    pub ask_prices: Vec<BigDecimal>,
    pub ask_quantities: Vec<BigDecimal>,
    pub best_bid_price: Option<BigDecimal>,
    pub best_bid_quantity: Option<BigDecimal>,
    pub best_ask_price: Option<BigDecimal>,
    pub best_ask_quantity: Option<BigDecimal>,
}

impl NewL2OrderbookSnapshot {
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: impl Into<String>,
        exchange: impl Into<String>,
        security_id: i32,
        exchange_id: i32,
        bid_prices: Vec<BigDecimal>,
        bid_quantities: Vec<BigDecimal>,
        ask_prices: Vec<BigDecimal>,
        ask_quantities: Vec<BigDecimal>,
    ) -> Self {
        let best_bid_price = bid_prices.first().cloned();
        let best_bid_quantity = bid_quantities.first().cloned();
        let best_ask_price = ask_prices.first().cloned();
        let best_ask_quantity = ask_quantities.first().cloned();
        Self {
            timestamp,
            symbol: symbol.into(),
            exchange: exchange.into(),
            security_id,
            exchange_id,
            bid_prices,
            bid_quantities,
            ask_prices,
            ask_quantities,
            best_bid_price,
            best_bid_quantity,
            best_ask_price,
            best_ask_quantity,
        }
    }
}
