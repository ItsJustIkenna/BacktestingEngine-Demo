use crate::models::historical_snapshot::HistoricalSnapshot;
use crate::schema::historical_snapshots::dsl::*;
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

pub async fn get_historical_snapshot(
    pool: Arc<Pool<AsyncPgConnection>>,
    sym: &str,
    exch: &str,
) -> Result<Vec<HistoricalSnapshot>, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    historical_snapshots
        .filter(symbol.eq(sym))
        .filter(exchange.eq(exch))
        .order(timestamp.asc())
        .load(&mut c)
        .await
}

pub async fn create_historical_snapshot(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<crate::models::historical_snapshot::NewHistoricalSnapshot>,
) -> Result<Vec<HistoricalSnapshot>, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(historical_snapshots)
        .values(&rows)
        .get_results(&mut c)
        .await
}
