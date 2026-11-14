//! Fresh in-repo database types for standalone demo builds.
//! Connection helpers talk to PostgreSQL when DATABASE_URL (or TIMESCALE_URL)
//! is set. They do not embed credentials.

pub mod models;
pub mod ops;
pub mod schema;

pub(crate) fn into_bd(value: impl IntoBigDecimal) -> bigdecimal::BigDecimal {
    value.into_bigdecimal()
}

pub(crate) trait IntoBigDecimal {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal;
}

impl IntoBigDecimal for bigdecimal::BigDecimal {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal { self }
}
impl IntoBigDecimal for &bigdecimal::BigDecimal {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal { self.clone() }
}
impl IntoBigDecimal for &str {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal {
        use std::str::FromStr;
        bigdecimal::BigDecimal::from_str(self).unwrap_or_else(|_| bigdecimal::BigDecimal::from(0))
    }
}
impl IntoBigDecimal for String {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal { (&*self).into_bigdecimal() }
}
impl IntoBigDecimal for &String {
    fn into_bigdecimal(self) -> bigdecimal::BigDecimal { self.as_str().into_bigdecimal() }
}

use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::{Object, Pool};
use diesel_async::AsyncPgConnection;
use std::sync::Arc;

pub type PgPool = Pool<AsyncPgConnection>;
pub type PgPooledConnection = Object<AsyncPgConnection>;

/// Build a deadpool for Timescale/Postgres.
///
/// The URL is read from `DATABASE_URL` or `TIMESCALE_URL`. If neither is set,
/// a local default URL is used so the pool can be constructed; checkout will
/// fail until a real database is available.
pub fn create_timescale_connection_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("TIMESCALE_URL"))
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:5432/postgres".to_string());
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(url);
    Pool::builder(manager)
        .build()
        .expect("failed to construct postgres connection pool")
}

pub async fn get_timescale_connection(
    pool: Arc<PgPool>,
) -> anyhow::Result<PgPooledConnection> {
    pool.get()
        .await
        .map_err(|e| anyhow::anyhow!("failed to check out a postgres connection: {e}"))
}
