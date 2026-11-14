use crate::models::wallet_balance::{NewWalletBalance, WalletBalance};
use crate::schema::wallet_balances::dsl::*;
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

pub async fn create_wallet_balances(
    pool: Arc<Pool<AsyncPgConnection>>,
    rows: Vec<NewWalletBalance>,
) -> Result<Vec<WalletBalance>, diesel::result::Error> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut c = checkout(pool).await?;
    diesel::insert_into(wallet_balances).values(&rows).get_results(&mut c).await
}
