use crate::models::l2_orderbook_snapshot::{L2OrderbookSnapshot, NewL2OrderbookSnapshot};
use crate::schema::l2_orderbook_snapshots::dsl::*;
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

pub async fn create_l2_snapshots(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<NewL2OrderbookSnapshot>,
) -> Result<usize, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(l2_orderbook_snapshots).values(&rows).execute(&mut c).await
}

pub async fn get_l2_snapshots_by_range(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<L2OrderbookSnapshot>, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    l2_orderbook_snapshots
        .filter(symbol.eq(sym))
        .filter(exchange.eq(exch))
        .filter(timestamp.ge(start))
        .filter(timestamp.lt(end))
        .order(timestamp.asc())
        .load(&mut c)
        .await
}

pub async fn delete_l2_snapshots_by_symbol_exchange(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<usize, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::delete(l2_orderbook_snapshots.filter(symbol.eq(sym)).filter(exchange.eq(exch)))
        .execute(&mut c)
        .await
}
