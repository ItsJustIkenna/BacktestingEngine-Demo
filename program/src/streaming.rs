//! Streaming data loader for memory-efficient backtesting
//!
//! Instead of loading all market data into memory at once, this module provides
//! iterators that fetch data in chunks from the database, reducing memory pressure
//! for large datasets (50K+ order book events).
//!
//! Only available with the `postgres` feature enabled.

#[cfg(feature = "postgres")]
use dataloader::MarketData;
#[cfg(feature = "postgres")]
use diesel::prelude::*;
#[cfg(feature = "postgres")]
use diesel_async::{AsyncPgConnection, RunQueryDsl};
#[cfg(feature = "postgres")]
use diesel_async::pooled_connection::deadpool;
#[cfg(feature = "postgres")]
use std::sync::Arc;
#[cfg(feature = "postgres")]
use bigdecimal::BigDecimal;
#[cfg(feature = "postgres")]
use chrono::{DateTime, Utc, NaiveDateTime};
#[cfg(feature = "postgres")]
use uuid::Uuid;
#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;

#[cfg(feature = "postgres")]
const DEFAULT_CHUNK_SIZE: usize = 5000; // Load 5K events at a time

#[cfg(feature = "postgres")]
/// Streaming iterator for order book events from database
pub struct OrderBookStreamIterator {
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
    symbol: String,
    exchange: String,
    chunk_size: usize,
    offset: i64,
    current_chunk: Vec<MarketData>,
    chunk_index: usize,
    total_loaded: usize,
    finished: bool,
}

#[cfg(feature = "postgres")]
impl OrderBookStreamIterator {
    pub fn new(
        pool: Arc<deadpool::Pool<AsyncPgConnection>>,
        symbol: String,
        exchange: String,
    ) -> Self {
        Self {
            pool,
            symbol,
            exchange,
            chunk_size: DEFAULT_CHUNK_SIZE,
            offset: 0,
            current_chunk: Vec::new(),
            chunk_index: 0,
            total_loaded: 0,
            finished: false,
        }
    }

    /// Load next chunk from database
    async fn load_next_chunk(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Nullable};

        #[derive(QueryableByName)]
        struct OrderEventRow {
            #[diesel(sql_type = Timestamptz)]
            timestamp: NaiveDateTime,
            #[diesel(sql_type = Varchar)]
            symbol: String,
            #[diesel(sql_type = Varchar)]
            exchange: String,
            #[diesel(sql_type = Varchar)]
            event_type: String,
            #[diesel(sql_type = Varchar)]
            side: String,
            #[diesel(sql_type = Numeric)]
            price_level: BigDecimal,
            #[diesel(sql_type = Numeric)]
            quantity: BigDecimal,
            #[diesel(sql_type = Varchar)]
            order_id: String,
            #[diesel(sql_type = Nullable<Numeric>)]
            prev_price: Option<BigDecimal>,
            #[diesel(sql_type = Nullable<Numeric>)]
            prev_quantity: Option<BigDecimal>,
            #[diesel(sql_type = Varchar)]
            status: String,
        }

        let mut conn = self.pool.get().await
            .map_err(|e| format!("Failed to get database connection: {}", e))?;

        // Query order book events with LIMIT/OFFSET for chunking
        let query = format!(
            "SELECT timestamp, symbol, exchange, event_type, side, price_level, quantity, \
             order_id, prev_price, prev_quantity, status \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' \
             AND event_type IN ('new', 'cancel', 'modify') \
             ORDER BY timestamp ASC LIMIT {} OFFSET {}",
            self.symbol, self.exchange, self.chunk_size, self.offset
        );

        let results: Vec<OrderEventRow> = diesel_async::RunQueryDsl::load(
            diesel::sql_query(&query),
            &mut conn
        )
        .await
        .map_err(|e| format!("Failed to load order events chunk: {}", e))?;

        if results.is_empty() {
            self.finished = true;
            return Ok(());
        }

        self.current_chunk = results
            .into_iter()
            .map(|row| {
                MarketData::Order(HistoricalOrder {
                    event_id: Uuid::new_v4(),
                    timestamp: DateTime::from_naive_utc_and_offset(row.timestamp, Utc),
                    order_id: row.order_id,
                    event_type: row.event_type,
                    side: row.side,
                    price_level: row.price_level,
                    quantity: row.quantity,
                    prev_price: row.prev_price,
                    prev_quantity: row.prev_quantity,
                    status: row.status,
                    exchange: row.exchange,
                    symbol: row.symbol,
                    security_id: Uuid::new_v4(),
                    exchange_id: Uuid::new_v4(),
                })
            })
            .collect();

        self.total_loaded += self.current_chunk.len();
        self.offset += self.chunk_size as i64;
        self.chunk_index = 0;

        Ok(())
    }

    pub fn total_loaded(&self) -> usize {
        self.total_loaded
    }
    
    /// Load all remaining data (async)
    pub async fn load_all(&mut self) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        let mut all_data = Vec::new();
        
        while !self.finished {
            self.load_next_chunk().await?;
            all_data.extend(self.current_chunk.drain(..));
        }
        
        Ok(all_data)
    }
}

// Removed sync Iterator trait - incompatible with async diesel
// Users should call load_all() instead

#[cfg(feature = "postgres")]
/// Batch processor for genetic algorithm optimization
/// Loads data once and creates references for parallel processing
pub struct OrderBookBatchLoader {
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
    symbol: String,
    exchange: String,
}

#[cfg(feature = "postgres")]
impl OrderBookBatchLoader {
    pub fn new(
        pool: Arc<deadpool::Pool<AsyncPgConnection>>,
        symbol: String,
        exchange: String,
    ) -> Self {
        Self {
            pool,
            symbol,
            exchange,
        }
    }

    /// Load data in chunks but keep in memory for genetic algorithm
    /// Uses sampling to reduce memory footprint
    pub async fn load_sampled(
        &self,
        sample_rate: f64, // 0.0-1.0, e.g., 0.5 = keep 50% of events
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = OrderBookStreamIterator::new(
            self.pool.clone(),
            self.symbol.clone(),
            self.exchange.clone(),
        );

        let all_data = stream.load_all().await?;
        let total = all_data.len();
        
        let sampled_data: Vec<MarketData> = all_data
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (*i as f64 * sample_rate) % 1.0 < sample_rate)
            .map(|(_, event)| event)
            .collect();

        println!("📊 Sampled {} events from {} total (rate: {:.1}%)", 
                 sampled_data.len(), total, sample_rate * 100.0);

        Ok(sampled_data)
    }

    /// Load data with max limit for genetic optimization
    pub async fn load_with_limit(
        &self,
        max_events: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = OrderBookStreamIterator::new(
            self.pool.clone(),
            self.symbol.clone(),
            self.exchange.clone(),
        );

        let mut data = stream.load_all().await?;
        data.truncate(max_events);

        println!("📊 Loaded {} events (limit: {})", data.len(), max_events);

        Ok(data)
    }
}


