use crate::models::trade::{NewTrade, Trade};
use crate::schema::trades::dsl::*;
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

pub async fn create_trades(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<NewTrade>,
) -> Result<Vec<Trade>, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(trades).values(&rows).get_results(&mut c).await
}

pub async fn get_trades_by_symbol(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<Vec<Trade>, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    trades.filter(symbol.eq(sym)).filter(exchange.eq(exch)).load(&mut c).await
}

pub async fn delete_trades_by_symbol_exchange(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<usize, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::delete(trades.filter(symbol.eq(sym)).filter(exchange.eq(exch)))
        .execute(&mut c)
        .await
}
