use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub open_price: BigDecimal,
    pub high_price: BigDecimal,
    pub low_price: BigDecimal,
    pub close_price: BigDecimal,
    pub volume: BigDecimal,
    pub trade_count: i32,
}
