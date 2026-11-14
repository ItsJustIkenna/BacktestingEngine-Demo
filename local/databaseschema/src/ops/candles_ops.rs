use crate::models::candle::Candle;
use crate::schema::{candles_15m, candles_1d, candles_1h, candles_1m, candles_5m};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use diesel_async::pooled_connection::deadpool::Pool;
use std::sync::Arc;

async fn checkout(pool: Arc<Pool<AsyncPgConnection>>) -> Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>, diesel::result::Error> {
    pool.get().await.map_err(|e| diesel::result::Error::DatabaseError(
        diesel::result::DatabaseErrorKind::UnableToSendCommand,
        Box::new(e.to_string()),
    ))
}

macro_rules! load_candles {
    ($table:ident, $pool:expr, $sym:expr, $exch:expr, $start:expr, $end:expr, $limit:expr) => {{
        let mut c = checkout($pool).await?;
        let mut q = $table::table
            .filter($table::symbol.eq($sym))
            .filter($table::exchange.eq($exch))
            .into_boxed();
        if let Some(start) = $start {
            q = q.filter($table::timestamp.ge(start));
        }
        if let Some(end) = $end {
            q = q.filter($table::timestamp.lt(end));
        }
        q = q.order($table::timestamp.asc());
        if let Some(lim) = $limit {
            q = q.limit(lim as i64);
        }
        let rows: Vec<(
            i64,
            DateTime<Utc>,
            String,
            String,
            bigdecimal::BigDecimal,
            bigdecimal::BigDecimal,
            bigdecimal::BigDecimal,
            bigdecimal::BigDecimal,
            bigdecimal::BigDecimal,
            i32,
        )> = q.load(&mut c).await?;
        Ok(rows
            .into_iter()
            .map(|(_id, timestamp, symbol, exchange, open_price, high_price, low_price, close_price, volume, trade_count)| Candle {
                timestamp,
                symbol,
                exchange,
                open_price,
                high_price,
                low_price,
                close_price,
                volume,
                trade_count,
            })
            .collect())
    }};
}

pub async fn get_candles_1m(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<Candle>, diesel::result::Error> {
    load_candles!(candles_1m, pool, sym, exch, start, end, limit)
}

pub async fn get_candles_5m(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<Candle>, diesel::result::Error> {
    load_candles!(candles_5m, pool, sym, exch, start, end, limit)
}

pub async fn get_candles_15m(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<Candle>, diesel::result::Error> {
    load_candles!(candles_15m, pool, sym, exch, start, end, limit)
}

pub async fn get_candles_1h(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<Candle>, diesel::result::Error> {
    load_candles!(candles_1h, pool, sym, exch, start, end, limit)
}

pub async fn get_candles_1d(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<i64>,
) -> Result<Vec<Candle>, diesel::result::Error> {
    load_candles!(candles_1d, pool, sym, exch, start, end, limit)
}
