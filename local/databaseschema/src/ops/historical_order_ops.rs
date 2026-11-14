use crate::models::historical_order::{HistoricalOrder, NewHistoricalOrder};
use crate::schema::historical_orders::dsl::*;
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

pub async fn create_historical_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<NewHistoricalOrder>,
) -> Result<Vec<HistoricalOrder>, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(historical_orders)
        .values(&rows)
        .get_results(&mut c)
        .await
}

pub async fn get_historical_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<Vec<HistoricalOrder>, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    historical_orders
        .filter(symbol.eq(sym))
        .filter(exchange.eq(exch))
        .order(timestamp.asc())
        .load(&mut c)
        .await
}

pub async fn get_bootstrap_historical_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    limit: usize,
    _block_size: Option<usize>,
    _seed: Option<u64>,
) -> Result<Vec<HistoricalOrder>, diesel::result::Error> {
    let mut rows = get_historical_orders(pool, sym, exch).await?;
    rows.truncate(limit);
    Ok(rows)
}

pub async fn get_randomized_historical_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    _window_minutes: i32,
    _seed: Option<u64>,
) -> Result<Vec<HistoricalOrder>, diesel::result::Error> {
    get_historical_orders(pool, sym, exch).await
}

pub async fn delete_historical_orders_by_symbol_exchange(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<usize, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::delete(historical_orders.filter(symbol.eq(sym)).filter(exchange.eq(exch)))
        .execute(&mut c)
        .await
}
