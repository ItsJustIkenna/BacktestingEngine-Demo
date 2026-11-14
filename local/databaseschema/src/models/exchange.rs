use crate::schema::exchanges;
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = exchanges)]
pub struct Exchange {
    pub exchange_id: i32,
    pub exchange: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = exchanges)]
pub struct NewExchange {
    pub exchange: String,
}

impl NewExchange {
    pub fn new(name: impl Into<String>) -> Self {
        Self { exchange: name.into() }
    }
}
