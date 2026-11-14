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
use crate::{log_debug, log_info_structured, log_warn_structured, log_error_structured};
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
                log_error_structured!(MAIN_LOGGER, "DB_CONNECTION_TIMEOUT",
                    "event" => "DB_CONNECTION_TIMEOUT",
                    "offset" => self.offset
                );
                "Connection pool timeout - possible deadlock or pool exhaustion"
            })?
            .map_err(|e| {
                log_error_structured!(MAIN_LOGGER, "DB_CONNECTION_FAILED",
                    "event" => "DB_CONNECTION_FAILED",
                    "error" => e.to_string()
                );
                format!("Failed to get database connection: {}", e)
            })?;
        
        log_debug!(MAIN_LOGGER, "Got database connection, executing query (offset={})", self.offset);

        // Query order book events with LIMIT/OFFSET for chunking
        // No event_type filter - simulation code handles all data types (L2/L3, Tardis/DataEngine)
        let query = format!(
            "SELECT timestamp, symbol, exchange, event_type, side, price_level, quantity, \
             order_id, prev_price, prev_quantity, status \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' \
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
                log_error_structured!(MAIN_LOGGER, "QUERY_TIMEOUT",
                    "event" => "QUERY_TIMEOUT",
                    "symbol" => &self.symbol,
                    "exchange" => &self.exchange,
                    "offset" => self.offset,
                    "timeout_secs" => 30
                );
                "Query execution timeout - database may be overloaded"
            })?
            .map_err(|e| {
                log_error_structured!(MAIN_LOGGER, "QUERY_FAILED",
                    "event" => "QUERY_FAILED",
                    "error" => e.to_string()
                );
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
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_ORDER_EVENTS",
            "event" => "DATA_LOAD_ORDER_EVENTS",
            "order_event_count" => all_data.len(),
            "limit" => max_events
        );
        
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
    /// NOTE: For stratified sampling, use databaseschema::ops::historical_order_ops::get_stratified_historical_orders
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

        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_SAMPLED",
            "event" => "DATA_LOAD_SAMPLED",
            "sampled_count" => sampled_data.len(),
            "total_count" => total,
            "sample_rate_pct" => format!("{:.1}", sample_rate * 100.0)
        );

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
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_ORDERS_AND_TRADES",
            "event" => "DATA_LOAD_ORDERS_AND_TRADES",
            "order_event_count" => order_events.len(),
            "trade_count" => trades.len()
        );
        
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
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_MERGED",
            "event" => "DATA_LOAD_MERGED",
            "merged_count" => merged.len(),
            "limit" => max_events
        );

        Ok(merged)
    }
    
    /// Load ALL order events and trades without any limit
    /// Use this for production backtesting where accuracy matters more than memory
    pub async fn load_all(
        &self,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        // Load ALL order events
        let mut stream = OrderBookStreamIterator::new(
            self.pool.clone(),
            self.symbol.clone(),
            self.exchange.clone(),
        );
        let order_events = stream.load_all().await?;
        
        // Load ALL trades
        let trades = self.load_all_trades().await?;
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_ALL_ORDERS_AND_TRADES",
            "event" => "DATA_LOAD_ALL_ORDERS_AND_TRADES",
            "order_event_count" => order_events.len(),
            "trade_count" => trades.len()
        );
        
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
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_MERGED_ALL",
            "event" => "DATA_LOAD_MERGED_ALL",
            "merged_count" => merged.len()
        );

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
        #[allow(dead_code)]
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
                MarketData::Trade(TradeData::new(
                    DateTime::from_naive_utc_and_offset(row.created_at, Utc),
                    row.symbol,
                    row.exchange,
                    row.price,
                    row.quantity,
                    row.side,
                    row.trade_id,
                ))
            })
            .collect();
        
        Ok(trades)
    }
    
    /// Load ALL trade events without limit
    async fn load_all_trades(
        &self,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Uuid as DieselUuid};
        use dataloader::TradeData;
        
        #[derive(QueryableByName)]
        #[allow(dead_code)]
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
        
        // No LIMIT - load ALL trades
        let query = format!(
            "SELECT created_at, symbol, exchange, trade_id, security_id, exchange_id, side, price, quantity \
             FROM trades \
             WHERE symbol = '{}' AND exchange = '{}' \
             ORDER BY created_at ASC",
            self.symbol, self.exchange
        );
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_ALL_TRADES_START",
            "event" => "DATA_LOAD_ALL_TRADES_START",
            "symbol" => &self.symbol,
            "exchange" => &self.exchange
        );
        
        let results: Vec<TradeRow> = tokio::time::timeout(
            std::time::Duration::from_secs(120), // Longer timeout for large datasets
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| "Query timeout loading all trades (120s limit)")?
            .map_err(|e| format!("Failed to load trades: {}", e))?;
        
        log_info_structured!(MAIN_LOGGER, "DATA_LOAD_TRADES_COMPLETE",
            "event" => "DATA_LOAD_TRADES_COMPLETE",
            "trade_count" => results.len()
        );
        
        let trades: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Trade(TradeData::new(
                    DateTime::from_naive_utc_and_offset(row.created_at, Utc),
                    row.symbol,
                    row.exchange,
                    row.price,
                    row.quantity,
                    row.side,
                    row.trade_id,
                ))
            })
            .collect();
        
        Ok(trades)
    }
    
    /// Load trade events from historical_orders table (for websocket-sourced data like Kraken)
    /// 
    /// This is useful when trades are stored in historical_orders with event_type='trade'
    /// rather than in the separate trades table.
    pub async fn load_trades_from_historical_orders(
        &self,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric};
        use dataloader::TradeData;
        use bigdecimal::ToPrimitive;
        
        #[derive(QueryableByName)]
        #[allow(dead_code)]
        struct HistoricalTradeRow {
            #[diesel(sql_type = Timestamptz)]
            timestamp: NaiveDateTime,
            #[diesel(sql_type = Varchar)]
            symbol: String,
            #[diesel(sql_type = Varchar)]
            exchange: String,
            #[diesel(sql_type = Varchar)]
            order_id: String,
            #[diesel(sql_type = Varchar)]
            side: String,
            #[diesel(sql_type = Numeric)]
            price_level: BigDecimal,
            #[diesel(sql_type = Numeric)]
            quantity: BigDecimal,
        }
        
        let mut conn = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.pool.get()
        ).await
            .map_err(|_| "Connection pool timeout loading trades from historical_orders")?
            .map_err(|e| format!("Failed to get database connection: {}", e))?;
        
        // Query historical_orders for trade events only
        let query = format!(
            "SELECT timestamp, symbol, exchange, order_id, side, price_level, quantity \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' AND event_type = 'trade' \
             ORDER BY timestamp ASC",
            self.symbol, self.exchange
        );
        
        eprintln!("      🔍 load_trades_from_historical_orders: querying for {}/{}", self.symbol, self.exchange);
        
        let results: Vec<HistoricalTradeRow> = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| "Query timeout loading trades from historical_orders (120s limit)")?
            .map_err(|e| format!("Failed to load trades from historical_orders: {}", e))?;
        
        eprintln!("      📊 load_trades_from_historical_orders: found {} trades", results.len());
        
        let trades: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Trade(TradeData::new(
                    DateTime::from_naive_utc_and_offset(row.timestamp, Utc),
                    row.symbol,
                    row.exchange,
                    row.price_level,
                    row.quantity,
                    row.side,
                    row.order_id,
                ))
            })
            .collect();
        
        Ok(trades)
    }
}

// ============================================================================
// STREAMING DATA LOADER FOR DISTRIBUTED WORKERS
// ============================================================================

#[cfg(feature = "postgres")]
/// Streaming data loader for distributed backtest workers
/// 
/// This loader efficiently loads data for a specific time range,
/// supporting cursor-based pagination to avoid memory spikes.
pub struct StreamingDataLoader {
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
}

#[cfg(feature = "postgres")]
impl StreamingDataLoader {
    /// Create a new streaming data loader
    pub async fn new(db_url: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use diesel_async::pooled_connection::AsyncDieselConnectionManager;
        
        let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(db_url);
        let pool = deadpool::Pool::builder(config)
            .max_size(4)
            .build()
            .map_err(|e| format!("Failed to create connection pool: {}", e))?;
        
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
    
    /// Create from existing pool
    pub fn from_pool(pool: Arc<deadpool::Pool<AsyncPgConnection>>) -> Self {
        Self { pool }
    }
    
    /// Stream events for a time range
    /// 
    /// Returns a DataStream that yields batches of events.
    /// Memory efficient: only one batch in memory at a time.
    pub async fn stream_events(
        &self,
        symbol: &str,
        exchange: &str,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
        batch_size: usize,
    ) -> Result<DataStream, Box<dyn std::error::Error + Send + Sync>> {
        // For now, we load all at once but in batches
        // A true streaming implementation would use async_stream
        
        let mut all_batches = Vec::new();
        let mut offset = 0i64;
        
        loop {
            let batch = self.load_batch(symbol, exchange, start, end, batch_size, offset).await?;
            
            if batch.is_empty() {
                break;
            }
            
            let batch_len = batch.len();
            all_batches.push(Ok(batch));
            offset += batch_len as i64;
            
            // Safety limit
            if all_batches.len() > 10000 {
                log_warn_structured!(MAIN_LOGGER, "BATCH_LIMIT_REACHED",
                    "event" => "BATCH_LIMIT_REACHED",
                    "batch_count" => 10000
                );
                break;
            }
        }
        
        Ok(DataStream {
            batches: all_batches,
            current: 0,
        })
    }
    
    /// Load a single batch of events
    async fn load_batch(
        &self,
        symbol: &str,
        exchange: &str,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
        batch_size: usize,
        offset: i64,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
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
        
        let mut conn = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.pool.get()
        ).await
            .map_err(|_| "Connection pool timeout")?
            .map_err(|e| format!("Failed to get connection: {}", e))?;
        
        // No event_type filter - simulation code handles all data types (L2/L3, Tardis/DataEngine)
        let query = format!(
            "SELECT timestamp, symbol, exchange, event_type, side, price_level, quantity, \
             order_id, prev_price, prev_quantity, status \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' \
             AND timestamp >= '{}' AND timestamp < '{}' \
             ORDER BY timestamp ASC \
             LIMIT {} OFFSET {}",
            symbol, exchange, 
            start.to_rfc3339(), end.to_rfc3339(),
            batch_size, offset
        );
        
        let results: Vec<OrderEventRow> = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| "Query timeout")?
            .map_err(|e| format!("Query failed: {}", e))?;
        
        let events: Vec<MarketData> = results
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
        
        Ok(events)
    }
    
    /// Load events for a time range with a maximum limit
    pub async fn load_events_with_limit(
        &self,
        symbol: &str,
        exchange: &str,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
        max_events: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        let batch_size = 50_000;
        let mut all_events = Vec::new();
        let mut offset = 0i64;
        
        while all_events.len() < max_events {
            let remaining = max_events - all_events.len();
            let this_batch = remaining.min(batch_size);
            
            let batch = self.load_batch(symbol, exchange, start, end, this_batch, offset).await?;
            
            if batch.is_empty() {
                break;
            }
            
            offset += batch.len() as i64;
            all_events.extend(batch);
        }
        
        Ok(all_events)
    }
}

#[cfg(feature = "postgres")]
/// Simple async-like stream for batch data loading
pub struct DataStream {
    batches: Vec<Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>>>,
    current: usize,
}

#[cfg(feature = "postgres")]
impl DataStream {
    /// Get next batch
    pub async fn next(&mut self) -> Option<Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>>> {
        if self.current < self.batches.len() {
            // Remove from front to free memory
            let result = self.batches.remove(0);
            Some(result)
        } else {
            None
        }
    }
    
    /// Check if stream is exhausted
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }
}

// ============================================================================
// PARALLEL STREAMING LOADER
// ============================================================================

#[cfg(feature = "postgres")]
/// Parallel data loader that uses multiple connections to load data concurrently
/// 
/// Instead of sequential OFFSET/LIMIT queries, this divides the data into
/// time-based partitions and loads them in parallel using separate connections.
pub struct ParallelDataLoader {
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
    num_workers: usize,
}

#[cfg(feature = "postgres")]
impl ParallelDataLoader {
    /// Create a new parallel loader with specified number of workers
    pub fn new(pool: Arc<deadpool::Pool<AsyncPgConnection>>, num_workers: usize) -> Self {
        Self {
            pool,
            num_workers: num_workers.max(1).min(20), // Clamp between 1-20
        }
    }
    
    /// Load all historical orders in parallel using time-based partitioning
    pub async fn load_all_orders(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;
        
        let start_time = Instant::now();
        eprintln!("          📥 load_all_orders: sym={} ex={} workers={}", symbol, exchange, self.num_workers);
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_START",
            "event" => "PARALLEL_LOAD_START",
            "worker_count" => self.num_workers,
            "symbol" => symbol,
            "exchange" => exchange
        );
        
        // First, get the time range of the data
        let (min_ts, max_ts) = self.get_time_range(symbol, exchange).await?;
        
        if min_ts.is_none() || max_ts.is_none() {
            eprintln!("          ⚠️ load_all_orders: NO TIME RANGE FOUND - no data in historical_orders for sym={} ex={}", symbol, exchange);
            log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_NO_DATA",
                "event" => "PARALLEL_LOAD_NO_DATA",
                "symbol" => symbol,
                "exchange" => exchange
            );
            return Ok(Vec::new());
        }
        
        let min_ts = min_ts.unwrap();
        let max_ts = max_ts.unwrap();
        
        eprintln!("          📅 load_all_orders: time range {} to {}", min_ts, max_ts);
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_DATA_RANGE",
            "event" => "PARALLEL_LOAD_DATA_RANGE",
            "min_timestamp" => min_ts.to_string(),
            "max_timestamp" => max_ts.to_string()
        );
        
        // Divide time range into partitions
        let duration = max_ts.signed_duration_since(min_ts);
        let partition_duration = duration / (self.num_workers as i32);
        
        // Create partition ranges
        let mut partitions = Vec::with_capacity(self.num_workers);
        let mut current_start = min_ts;
        
        for i in 0..self.num_workers {
            let current_end = if i == self.num_workers - 1 {
                max_ts + chrono::Duration::seconds(1) // Include last record
            } else {
                current_start + partition_duration
            };
            partitions.push((current_start, current_end));
            current_start = current_end;
        }
        
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_PARTITIONS",
            "event" => "PARALLEL_LOAD_PARTITIONS",
            "partition_count" => partitions.len()
        );
        
        // Load each partition in parallel
        let symbol = symbol.to_string();
        let exchange = exchange.to_string();
        let pool = self.pool.clone();
        
        let mut handles = Vec::with_capacity(self.num_workers);
        
        for (idx, (start, end)) in partitions.into_iter().enumerate() {
            let pool = pool.clone();
            let symbol = symbol.clone();
            let exchange = exchange.clone();
            
            let handle = tokio::spawn(async move {
                Self::load_partition(pool, &symbol, &exchange, start, end, idx).await
            });
            handles.push(handle);
        }
        
        // Collect results
        let mut all_events: Vec<MarketData> = Vec::new();
        let mut partition_counts = Vec::new();
        
        for (idx, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(events)) => {
                    partition_counts.push((idx, events.len()));
                    all_events.extend(events);
                }
                Ok(Err(e)) => {
                    log_error_structured!(MAIN_LOGGER, "PARTITION_FAILED",
                        "event" => "PARTITION_FAILED",
                        "partition_idx" => idx,
                        "error" => e.to_string()
                    );
                    return Err(e);
                }
                Err(e) => {
                    log_error_structured!(MAIN_LOGGER, "PARTITION_PANICKED",
                        "event" => "PARTITION_PANICKED",
                        "partition_idx" => idx,
                        "error" => e.to_string()
                    );
                    return Err(format!("Task panicked: {}", e).into());
                }
            }
        }
        
        // Sort by timestamp (partitions may have slight overlap at boundaries)
        all_events.sort_by(|a, b| {
            let ts_a = match a {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            let ts_b = match b {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            ts_a.cmp(&ts_b)
        });
        
        let elapsed = start_time.elapsed();
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_COMPLETE",
            "event" => "PARALLEL_LOAD_COMPLETE",
            "order_count" => all_events.len(),
            "elapsed_secs" => format!("{:.2}", elapsed.as_secs_f64()),
            "rows_per_sec" => format!("{:.0}", all_events.len() as f64 / elapsed.as_secs_f64())
        );
        
        for (idx, count) in partition_counts {
            log_debug!(MAIN_LOGGER, "  Partition {}: {} rows", idx, count);
        }
        
        Ok(all_events)
    }
    
    /// Load all trades in parallel
    pub async fn load_all_trades(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;
        
        let start_time = Instant::now();
        log_info_structured!(MAIN_LOGGER, "PARALLEL_TRADE_LOAD_START",
            "event" => "PARALLEL_TRADE_LOAD_START",
            "worker_count" => self.num_workers
        );
        
        // Get time range for trades
        let (min_ts, max_ts) = self.get_trade_time_range(symbol, exchange).await?;
        
        if min_ts.is_none() || max_ts.is_none() {
            log_info_structured!(MAIN_LOGGER, "PARALLEL_TRADE_LOAD_NO_DATA",
                "event" => "PARALLEL_TRADE_LOAD_NO_DATA",
                "symbol" => symbol,
                "exchange" => exchange
            );
            return Ok(Vec::new());
        }
        
        let min_ts = min_ts.unwrap();
        let max_ts = max_ts.unwrap();
        
        // Divide into partitions
        let duration = max_ts.signed_duration_since(min_ts);
        let partition_duration = duration / (self.num_workers as i32);
        
        let mut partitions = Vec::with_capacity(self.num_workers);
        let mut current_start = min_ts;
        
        for i in 0..self.num_workers {
            let current_end = if i == self.num_workers - 1 {
                max_ts + chrono::Duration::seconds(1)
            } else {
                current_start + partition_duration
            };
            partitions.push((current_start, current_end));
            current_start = current_end;
        }
        
        // Load partitions in parallel
        let symbol = symbol.to_string();
        let exchange = exchange.to_string();
        let pool = self.pool.clone();
        
        let mut handles = Vec::with_capacity(self.num_workers);
        
        for (idx, (start, end)) in partitions.into_iter().enumerate() {
            let pool = pool.clone();
            let symbol = symbol.clone();
            let exchange = exchange.clone();
            
            let handle = tokio::spawn(async move {
                Self::load_trade_partition(pool, &symbol, &exchange, start, end, idx).await
            });
            handles.push(handle);
        }
        
        // Collect results
        let mut all_trades: Vec<MarketData> = Vec::new();
        
        for (idx, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(trades)) => {
                    all_trades.extend(trades);
                }
                Ok(Err(e)) => {
                    log_error_structured!(MAIN_LOGGER, "TRADE_PARTITION_FAILED",
                        "event" => "TRADE_PARTITION_FAILED",
                        "partition_idx" => idx,
                        "error" => e.to_string()
                    );
                    return Err(e);
                }
                Err(e) => {
                    log_error_structured!(MAIN_LOGGER, "TRADE_PARTITION_PANICKED",
                        "event" => "TRADE_PARTITION_PANICKED",
                        "partition_idx" => idx,
                        "error" => e.to_string()
                    );
                    return Err(format!("Task panicked: {}", e).into());
                }
            }
        }
        
        // Sort by timestamp
        all_trades.sort_by(|a, b| {
            let ts_a = match a {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            let ts_b = match b {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            ts_a.cmp(&ts_b)
        });
        
        let elapsed = start_time.elapsed();
        log_info_structured!(MAIN_LOGGER, "PARALLEL_TRADE_LOAD_COMPLETE",
            "event" => "PARALLEL_TRADE_LOAD_COMPLETE",
            "trade_count" => all_trades.len(),
            "elapsed_secs" => format!("{:.2}", elapsed.as_secs_f64())
        );
        
        Ok(all_trades)
    }
    
    /// Load trades from historical_orders table (for websocket-sourced data like Kraken)
    /// Queries historical_orders WHERE event_type = 'trade'
    pub async fn load_trades_from_historical_orders(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;
        use dataloader::TradeData;
        
        let start_time = Instant::now();
        eprintln!("        📥 load_trades_from_historical_orders: sym={} ex={}", symbol, exchange);
        
        // Get time range for trades in historical_orders
        let (min_ts, max_ts) = self.get_trade_time_range_from_historical_orders(symbol, exchange).await?;
        
        if min_ts.is_none() || max_ts.is_none() {
            eprintln!("        ⚠️ No trades found in historical_orders for {}/{}", symbol, exchange);
            return Ok(Vec::new());
        }
        
        let min_ts = min_ts.unwrap();
        let max_ts = max_ts.unwrap();
        
        eprintln!("        📅 Trade time range: {} to {}", min_ts, max_ts);
        
        // For now, use a simple single query since trade events are usually fewer than order events
        let mut conn = self.pool.get().await
            .map_err(|e| format!("Failed to get connection: {}", e))?;
        
        let query = format!(
            "SELECT timestamp, symbol, exchange, order_id, side, price_level, quantity \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' AND event_type = 'trade' \
             ORDER BY timestamp ASC",
            symbol, exchange
        );
        
        #[derive(diesel::QueryableByName)]
        #[allow(dead_code)]
        struct HistoricalTradeRow {
            #[diesel(sql_type = diesel::sql_types::Timestamptz)]
            timestamp: NaiveDateTime,
            #[diesel(sql_type = diesel::sql_types::Varchar)]
            symbol: String,
            #[diesel(sql_type = diesel::sql_types::Varchar)]
            exchange: String,
            #[diesel(sql_type = diesel::sql_types::Varchar)]
            order_id: String,
            #[diesel(sql_type = diesel::sql_types::Varchar)]
            side: String,
            #[diesel(sql_type = diesel::sql_types::Numeric)]
            price_level: BigDecimal,
            #[diesel(sql_type = diesel::sql_types::Numeric)]
            quantity: BigDecimal,
        }
        
        let results: Vec<HistoricalTradeRow> = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            diesel_async::RunQueryDsl::load(diesel::sql_query(&query), &mut conn)
        ).await
            .map_err(|_| "Query timeout loading trades from historical_orders")?
            .map_err(|e| format!("Failed to load trades from historical_orders: {}", e))?;
        
        let trades: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Trade(TradeData::new(
                    DateTime::from_naive_utc_and_offset(row.timestamp, Utc),
                    row.symbol,
                    row.exchange,
                    row.price_level,
                    row.quantity,
                    row.side,
                    row.order_id,
                ))
            })
            .collect();
        
        let elapsed = start_time.elapsed();
        eprintln!("        ✅ Loaded {} trades from historical_orders in {:.2}s", trades.len(), elapsed.as_secs_f64());
        
        Ok(trades)
    }
    
    /// Get min/max timestamp for trade events in historical_orders
    async fn get_trade_time_range_from_historical_orders(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>), Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Nullable, Timestamptz};
        
        #[derive(diesel::QueryableByName)]
        struct TimeRange {
            #[diesel(sql_type = Nullable<Timestamptz>)]
            min_ts: Option<NaiveDateTime>,
            #[diesel(sql_type = Nullable<Timestamptz>)]
            max_ts: Option<NaiveDateTime>,
        }
        
        let mut conn = self.pool.get().await
            .map_err(|e| format!("Failed to get connection: {}", e))?;
        
        let query = format!(
            "SELECT MIN(timestamp) as min_ts, MAX(timestamp) as max_ts \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' AND event_type = 'trade'",
            symbol, exchange
        );
        
        let result: TimeRange = diesel_async::RunQueryDsl::get_result(
            diesel::sql_query(&query),
            &mut conn
        ).await.map_err(|e| format!("Failed to get trade time range: {}", e))?;
        
        Ok((
            result.min_ts.map(|ts| DateTime::from_naive_utc_and_offset(ts, Utc)),
            result.max_ts.map(|ts| DateTime::from_naive_utc_and_offset(ts, Utc)),
        ))
    }
    
    /// Load both orders and trades in parallel, merge by timestamp
    pub async fn load_all(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;
        
        let start_time = Instant::now();
        eprintln!("        📥 ParallelDataLoader::load_all sym={} ex={}", symbol, exchange);
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_ALL_START",
            "event" => "PARALLEL_LOAD_ALL_START",
            "symbol" => symbol,
            "exchange" => exchange
        );
        
        // Load orders and trades concurrently
        let orders_future = self.load_all_orders(symbol, exchange);
        let trades_future = self.load_all_trades(symbol, exchange);
        
        let (orders_result, trades_result) = tokio::join!(orders_future, trades_future);
        
        let orders = orders_result?;
        let trades = trades_result?;
        
        eprintln!("        📊 ParallelDataLoader: orders={} trades={}", orders.len(), trades.len());
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_ORDERS_TRADES_COUNTS",
            "event" => "PARALLEL_LOAD_ORDERS_TRADES_COUNTS",
            "order_count" => orders.len(),
            "trade_count" => trades.len()
        );
        
        // Merge and sort
        let mut merged = Vec::with_capacity(orders.len() + trades.len());
        merged.extend(orders);
        merged.extend(trades);
        
        merged.sort_by(|a, b| {
            let ts_a = match a {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            let ts_b = match b {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            ts_a.cmp(&ts_b)
        });
        
        let elapsed = start_time.elapsed();
        eprintln!("        ✅ ParallelDataLoader: merged {} events in {:.2}s", merged.len(), elapsed.as_secs_f64());
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_ALL_COMPLETE",
            "event" => "PARALLEL_LOAD_ALL_COMPLETE",
            "total_event_count" => merged.len(),
            "elapsed_secs" => format!("{:.2}", elapsed.as_secs_f64()),
            "rows_per_sec" => format!("{:.0}", merged.len() as f64 / elapsed.as_secs_f64())
        );
        
        Ok(merged)
    }
    
    /// Load orders and trades with a maximum event limit to prevent OOM
    /// Distributes the budget between orders and trades, then truncates to limit
    pub async fn load_with_limit(
        &self,
        symbol: &str,
        exchange: &str,
        max_events: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::Instant;
        
        let start_time = Instant::now();
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_WITH_LIMIT_START",
            "event" => "PARALLEL_LOAD_WITH_LIMIT_START",
            "symbol" => symbol,
            "exchange" => exchange,
            "max_events" => max_events
        );
        
        // Load orders and trades concurrently
        let orders_future = self.load_all_orders(symbol, exchange);
        let trades_future = self.load_all_trades(symbol, exchange);
        
        let (orders_result, trades_result) = tokio::join!(orders_future, trades_future);
        
        let orders = orders_result?;
        let trades = trades_result?;
        
        let total_loaded = orders.len() + trades.len();
        
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_WITH_LIMIT_RAW_COUNTS",
            "event" => "PARALLEL_LOAD_WITH_LIMIT_RAW_COUNTS",
            "order_count" => orders.len(),
            "trade_count" => trades.len(),
            "total_loaded" => total_loaded,
            "max_events" => max_events
        );
        
        // Merge and sort
        let mut merged = Vec::with_capacity(orders.len().min(max_events) + trades.len().min(max_events));
        merged.extend(orders);
        merged.extend(trades);
        
        merged.sort_by(|a, b| {
            let ts_a = match a {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            let ts_b = match b {
                MarketData::Order(o) => o.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
            };
            ts_a.cmp(&ts_b)
        });
        
        // Truncate to max_events to prevent OOM
        if merged.len() > max_events {
            log_warn!(MAIN_LOGGER, "Truncating {} events to {} to prevent OOM", merged.len(), max_events);
            merged.truncate(max_events);
            // Shrink capacity to free excess memory
            merged.shrink_to_fit();
        }
        
        let elapsed = start_time.elapsed();
        log_info_structured!(MAIN_LOGGER, "PARALLEL_LOAD_WITH_LIMIT_COMPLETE",
            "event" => "PARALLEL_LOAD_WITH_LIMIT_COMPLETE",
            "total_event_count" => merged.len(),
            "events_truncated" => total_loaded > max_events,
            "events_dropped" => total_loaded.saturating_sub(merged.len()),
            "elapsed_secs" => format!("{:.2}", elapsed.as_secs_f64()),
            "rows_per_sec" => format!("{:.0}", merged.len() as f64 / elapsed.as_secs_f64())
        );
        
        Ok(merged)
    }
    
    /// Get min/max timestamp for orders
    async fn get_time_range(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>), Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Nullable, Timestamptz};
        
        #[derive(QueryableByName)]
        struct TimeRange {
            #[diesel(sql_type = Nullable<Timestamptz>)]
            min_ts: Option<NaiveDateTime>,
            #[diesel(sql_type = Nullable<Timestamptz>)]
            max_ts: Option<NaiveDateTime>,
        }
        
        let mut conn = self.pool.get().await
            .map_err(|e| format!("Failed to get connection: {}", e))?;
        
        // No event_type filter - include all data types for time range calculation
        let query = format!(
            "SELECT MIN(timestamp) as min_ts, MAX(timestamp) as max_ts \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}'",
            symbol, exchange
        );
        
        eprintln!("            🔍 get_time_range SQL: SELECT MIN/MAX FROM historical_orders WHERE symbol='{}' AND exchange='{}'", symbol, exchange);
        
        let result: TimeRange = diesel_async::RunQueryDsl::get_result(
            diesel::sql_query(&query),
            &mut conn
        ).await.map_err(|e| format!("Time range query failed: {}", e))?;
        
        eprintln!("            📅 get_time_range result: min={:?} max={:?}", result.min_ts, result.max_ts);
        
        Ok((
            result.min_ts.map(|t| DateTime::from_naive_utc_and_offset(t, Utc)),
            result.max_ts.map(|t| DateTime::from_naive_utc_and_offset(t, Utc)),
        ))
    }
    
    /// Get min/max timestamp for trades
    async fn get_trade_time_range(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>), Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Nullable, Timestamptz};
        
        #[derive(QueryableByName)]
        struct TimeRange {
            #[diesel(sql_type = Nullable<Timestamptz>)]
            min_ts: Option<NaiveDateTime>,
            #[diesel(sql_type = Nullable<Timestamptz>)]
            max_ts: Option<NaiveDateTime>,
        }
        
        let mut conn = self.pool.get().await
            .map_err(|e| format!("Failed to get connection: {}", e))?;
        
        let query = format!(
            "SELECT MIN(created_at) as min_ts, MAX(created_at) as max_ts \
             FROM trades \
             WHERE symbol = '{}' AND exchange = '{}'",
            symbol, exchange
        );
        
        let result: TimeRange = diesel_async::RunQueryDsl::get_result(
            diesel::sql_query(&query),
            &mut conn
        ).await.map_err(|e| format!("Trade time range query failed: {}", e))?;
        
        Ok((
            result.min_ts.map(|t| DateTime::from_naive_utc_and_offset(t, Utc)),
            result.max_ts.map(|t| DateTime::from_naive_utc_and_offset(t, Utc)),
        ))
    }
    
    /// Load a single partition of orders
    async fn load_partition(
        pool: Arc<deadpool::Pool<AsyncPgConnection>>,
        symbol: &str,
        exchange: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        partition_idx: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Nullable};
        
        #[derive(QueryableByName)]
        struct OrderRow {
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
        
        log_debug!(MAIN_LOGGER, "Partition {} loading: {} to {}", partition_idx, start, end);
        
        let mut conn = pool.get().await
            .map_err(|e| format!("Partition {} connection failed: {}", partition_idx, e))?;
        
        // No event_type filter - simulation code handles all data types (L2/L3, Tardis/DataEngine)
        let query = format!(
            "SELECT timestamp, symbol, exchange, event_type, side, price_level, quantity, \
             order_id, prev_price, prev_quantity, status \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' \
             AND timestamp >= '{}' AND timestamp < '{}' \
             ORDER BY timestamp ASC",
            symbol, exchange,
            start.to_rfc3339(), end.to_rfc3339()
        );
        
        let results: Vec<OrderRow> = diesel_async::RunQueryDsl::load(
            diesel::sql_query(&query),
            &mut conn
        ).await.map_err(|e| format!("Partition {} query failed: {}", partition_idx, e))?;
        
        log_debug!(MAIN_LOGGER, "Partition {} loaded {} rows", partition_idx, results.len());
        
        let events: Vec<MarketData> = results
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
        
        Ok(events)
    }
    
    /// Load a single partition of trades
    async fn load_trade_partition(
        pool: Arc<deadpool::Pool<AsyncPgConnection>>,
        symbol: &str,
        exchange: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        partition_idx: usize,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::sql_types::{Timestamptz, Varchar, Numeric};
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
            #[diesel(sql_type = Varchar)]
            side: String,
            #[diesel(sql_type = Numeric)]
            price: BigDecimal,
            #[diesel(sql_type = Numeric)]
            quantity: BigDecimal,
        }
        
        log_debug!(MAIN_LOGGER, "Trade partition {} loading: {} to {}", partition_idx, start, end);
        
        let mut conn = pool.get().await
            .map_err(|e| format!("Trade partition {} connection failed: {}", partition_idx, e))?;
        
        let query = format!(
            "SELECT created_at, symbol, exchange, trade_id, side, price, quantity \
             FROM trades \
             WHERE symbol = '{}' AND exchange = '{}' \
             AND created_at >= '{}' AND created_at < '{}' \
             ORDER BY created_at ASC",
            symbol, exchange,
            start.to_rfc3339(), end.to_rfc3339()
        );
        
        let results: Vec<TradeRow> = diesel_async::RunQueryDsl::load(
            diesel::sql_query(&query),
            &mut conn
        ).await.map_err(|e| format!("Trade partition {} query failed: {}", partition_idx, e))?;
        
        log_debug!(MAIN_LOGGER, "Trade partition {} loaded {} rows", partition_idx, results.len());
        
        let trades: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Trade(TradeData::new(
                    DateTime::from_naive_utc_and_offset(row.created_at, Utc),
                    row.symbol,
                    row.exchange,
                    row.price,
                    row.quantity,
                    row.side,
                    row.trade_id,
                ))
            })
            .collect();
        
        Ok(trades)
    }
}
