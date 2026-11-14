use crate::models::security::{NewSecurity, Security};
use crate::schema::securities::dsl::*;
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

pub async fn security_exists(pool: Arc<Pool<AsyncPgConnection>>, sym: &str) -> bool {
    let mut c = match checkout(pool).await {
        Ok(c) => c,
        Err(_) => return false,
    };
    diesel::select(diesel::dsl::exists(securities.filter(symbol.eq(sym))))
        .get_result::<bool>(&mut c)
        .await
        .unwrap_or(false)
}

pub async fn create_security(pool: Arc<Pool<AsyncPgConnection>>, new_row: NewSecurity) -> Result<Security, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    diesel::insert_into(securities).values(&new_row).get_result(&mut c).await
}

pub async fn get_security_by_symbol(pool: Arc<Pool<AsyncPgConnection>>, sym: &str) -> Result<Security, diesel::result::Error> {
    let mut c = checkout(pool).await?;
    securities.filter(symbol.eq(sym)).first(&mut c).await
}
