use crate::schema::wallet_balances;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = wallet_balances)]
pub struct WalletBalance {
    pub id: i64,
    pub exchange: String,
    pub asset: String,
    pub wallet_type: String,
    pub wallet_id: String,
    pub balance: BigDecimal,
    pub locked_balance: BigDecimal,
    pub sequence: i64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = wallet_balances)]
pub struct NewWalletBalance {
    pub exchange: String,
    pub asset: String,
    pub wallet_type: String,
    pub wallet_id: String,
    pub balance: BigDecimal,
    pub locked_balance: BigDecimal,
    pub sequence: i64,
    pub timestamp: DateTime<Utc>,
}

impl NewWalletBalance {
    pub fn new(
        exchange: &str,
        asset: &str,
        wallet_type: &str,
        wallet_id: &str,
        balance: BigDecimal,
        locked_balance: BigDecimal,
        sequence: i64,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            exchange: exchange.to_string(),
            asset: asset.to_string(),
            wallet_type: wallet_type.to_string(),
            wallet_id: wallet_id.to_string(),
            balance,
            locked_balance,
            sequence,
            timestamp,
        }
    }

    pub fn from_snapshot(
        exchange: &str,
        asset: &str,
        wallet_type: &str,
        wallet_id: &str,
        balance: BigDecimal,
        locked_balance: BigDecimal,
        sequence: i64,
    ) -> Self {
        Self::new(
            exchange,
            asset,
            wallet_type,
            wallet_id,
            balance,
            locked_balance,
            sequence,
            Utc::now(),
        )
    }
}
