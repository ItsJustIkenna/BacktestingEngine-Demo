use crate::models::exchange::{Exchange, NewExchange};
use crate::schema::exchanges::dsl::*;
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

pub async fn exchange_exists(pool: Arc<Pool<AsyncPgConnection>>, name: &str) -> bool {
    let mut c = match checkout(pool).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    diesel::select(diesel::dsl::exists(exchanges.filter(exchange.eq(name))))
        .get_result::<bool>(&mut c)
        .await
        .unwrap_or(false)
}

pub async fn create_exchange(pool: Arc<Pool<AsyncPgConnection>>, new_row: NewExchange) -> Result<Exchange, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::insert_into(exchanges)
        .values(&new_row)
        .get_result(&mut c)
        .await
}

pub async fn get_exchanges_by_name(pool: Arc<Pool<AsyncPgConnection>>, name: &str) -> Result<Exchange, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    exchanges.filter(exchange.eq(name)).first(&mut c).await
}
