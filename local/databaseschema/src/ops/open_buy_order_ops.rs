use crate::models::open_buy_order::NewOpenBuyOrder;
use crate::schema::open_buy_orders::dsl::*;
use bigdecimal::BigDecimal;
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

pub async fn create_open_buy_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<NewOpenBuyOrder>,
) -> Result<usize, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(open_buy_orders).values(&rows).execute(&mut c).await
}

pub async fn modify_open_buy_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<(impl Into<String>, impl crate::IntoBigDecimal, impl crate::IntoBigDecimal)>,
) -> Result<usize, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    let mut n = 0usize;
    for (oid, price, qty) in rows {
        let oid = oid.into();
        let price = crate::into_bd(price);
        let qty = crate::into_bd(qty);
        n += diesel::update(open_buy_orders.filter(unique_id.eq(oid)))
            .set((price_level.eq(price), quantity.eq(qty)))
            .execute(&mut c)
            .await?;
    }
    Ok(n)
}

pub async fn delete_open_buy_orders(
    pool: Arc<Pool<AsyncPgConnection>>,
    ids: &[String],
) -> Result<usize, diesel::result::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut c = checkout(pool).await?;
    diesel::delete(open_buy_orders.filter(unique_id.eq_any(ids)))
        .execute(&mut c)
        .await
}
