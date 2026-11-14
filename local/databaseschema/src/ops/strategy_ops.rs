use crate::models::strategy::{NewStrategy, NewStrategyInstance, Strategy, StrategyInstance};
use crate::schema::{strategies, strategy_instances};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, RunQueryDsl};

pub struct StrategyOperations;

impl StrategyOperations {
    pub async fn get_strategy_by_name_version(
        conn: &mut impl AsyncConnection<Backend = diesel::pg::Pg>,
        name: &str,
        ver: &str,
    ) -> Result<Strategy, diesel::result::Error> {
        strategies::table
            .filter(strategies::strategy_name.eq(name))
            .filter(strategies::version.eq(ver))
            .first(conn)
            .await
    }

    pub async fn create_strategy(
        conn: &mut impl AsyncConnection<Backend = diesel::pg::Pg>,
        new_row: NewStrategy,
    ) -> Result<Strategy, diesel::result::Error> {
        diesel::insert_into(strategies::table)
            .values(&new_row)
            .get_result(conn)
            .await
    }

    pub async fn create_strategy_instance(
        conn: &mut impl AsyncConnection<Backend = diesel::pg::Pg>,
        new_row: NewStrategyInstance,
    ) -> Result<StrategyInstance, diesel::result::Error> {
        diesel::insert_into(strategy_instances::table)
            .values(&new_row)
            .get_result(conn)
            .await
    }
}
