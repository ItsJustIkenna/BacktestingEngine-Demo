//! Database streaming for efficient data loading
//!
//! Uses cursor-based pagination to read data in chunks without
//! loading everything into memory at once.

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::stream::{self, Stream};

#[cfg(feature = "postgres")]
use diesel::prelude::*;
#[cfg(feature = "postgres")]
use diesel_async::{AsyncPgConnection, RunQueryDsl};
#[cfg(feature = "postgres")]
use diesel_async::pooled_connection::deadpool::Pool;
#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;
#[cfg(feature = "postgres")]
use databaseschema::schema::historical_orders;

/// Streams data from database in chunks
#[cfg(feature = "postgres")]
pub struct DatabaseStreamer {
    pool: Pool<AsyncPgConnection>,
    chunk_size: usize,
}

#[cfg(feature = "postgres")]
impl DatabaseStreamer {
    /// Create a new database streamer
    pub fn new(pool: Pool<AsyncPgConnection>, chunk_size: usize) -> Self {
        Self { pool, chunk_size }
    }
    
    /// Stream historical orders for a symbol/exchange
    /// 
    /// Returns an async stream that yields chunks of orders.
    /// Uses cursor-based pagination (timestamp + id) for efficiency.
    pub fn stream_orders<'a>(
        &'a self,
        symbol: &'a str,
        exchange: &'a str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> impl Stream<Item = Result<Vec<HistoricalOrder>>> + 'a {
        let chunk_size = self.chunk_size;
        
        stream::unfold(
            StreamState {
                pool: self.pool.clone(),
                symbol: symbol.to_string(),
                exchange: exchange.to_string(),
                start_time,
                end_time,
                last_timestamp: start_time,
                chunk_size,
                done: false,
            },
            |mut state| async move {
                if state.done {
                    return None;
                }
                
                match state.fetch_next_chunk().await {
                    Ok(orders) => {
                        if orders.is_empty() {
                            state.done = true;
                            return None;
                        }
                        
                        // Update cursor for next iteration
                        if let Some(last) = orders.last() {
                            state.last_timestamp = Some(last.timestamp);
                        }
                        
                        // Check if we got fewer than requested (end of data)
                        if orders.len() < state.chunk_size {
                            state.done = true;
                        }
                        
                        Some((Ok(orders), state))
                    }
                    Err(e) => {
                        state.done = true;
                        Some((Err(e), state))
                    }
                }
            },
        )
    }
    
    /// Count total rows (for progress reporting)
    /// Only counts trade events, not order book events
    pub async fn count_orders(
        &self,
        symbol: &str,
        exchange: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64> {
        use diesel::dsl::count_star;
        
        let mut conn = self.pool.get().await?;
        
        let mut query = historical_orders::table
            .filter(historical_orders::symbol.eq(symbol))
            .filter(historical_orders::exchange.eq(exchange))
            .filter(historical_orders::event_type.eq("trade"))
            .select(count_star())
            .into_boxed();
        
        if let Some(start) = start_time {
            query = query.filter(historical_orders::timestamp.ge(start));
        }
        if let Some(end) = end_time {
            query = query.filter(historical_orders::timestamp.lt(end));
        }
        
        let count: i64 = query.first(&mut conn).await?;
        Ok(count)
    }
}

#[cfg(feature = "postgres")]
struct StreamState {
    pool: Pool<AsyncPgConnection>,
    symbol: String,
    exchange: String,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    last_timestamp: Option<DateTime<Utc>>,
    chunk_size: usize,
    done: bool,
}

#[cfg(feature = "postgres")]
impl StreamState {
    async fn fetch_next_chunk(&self) -> Result<Vec<HistoricalOrder>> {
        let mut conn = self.pool.get().await?;
        
        let mut query = historical_orders::table
            .filter(historical_orders::symbol.eq(&self.symbol))
            .filter(historical_orders::exchange.eq(&self.exchange))
            // Only include trade events - order book events (new/cancel/modify) 
            // don't represent actual executions and would give false fill signals
            .filter(historical_orders::event_type.eq("trade"))
            .order(historical_orders::timestamp.asc())
            .limit(self.chunk_size as i64)
            .into_boxed();
        
        // Cursor: start from last timestamp (exclusive for subsequent chunks)
        if let Some(ts) = self.last_timestamp {
            // Use > for subsequent chunks to avoid duplicates
            // For first chunk (when last_timestamp == start_time), use >=
            if self.start_time == self.last_timestamp {
                query = query.filter(historical_orders::timestamp.ge(ts));
            } else {
                query = query.filter(historical_orders::timestamp.gt(ts));
            }
        }
        
        if let Some(end) = self.end_time {
            query = query.filter(historical_orders::timestamp.lt(end));
        }
        
        let orders: Vec<HistoricalOrder> = query.load(&mut conn).await?;
        Ok(orders)
    }
}

#[cfg(feature = "postgres")]
impl DatabaseStreamer {
    /// Stream ALL order book events (new, modify, cancel, trade) for realistic simulation
    /// 
    /// Unlike stream_orders which only returns trades, this returns all event types
    /// needed to reconstruct the full order book state.
    pub fn stream_all_events<'a>(
        &'a self,
        symbol: &'a str,
        exchange: &'a str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> impl Stream<Item = Result<Vec<HistoricalOrder>>> + 'a {
        let chunk_size = self.chunk_size;
        
        stream::unfold(
            AllEventsStreamState {
                pool: self.pool.clone(),
                symbol: symbol.to_string(),
                exchange: exchange.to_string(),
                start_time,
                end_time,
                last_timestamp: start_time,
                chunk_size,
                done: false,
            },
            |mut state| async move {
                if state.done {
                    return None;
                }
                
                match state.fetch_next_chunk().await {
                    Ok(orders) => {
                        if orders.is_empty() {
                            state.done = true;
                            return None;
                        }
                        
                        // Update cursor for next iteration
                        if let Some(last) = orders.last() {
                            state.last_timestamp = Some(last.timestamp);
                        }
                        
                        // Check if we got fewer than requested (end of data)
                        if orders.len() < state.chunk_size {
                            state.done = true;
                        }
                        
                        Some((Ok(orders), state))
                    }
                    Err(e) => {
                        state.done = true;
                        Some((Err(e), state))
                    }
                }
            },
        )
    }
    
    /// Count total rows including all event types (for progress reporting)
    pub async fn count_all_events(
        &self,
        symbol: &str,
        exchange: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<i64> {
        use diesel::dsl::count_star;
        
        let mut conn = self.pool.get().await?;
        
        let mut query = historical_orders::table
            .filter(historical_orders::symbol.eq(symbol))
            .filter(historical_orders::exchange.eq(exchange))
            // No event_type filter - include all events
            .select(count_star())
            .into_boxed();
        
        if let Some(start) = start_time {
            query = query.filter(historical_orders::timestamp.ge(start));
        }
        if let Some(end) = end_time {
            query = query.filter(historical_orders::timestamp.lt(end));
        }
        
        let count: i64 = query.first(&mut conn).await?;
        Ok(count)
    }
}

#[cfg(feature = "postgres")]
struct AllEventsStreamState {
    pool: Pool<AsyncPgConnection>,
    symbol: String,
    exchange: String,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    last_timestamp: Option<DateTime<Utc>>,
    chunk_size: usize,
    done: bool,
}

#[cfg(feature = "postgres")]
impl AllEventsStreamState {
    async fn fetch_next_chunk(&self) -> Result<Vec<HistoricalOrder>> {
        let mut conn = self.pool.get().await?;
        
        let mut query = historical_orders::table
            .filter(historical_orders::symbol.eq(&self.symbol))
            .filter(historical_orders::exchange.eq(&self.exchange))
            // Include ALL event types for realistic simulation
            .order(historical_orders::timestamp.asc())
            .limit(self.chunk_size as i64)
            .into_boxed();
        
        // Cursor: start from last timestamp (exclusive for subsequent chunks)
        if let Some(ts) = self.last_timestamp {
            if self.start_time == self.last_timestamp {
                query = query.filter(historical_orders::timestamp.ge(ts));
            } else {
                query = query.filter(historical_orders::timestamp.gt(ts));
            }
        }
        
        if let Some(end) = self.end_time {
            query = query.filter(historical_orders::timestamp.lt(end));
        }
        
        let orders: Vec<HistoricalOrder> = query.load(&mut conn).await?;
        Ok(orders)
    }
}

#[cfg(not(feature = "postgres"))]
pub struct DatabaseStreamer;

#[cfg(not(feature = "postgres"))]
impl DatabaseStreamer {
    pub fn new() -> Self {
        Self
    }
}
