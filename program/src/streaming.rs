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
use diesel_async::AsyncPgConnection;
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
use crate::{log_info, log_error, log_debug};
#[cfg(feature = "postgres")]
use crate::logging_facade::MAIN_LOGGER;

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

        // Use timeout to prevent indefinite blocking on pool exhaustion
        log_debug!(MAIN_LOGGER, "Getting database connection for chunk load (offset={})", self.offset);
        let mut conn = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.pool.get()
        ).await
            .map_err(|_| {
                log_error!(MAIN_LOGGER, "Connection pool timeout at offset {}", self.offset);
                "Connection pool timeout - possible deadlock or pool exhaustion"
            })?
            .map_err(|e| {
                log_error!(MAIN_LOGGER, "Failed to get database connection: {}", e);
                format!("Failed to get database connection: {}", e)
            })?;
        
        log_debug!(MAIN_LOGGER, "Got database connection, executing query (offset={})", self.offset);

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

        log_debug!(MAIN_LOGGER, "Executing query for {} {} (limit={}, offset={})", 
            self.symbol, self.exchange, self.chunk_size, self.offset);

        let results: Vec<OrderEventRow> = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| {
                log_error!(MAIN_LOGGER, "Query timeout loading order events for {} {} at offset {} (30s limit)", 
                    self.symbol, self.exchange, self.offset);
                "Query execution timeout - database may be overloaded"
            })?
            .map_err(|e| {
                log_error!(MAIN_LOGGER, "Query failed: {}", e);
                format!("Failed to load order events chunk: {}", e)
            })?;

        log_debug!(MAIN_LOGGER, "Query returned {} rows at offset {}", results.len(), self.offset);

        if results.is_empty() {
            log_debug!(MAIN_LOGGER, "No more results, finished loading");
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
        self.load_up_to(usize::MAX).await
    }
    
    /// Load data up to a maximum number of events (async)
    /// Stops loading once max_events is reached to prevent OOM
    pub async fn load_up_to(&mut self, max_events: usize) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        let mut all_data = Vec::new();
        
        while !self.finished && all_data.len() < max_events {
            self.load_next_chunk().await?;
            
            // Only take what we need to reach max_events
            let remaining = max_events.saturating_sub(all_data.len());
            let to_take = remaining.min(self.current_chunk.len());
            all_data.extend(self.current_chunk.drain(..to_take));
            
            if all_data.len() >= max_events {
                break;
            }
        }
        
        log_info!(MAIN_LOGGER, "Loaded {} order events (limit: {})", all_data.len(), max_events);
        
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

        log_info!(MAIN_LOGGER, "Sampled {} events from {} total (rate: {:.1}%)", 
                 sampled_data.len(), total, sample_rate * 100.0);

        Ok(sampled_data)
    }

    /// Load data with max limit for genetic optimization
    /// Includes BOTH order events AND trades, merged by timestamp
    pub async fn load_with_limit(
        &self,
        max_events: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        // Load order events
        let mut stream = OrderBookStreamIterator::new(
            self.pool.clone(),
            self.symbol.clone(),
            self.exchange.clone(),
        );
        let order_events = stream.load_up_to(max_events / 2).await?; // Split budget between orders and trades
        
        // Load trades
        let trades = self.load_trades(max_events / 2).await?;
        
        log_info!(MAIN_LOGGER, "Loaded {} order events and {} trades", order_events.len(), trades.len());
        
        // Merge by timestamp
        let mut merged = Vec::with_capacity(order_events.len() + trades.len());
        merged.extend(order_events);
        merged.extend(trades);
        
        // Sort by timestamp
        merged.sort_by(|a, b| {
            let ts_a = match a {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.timestamp,
            };
            let ts_b = match b {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.timestamp,
            };
            ts_a.cmp(&ts_b)
        });
        
        // Truncate to max_events if needed
        if merged.len() > max_events {
            merged.truncate(max_events);
        }
        
        log_info!(MAIN_LOGGER, "Total merged events: {} (limit: {})", merged.len(), max_events);

        Ok(merged)
    }
    
    /// Load trade events from trades table
    async fn load_trades(
        &self,
        max_trades: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Uuid as DieselUuid};
        use dataloader::TradeData;
        
        #[derive(QueryableByName)]
        struct TradeRow {
            #[diesel(sql_type = Timestamptz)]
            created_at: NaiveDateTime,
            #[diesel(sql_type = Varchar)]
            symbol: String,
            #[diesel(sql_type = Varchar)]
            exchange: String,
            #[diesel(sql_type = Varchar)]
            trade_id: String,
            #[diesel(sql_type = DieselUuid)]
            security_id: Uuid,
            #[diesel(sql_type = DieselUuid)]
            exchange_id: Uuid,
            #[diesel(sql_type = Varchar)]
            side: String,
            #[diesel(sql_type = Numeric)]
            price: BigDecimal,
            #[diesel(sql_type = Numeric)]
            quantity: BigDecimal,
        }
        
        let mut conn = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.pool.get()
        ).await
            .map_err(|_| "Connection pool timeout loading trades")?
            .map_err(|e| format!("Failed to get database connection: {}", e))?;
        
        let query = format!(
            "SELECT created_at, symbol, exchange, trade_id, security_id, exchange_id, side, price, quantity \
             FROM trades \
             WHERE symbol = '{}' AND exchange = '{}' \
             ORDER BY created_at ASC \
             LIMIT {}",
            self.symbol, self.exchange, max_trades
        );
        
        let results: Vec<TradeRow> = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| "Query timeout loading trades")?
            .map_err(|e| format!("Failed to load trades: {}", e))?;
        
        let trades: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Trade(TradeData {
                    timestamp: DateTime::from_naive_utc_and_offset(row.created_at, Utc),
                    symbol: row.symbol,
                    exchange: row.exchange,
                    trade_id: row.trade_id,
                    side: row.side,
                    price: row.price,
                    quantity: row.quantity,
                })
            })
            .collect();
        
        Ok(trades)
    }
}


