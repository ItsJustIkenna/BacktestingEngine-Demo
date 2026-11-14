use crate::schema::securities;
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = securities)]
pub struct Security {
    pub security_id: i32,
    pub symbol: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = securities)]
pub struct NewSecurity {
    pub symbol: String,
}

impl NewSecurity {
    pub fn new(symbol: impl Into<String>) -> Self {
        Self { symbol: symbol.into() }
    }
}
