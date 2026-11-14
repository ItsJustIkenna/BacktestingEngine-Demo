use crate::models::order_book::{NewOrderBook, OrderBook};
use crate::schema::order_books::dsl::*;
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

pub async fn orderbook_exists(pool: Arc<Pool<AsyncPgConnection>>, sec_id: i32) -> bool {
    let mut c = match checkout(pool).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    diesel::select(diesel::dsl::exists(order_books.filter(security_id.eq(sec_id))))
        .get_result::<bool>(&mut c)
        .await
        .unwrap_or(false)
}

pub async fn create_orderbook(pool: Arc<Pool<AsyncPgConnection>>, new_row: NewOrderBook) -> Result<OrderBook, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::insert_into(order_books).values(&new_row).get_result(&mut c).await
}

pub async fn get_orderbook_by_exchange_id_and_security_id(
    pool: Arc<Pool<AsyncPgConnection>>,
    ex_id: i32,
    sec_id: i32,
) -> Result<OrderBook, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    order_books
        .filter(exchange_id.eq(ex_id))
        .filter(security_id.eq(sec_id))
        .first(&mut c)
        .await
}

pub async fn update_orderbook(
    pool: Arc<Pool<AsyncPgConnection>>,
    book: OrderBook,
    volume: BigDecimal,
) -> Result<OrderBook, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::update(order_books.filter(order_book_id.eq(book.order_book_id)))
        .set(total_volume.eq(volume))
        .get_result(&mut c)
        .await
}
