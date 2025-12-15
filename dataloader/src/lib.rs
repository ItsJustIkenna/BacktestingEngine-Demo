pub mod cache;
pub mod aggregation;
pub mod query;

#[cfg(feature = "postgres")]
use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(feature = "postgres")]
use databaseschema::{
    models::{
        historical_order::{HistoricalOrder, NewHistoricalOrder},
        historical_snapshot::{HistoricalSnapshot, NewHistoricalSnapshot},
    },
    ops::{
        historical_order_ops,
        historical_snapshot_ops,
    },
};
#[cfg(feature = "postgres")]
use diesel_async::{AsyncPgConnection, pooled_connection::deadpool};

/// Core data loader for market data processing
pub struct DataLoader {
    #[cfg(feature = "postgres")]
    pool: Arc<deadpool::Pool<AsyncPgConnection>>,
    cache: Arc<cache::DataCache>,
}

#[cfg(not(feature = "postgres"))]
impl DataLoader {
    /// Create a new DataLoader instance without database
    pub fn new() -> Self {
        Self {
            cache: Arc::new(cache::DataCache::new()),
        }
    }

    /// Create a mock DataLoader instance for testing
    pub fn mock() -> Self {
        Self::new()
    }

    /// Clear the cache
    pub async fn invalidate_cache(&self) {
        self.cache.clear().await;
    }
}

#[cfg(feature = "postgres")]

/// Market data types supported by the loader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketData {
    Order(HistoricalOrder),
    Snapshot(HistoricalSnapshot),
    Candle(Candle),
    Trade(TradeData),
}

#[cfg(not(feature = "postgres"))]

/// Market data types supported by the loader (without postgres types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketData {
    Candle(Candle),
    Trade(TradeData),
}

/// OHLCV candle data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub open: BigDecimal,
    pub high: BigDecimal,
    pub low: BigDecimal,
    pub close: BigDecimal,
    pub volume: BigDecimal,
    pub trade_count: i64,
}

/// Trade data extracted from historical orders
/// Pre-computed f64 values for performance in hot simulation loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub price: BigDecimal,
    pub quantity: BigDecimal,
    pub side: String,
    pub trade_id: String,
    /// Pre-computed price as f64 for fast access in simulation
    #[serde(default)]
    pub price_f64: f64,
    /// Pre-computed quantity as f64 for fast access in simulation
    #[serde(default)]
    pub quantity_f64: f64,
    /// Pre-computed value (price * quantity) as f64
    #[serde(default)]
    pub value_f64: f64,
}

impl TradeData {
    /// Create a new TradeData with pre-computed f64 values
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: String,
        exchange: String,
        price: BigDecimal,
        quantity: BigDecimal,
        side: String,
        trade_id: String,
    ) -> Self {
        use bigdecimal::ToPrimitive;
        let price_f64 = price.to_f64().unwrap_or(0.0);
        let quantity_f64 = quantity.to_f64().unwrap_or(0.0);
        Self {
            timestamp,
            symbol,
            exchange,
            price,
            quantity,
            side,
            trade_id,
            price_f64,
            quantity_f64,
            value_f64: price_f64 * quantity_f64,
        }
    }
}

/// Query parameters for data retrieval
#[derive(Debug, Clone)]
pub struct QueryParams {
    pub symbol: String,
    pub exchange: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

/// Monte Carlo simulation parameters
#[derive(Debug, Clone)]
pub struct MonteCarloParams {
    pub iterations: usize,
    pub window_minutes: i32,
    pub seed: Option<u64>,
    pub use_bootstrap: bool,
    pub block_size: Option<usize>,
}

#[cfg(feature = "postgres")]
impl DataLoader {
    /// Create a new DataLoader instance
    pub fn new(pool: Arc<deadpool::Pool<AsyncPgConnection>>) -> Self {
        Self {
            pool,
            cache: Arc::new(cache::DataCache::new()),
        }
    }

    /// Load historical orders for a symbol and exchange with enhanced error handling
    #[cfg(feature = "postgres")]
    pub async fn load_historical_orders(&self, params: &QueryParams) -> Result<Vec<HistoricalOrder>> {
        let cache_key = format!("orders_{}_{}", params.symbol, params.exchange);
        
        // Check cache first
        if let Some(cached_orders) = self.cache.get_orders(&cache_key).await {
            return Ok(self.filter_orders(cached_orders, params));
        }

        // Load from database with retry strategy
        let orders = self.retry_database_operation(|| {
            historical_order_ops::get_historical_orders(
                self.pool.clone(),
                &params.symbol,
                &params.exchange,
            )
        }).await.map_err(|e| anyhow::anyhow!("Failed to load historical orders after retries: {}", e))?;

        // Update cache
        self.cache.put_orders(cache_key, orders.clone()).await;

        Ok(self.filter_orders(orders, params))
    }

    /// Load historical snapshots for a symbol and exchange with enhanced error handling
    #[cfg(feature = "postgres")]
    pub async fn load_historical_snapshots(&self, params: &QueryParams) -> Result<Vec<HistoricalSnapshot>> {
        let cache_key = format!("snapshots_{}_{}", params.symbol, params.exchange);
        
        // Check cache first
        if let Some(cached_snapshots) = self.cache.get_snapshots(&cache_key).await {
            return Ok(self.filter_snapshots(cached_snapshots, params));
        }

        // Load from database with retry strategy
        let snapshots = self.retry_database_operation(|| {
            historical_snapshot_ops::get_historical_snapshot(
                self.pool.clone(),
                &params.symbol,
                &params.exchange,
            )
        }).await.map_err(|e| anyhow::anyhow!("Failed to load historical snapshots after retries: {}", e))?;

        // Update cache
        self.cache.put_snapshots(cache_key, snapshots.clone()).await;

        Ok(self.filter_snapshots(snapshots, params))
    }

    /// Load trade data directly from trades table with enhanced error handling
    #[cfg(feature = "postgres")]
    pub async fn load_trade_data(&self, params: &QueryParams) -> Result<Vec<TradeData>> {
        use bigdecimal::ToPrimitive;
        
        // Load directly from trades table instead of historical_orders
        let trades = self.retry_database_operation(|| {
            databaseschema::ops::trades_ops::get_trades_by_symbol(
                self.pool.clone(),
                &params.symbol,
                &params.exchange,
            )
        }).await.map_err(|e| anyhow::anyhow!("Failed to load trades after retries: {}", e))?;

        let trade_data: Vec<TradeData> = trades
            .into_iter()
            .map(|trade| {
                let price_f64 = trade.price.to_f64().unwrap_or(0.0);
                let quantity_f64 = trade.quantity.to_f64().unwrap_or(0.0);
                TradeData {
                    timestamp: trade.created_at,
                    symbol: trade.symbol,
                    exchange: trade.exchange,
                    price: trade.price,
                    quantity: trade.quantity,
                    side: trade.side,
                    trade_id: trade.trade_id,
                    price_f64,
                    quantity_f64,
                    value_f64: price_f64 * quantity_f64,
                }
            })
            .collect();

        Ok(self.filter_trades(trade_data, params))
    }

    /// Load candles using TimescaleDB continuous aggregates (extremely fast)
    #[cfg(feature = "postgres")]
    pub async fn load_candles(&self, params: &QueryParams, interval_minutes: i32) -> Result<Vec<Candle>> {
        let cache_key = format!("candles_{}_{}_{}", params.symbol, params.exchange, interval_minutes);
        
        // Check cache first with shorter TTL for real-time data
        if let Some(cached_candles) = self.cache.get_candles(&cache_key).await {
            return Ok(self.filter_candles(cached_candles, params));
        }

        // Load from continuous aggregates based on interval
        let candles = match interval_minutes {
            1 => {
                let db_candles = databaseschema::ops::candles_ops::get_candles_1m(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.start_time,
                    params.end_time,
                    params.limit,
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                db_candles.into_iter().map(|candle| Candle {
                    timestamp: candle.timestamp,
                    symbol: candle.symbol,
                    exchange: candle.exchange,
                    open: candle.open_price,
                    high: candle.high_price,
                    low: candle.low_price,
                    close: candle.close_price,
                    volume: candle.volume,
                    trade_count: candle.trade_count as i64,
                }).collect::<Vec<Candle>>()
            },
            5 => {
                let db_candles = databaseschema::ops::candles_ops::get_candles_5m(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.start_time,
                    params.end_time,
                    params.limit,
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                db_candles.into_iter().map(|candle| Candle {
                    timestamp: candle.timestamp,
                    symbol: candle.symbol,
                    exchange: candle.exchange,
                    open: candle.open_price,
                    high: candle.high_price,
                    low: candle.low_price,
                    close: candle.close_price,
                    volume: candle.volume,
                    trade_count: candle.trade_count as i64,
                }).collect::<Vec<Candle>>()
            },
            15 => {
                let db_candles = databaseschema::ops::candles_ops::get_candles_15m(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.start_time,
                    params.end_time,
                    params.limit,
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                db_candles.into_iter().map(|candle| Candle {
                    timestamp: candle.timestamp,
                    symbol: candle.symbol,
                    exchange: candle.exchange,
                    open: candle.open_price,
                    high: candle.high_price,
                    low: candle.low_price,
                    close: candle.close_price,
                    volume: candle.volume,
                    trade_count: candle.trade_count as i64,
                }).collect::<Vec<Candle>>()
            },
            60 => {
                let db_candles = databaseschema::ops::candles_ops::get_candles_1h(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.start_time,
                    params.end_time,
                    params.limit,
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                db_candles.into_iter().map(|candle| Candle {
                    timestamp: candle.timestamp,
                    symbol: candle.symbol,
                    exchange: candle.exchange,
                    open: candle.open_price,
                    high: candle.high_price,
                    low: candle.low_price,
                    close: candle.close_price,
                    volume: candle.volume,
                    trade_count: candle.trade_count as i64,
                }).collect::<Vec<Candle>>()
            },
            1440 => { // 24 * 60 minutes = 1 day
                let db_candles = databaseschema::ops::candles_ops::get_candles_1d(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.start_time,
                    params.end_time,
                    params.limit,
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                db_candles.into_iter().map(|candle| Candle {
                    timestamp: candle.timestamp,
                    symbol: candle.symbol,
                    exchange: candle.exchange,
                    open: candle.open_price,
                    high: candle.high_price,
                    low: candle.low_price,
                    close: candle.close_price,
                    volume: candle.volume,
                    trade_count: candle.trade_count as i64,
                }).collect::<Vec<Candle>>()
            },
            _ => {
                // For non-standard intervals, fall back to generating from trades
                tracing::warn!("Non-standard interval {}, generating from trades", interval_minutes);
                return self.generate_candles(params, interval_minutes).await;
            }
        };

        // Cache with optimized TTL based on interval
        let cache_ttl = match interval_minutes {
            1 => std::time::Duration::from_secs(60),      // 1m candles: 1 minute cache
            5 => std::time::Duration::from_secs(300),     // 5m candles: 5 minute cache
            15 => std::time::Duration::from_secs(900),    // 15m candles: 15 minute cache
            60 => std::time::Duration::from_secs(3600),   // 1h candles: 1 hour cache
            1440 => std::time::Duration::from_secs(86400), // 1d candles: 1 day cache
            _ => std::time::Duration::from_secs(300),     // Default: 5 minutes
        };
        
        self.cache.put_candles_with_ttl(cache_key, candles.clone(), cache_ttl).await;

        Ok(candles)
    }

    /// Load pre-computed candles from continuous aggregates (replaced by load_candles)
    async fn _load_precomputed_candles(&self, params: &QueryParams, interval_minutes: i32) -> Result<Vec<Candle>> {
        // This method is now replaced by the continuous aggregate approach in load_candles
        self.load_candles(params, interval_minutes).await
    }

    /// Align timestamp to interval boundary
    fn align_to_interval(timestamp: DateTime<Utc>, interval_minutes: i32) -> DateTime<Utc> {
        let minutes = timestamp.timestamp() / 60;
        let aligned_minutes = (minutes / interval_minutes as i64) * interval_minutes as i64;
        DateTime::from_timestamp(aligned_minutes * 60, 0).unwrap_or(timestamp)
    }
    
    /// Generate OHLCV candles from trade data (fallback method)
    pub async fn generate_candles(&self, params: &QueryParams, interval_minutes: i32) -> Result<Vec<Candle>> {
        let trades = self.load_trade_data(params).await?;
        
        if trades.is_empty() {
            return Ok(vec![]);
        }

        let mut candles = Vec::new();
        let mut current_candle: Option<Candle> = None;

        for trade in trades {
            let candle_start = Self::align_to_interval(trade.timestamp, interval_minutes);
            
            match &mut current_candle {
                Some(candle) if candle.timestamp == candle_start => {
                    // Update existing candle
                    if trade.price > candle.high {
                        candle.high = trade.price.clone();
                    }
                    if trade.price < candle.low {
                        candle.low = trade.price.clone();
                    }
                    candle.close = trade.price;
                    candle.volume += &trade.quantity;
                    candle.trade_count += 1;
                },
                _ => {
                    // Save previous candle if exists
                    if let Some(candle) = current_candle.take() {
                        candles.push(candle);
                    }
                    
                    // Create new candle
                    current_candle = Some(Candle {
                        timestamp: candle_start,
                        symbol: trade.symbol.clone(),
                        exchange: trade.exchange.clone(),
                        open: trade.price.clone(),
                        high: trade.price.clone(),
                        low: trade.price.clone(),
                        close: trade.price,
                        volume: trade.quantity,
                        trade_count: 1,
                    });
                }
            }
        }

        // Add final candle
        if let Some(candle) = current_candle {
            candles.push(candle);
        }

        Ok(candles)
    }

    /// Generate Monte Carlo simulations
    pub async fn monte_carlo_simulation(
        &self,
        params: &QueryParams,
        mc_params: &MonteCarloParams,
    ) -> Result<Vec<Vec<HistoricalOrder>>> {
        let mut simulations = Vec::with_capacity(mc_params.iterations);

        for i in 0..mc_params.iterations {
            let seed = mc_params.seed.map(|s| s + i as u64);
            
            let simulated_orders = if mc_params.use_bootstrap {
                historical_order_ops::get_bootstrap_historical_orders(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    params.limit.unwrap_or(1000) as usize,
                    mc_params.block_size,
                    seed,
                ).await.map_err(|e| anyhow::anyhow!("Bootstrap simulation error: {}", e))?
            } else {
                historical_order_ops::get_randomized_historical_orders(
                    self.pool.clone(),
                    &params.symbol,
                    &params.exchange,
                    mc_params.window_minutes,
                    seed,
                ).await.map_err(|e| anyhow::anyhow!("Randomized simulation error: {}", e))?
            };

            simulations.push(self.filter_orders(simulated_orders, params));
        }

        Ok(simulations)
    }

    /// Insert new historical orders
    #[cfg(feature = "postgres")]
    pub async fn insert_historical_orders(&self, orders: Vec<NewHistoricalOrder>) -> Result<Vec<HistoricalOrder>> {
        if orders.is_empty() {
            return Ok(vec![]);
        }

        let result = historical_order_ops::create_historical_orders(self.pool.clone(), orders)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to insert orders: {}", e))?;

        // Invalidate cache
        self.invalidate_cache().await;

        Ok(result)
    }

    /// Insert new historical orders (mock implementation)
    #[cfg(not(feature = "postgres"))]
    pub async fn insert_historical_orders(&self, orders: Vec<NewHistoricalOrder>) -> Result<Vec<HistoricalOrder>> {
        // Mock implementation - return empty vec
        Ok(vec![])
    }

    /// Insert new historical snapshots
    #[cfg(feature = "postgres")]
    pub async fn insert_historical_snapshots(&self, snapshots: Vec<NewHistoricalSnapshot>) -> Result<Vec<HistoricalSnapshot>> {
        if snapshots.is_empty() {
            return Ok(vec![]);
        }

        let result = historical_snapshot_ops::create_historical_snapshot(self.pool.clone(), snapshots)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to insert snapshots: {}", e))?;

        // Invalidate cache
        self.invalidate_cache().await;

        Ok(result)
    }

    /// Insert new historical snapshots (mock implementation)
    #[cfg(not(feature = "postgres"))]
    pub async fn insert_historical_snapshots(&self, snapshots: Vec<NewHistoricalSnapshot>) -> Result<Vec<HistoricalSnapshot>> {
        // Mock implementation - return empty vec
        Ok(vec![])
    }

    /// Clear the cache
    pub async fn invalidate_cache(&self) {
        self.cache.clear().await;
    }

    /// Enhanced retry logic for database operations
    #[cfg(feature = "postgres")]
    async fn retry_database_operation<T, F, Fut>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, diesel::result::Error>>,
    {
        use tokio::time::{sleep, Duration};
        
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 3;
        const BASE_DELAY_MS: u64 = 100;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        return Err(anyhow::anyhow!("Database operation failed after {} attempts: {}", MAX_ATTEMPTS, e).into());
                    }

                    // Exponential backoff with jitter
                    let delay = Duration::from_millis(BASE_DELAY_MS * 2_u64.pow(attempts as u32 - 1));
                    let jitter = Duration::from_millis(fastrand::u64(0..=50));
                    
                    tracing::warn!("Database operation failed (attempt {}/{}): {}. Retrying in {:?}...", 
                                 attempts, MAX_ATTEMPTS, e, delay + jitter);
                    
                    sleep(delay + jitter).await;
                }
            }
        }
    }

    /// Filter orders based on query parameters
    fn filter_orders(&self, mut orders: Vec<HistoricalOrder>, params: &QueryParams) -> Vec<HistoricalOrder> {
        // Filter by time range
        if let Some(start) = params.start_time {
            orders.retain(|order| *order.get_timestamp() >= start);
        }
        if let Some(end) = params.end_time {
            orders.retain(|order| *order.get_timestamp() <= end);
        }

        // Apply limit
        if let Some(limit) = params.limit {
            orders.truncate(limit as usize);
        }

        orders
    }

    /// Filter snapshots based on query parameters
    fn filter_snapshots(&self, mut snapshots: Vec<HistoricalSnapshot>, params: &QueryParams) -> Vec<HistoricalSnapshot> {
        // Filter by time range
        if let Some(start) = params.start_time {
            snapshots.retain(|snapshot| snapshot.get_timestamp() >= start);
        }
        if let Some(end) = params.end_time {
            snapshots.retain(|snapshot| snapshot.get_timestamp() <= end);
        }

        // Apply limit
        if let Some(limit) = params.limit {
            snapshots.truncate(limit as usize);
        }

        snapshots
    }

    /// Filter trades based on query parameters
    fn filter_trades(&self, mut trades: Vec<TradeData>, params: &QueryParams) -> Vec<TradeData> {
        // Filter by time range
        if let Some(start) = params.start_time {
            trades.retain(|trade| trade.timestamp >= start);
        }
        if let Some(end) = params.end_time {
            trades.retain(|trade| trade.timestamp <= end);
        }

        // Apply limit
        if let Some(limit) = params.limit {
            trades.truncate(limit as usize);
        }

        trades
    }

    /// Filter candles based on query parameters
    fn filter_candles(&self, mut candles: Vec<Candle>, params: &QueryParams) -> Vec<Candle> {
        // Filter by time range
        if let Some(start) = params.start_time {
            candles.retain(|candle| candle.timestamp >= start);
        }
        if let Some(end) = params.end_time {
            candles.retain(|candle| candle.timestamp <= end);
        }

        // Apply limit
        if let Some(limit) = params.limit {
            candles.truncate(limit as usize);
        }

        candles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_params_creation() {
        let params = QueryParams {
            symbol: "BTCUSD".to_string(),
            exchange: "BINANCE".to_string(),
            start_time: None,
            end_time: None,
            limit: Some(100),
        };
        assert_eq!(params.symbol, "BTCUSD");
        assert_eq!(params.limit, Some(100));
    }
}
