//! Streaming data loader for memory-efficient backtesting
//!
//! Instead of loading all market data into memory at once, this module provides
//! iterators that fetch data in chunks from the database, reducing memory pressure
//! for large datasets (50K+ order book events).

use dataloader::MarketData;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc, NaiveDateTime};
use uuid::Uuid;
use databaseschema::models::historical_order::HistoricalOrder;

const DEFAULT_CHUNK_SIZE: usize = 5000; // Load 5K events at a time

/// Streaming iterator for order book events from database
pub struct OrderBookStreamIterator {
    pool: Pool<ConnectionManager<PgConnection>>,
    symbol: String,
    exchange: String,
    chunk_size: usize,
    offset: i64,
    current_chunk: Vec<MarketData>,
    chunk_index: usize,
    total_loaded: usize,
    finished: bool,
}

impl OrderBookStreamIterator {
    pub fn new(
        pool: Pool<ConnectionManager<PgConnection>>,
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
    fn load_next_chunk(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Nullable, BigInt};

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

        let mut conn = self.pool.get()
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

        let results: Vec<OrderEventRow> = diesel::sql_query(&query)
            .load(&mut conn)
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
}

impl Iterator for OrderBookStreamIterator {
    type Item = MarketData;

    fn next(&mut self) -> Option<Self::Item> {
        // If current chunk is exhausted, load next chunk
        if self.chunk_index >= self.current_chunk.len() {
            if self.finished {
                return None;
            }

            if let Err(e) = self.load_next_chunk() {
                eprintln!("Error loading chunk: {}", e);
                return None;
            }

            if self.current_chunk.is_empty() {
                return None;
            }
        }

        let item = self.current_chunk[self.chunk_index].clone();
        self.chunk_index += 1;
        Some(item)
    }
}

/// Batch processor for genetic algorithm optimization
/// Loads data once and creates references for parallel processing
pub struct OrderBookBatchLoader {
    pool: Pool<ConnectionManager<PgConnection>>,
    symbol: String,
    exchange: String,
}

impl OrderBookBatchLoader {
    pub fn new(
        pool: Pool<ConnectionManager<PgConnection>>,
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

        let mut sampled_data = Vec::new();
        let mut count = 0;

        for event in &mut stream {
            // Sample events deterministically
            if (count as f64 * sample_rate) % 1.0 < sample_rate {
                sampled_data.push(event);
            }
            count += 1;
        }

        println!("📊 Sampled {} events from {} total (rate: {:.1}%)", 
                 sampled_data.len(), stream.total_loaded(), sample_rate * 100.0);

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

        let mut data = Vec::with_capacity(max_events);

        for event in stream.take(max_events) {
            data.push(event);
        }

        println!("📊 Loaded {} events (limit: {})", data.len(), max_events);

        Ok(data)
    }
}
