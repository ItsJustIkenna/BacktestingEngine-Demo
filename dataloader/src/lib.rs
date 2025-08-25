pub mod cache;
pub mod aggregation;
pub mod query;

use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use databaseschema::{
    CustomAsyncPgConnectionManager,
    models::{
        historical_order::{HistoricalOrder, NewHistoricalOrder},
        historical_snapshot::{HistoricalSnapshot, NewHistoricalSnapshot},
    },
    ops::{
        historical_order_ops,
        historical_snapshot_ops,
    },
};
use deadpool::managed::Pool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Core data loader for market data processing
pub struct DataLoader {
    pool: Arc<Pool<CustomAsyncPgConnectionManager>>,
    cache: Arc<cache::DataCache>,
}

/// Market data types supported by the loader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketData {
    Order(HistoricalOrder),
    Snapshot(HistoricalSnapshot),
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub price: BigDecimal,
    pub quantity: BigDecimal,
    pub side: String,
    pub trade_id: String,
}

/// Query parameters for data retrieval
#[derive(Debug, Clone)]
pub struct QueryParams {
    pub symbol: String,
    pub exchange: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
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

impl DataLoader {
    /// Create a new DataLoader instance
    pub fn new(pool: Arc<Pool<CustomAsyncPgConnectionManager>>) -> Self {
        Self {
            pool,
            cache: Arc::new(cache::DataCache::new()),
        }
    }

    /// Load historical orders for a symbol and exchange
    pub async fn load_historical_orders(&self, params: &QueryParams) -> Result<Vec<HistoricalOrder>> {
        let cache_key = format!("orders_{}_{}", params.symbol, params.exchange);
        
        // Check cache first
        if let Some(cached_orders) = self.cache.get_orders(&cache_key).await {
            return Ok(self.filter_orders(cached_orders, params));
        }

        // Load from database
        let orders = historical_order_ops::get_historical_orders(
            self.pool.clone(),
            &params.symbol,
            &params.exchange,
        ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;

        // Update cache
        self.cache.put_orders(cache_key, orders.clone()).await;

        Ok(self.filter_orders(orders, params))
    }

    /// Load historical snapshots for a symbol and exchange
    pub async fn load_historical_snapshots(&self, params: &QueryParams) -> Result<Vec<HistoricalSnapshot>> {
        let cache_key = format!("snapshots_{}_{}", params.symbol, params.exchange);
        
        // Check cache first
        if let Some(cached_snapshots) = self.cache.get_snapshots(&cache_key).await {
            return Ok(self.filter_snapshots(cached_snapshots, params));
        }

        // Load from database
        let snapshots = historical_snapshot_ops::get_historical_snapshot(
            self.pool.clone(),
            &params.symbol,
            &params.exchange,
        ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;

        // Update cache
        self.cache.put_snapshots(cache_key, snapshots.clone()).await;

        Ok(self.filter_snapshots(snapshots, params))
    }

    /// Load trade data directly from trades table (more efficient)
    pub async fn load_trade_data(&self, params: &QueryParams) -> Result<Vec<TradeData>> {
        // Load directly from trades table instead of historical_orders
        let trades = databaseschema::ops::trades_ops::get_trades_by_symbol(
            self.pool.clone(),
            &params.symbol,
            &params.exchange,
        ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;

        let trade_data: Vec<TradeData> = trades
            .into_iter()
            .map(|trade| TradeData {
                timestamp: trade.created_at,
                symbol: trade.symbol,
                exchange: trade.exchange,
                price: trade.price,
                quantity: trade.quantity,
                side: trade.side,
                trade_id: trade.trade_id,
            })
            .collect();

        Ok(self.filter_trades(trade_data, params))
    }

    /// Load candles using TimescaleDB continuous aggregates (extremely fast)
    pub async fn load_candles(&self, params: &QueryParams, interval_minutes: i32) -> Result<Vec<Candle>> {
        let cache_key = format!("candles_{}_{}_{}", params.symbol, params.exchange, interval_minutes);
        
        // Check cache first
        if let Some(cached_candles) = self.cache.get_candles(&cache_key).await {
            return Ok(self.filter_candles(cached_candles, params));
        }

        // Determine which continuous aggregate view to query based on interval
        let view_name = match interval_minutes {
            1 => "candles_1m",
            5 => "candles_5m", 
            15 => "candles_15m",
            60 => "candles_1h",
            1440 => "candles_1d", // 24 * 60 minutes = 1 day
            _ => {
                // For non-standard intervals, fall back to generating from trades
                tracing::warn!("Non-standard interval {}, generating from trades", interval_minutes);
                return self.generate_candles(params, interval_minutes).await;
            }
        };

        // Query the continuous aggregate view directly
        let query = format!(
            "SELECT timestamp, symbol, exchange, open_price, high_price, low_price, close_price, volume, trade_count 
             FROM {} 
             WHERE symbol = $1 AND exchange = $2 
             {} 
             ORDER BY timestamp ASC 
             {}",
            view_name,
            if params.start_time.is_some() || params.end_time.is_some() {
                let mut conditions = Vec::new();
                if params.start_time.is_some() {
                    conditions.push("AND timestamp >= $3");
                }
                if params.end_time.is_some() {
                    let param_num = if params.start_time.is_some() { 4 } else { 3 };
                    conditions.push(&format!("AND timestamp <= ${}", param_num));
                }
                conditions.join(" ")
            } else {
                String::new()
            },
            if let Some(limit) = params.limit {
                format!("LIMIT {}", limit)
            } else {
                String::new()
            }
        );

        // Execute the query using the connection pool
        let conn = self.pool.get().await.map_err(|e| anyhow::anyhow!("Connection error: {}", e))?;
        
        // Build parameter vector based on what's provided
        let mut query_params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&params.symbol, &params.exchange];
        if let Some(ref start) = params.start_time {
            query_params.push(start);
        }
        if let Some(ref end) = params.end_time {
            query_params.push(end);
        }

        let rows = conn.query(&query, &query_params).await.map_err(|e| anyhow::anyhow!("Query error: {}", e))?;

        let candles: Vec<Candle> = rows
            .into_iter()
            .map(|row| Candle {
                timestamp: row.get(0),
                symbol: row.get(1),
                exchange: row.get(2),
                open: row.get(3),
                high: row.get(4),
                low: row.get(5),
                close: row.get(6),
                volume: row.get(7),
                trade_count: row.get(8),
            })
            .collect();

        // Update cache
        self.cache.put_candles(cache_key, candles.clone()).await;

        Ok(candles)
    }

    /// Load pre-computed candles from continuous aggregates (replaced by load_candles)
    async fn load_precomputed_candles(&self, params: &QueryParams, interval_minutes: i32) -> Result<Vec<Candle>> {
        // This method is now replaced by the continuous aggregate approach in load_candles
        self.load_candles(params, interval_minutes).await
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
            let candle_start = align_to_interval(trade.timestamp, interval_minutes);
            
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
                    params.limit.unwrap_or(1000),
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

    /// Insert new historical snapshots
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

    /// Clear the cache
    pub async fn invalidate_cache(&self) {
        self.cache.clear().await;
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
            orders.truncate(limit);
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
            snapshots.truncate(limit);
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
            trades.truncate(limit);
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
            candles.truncate(limit);
        }

        candles
    }
}

/// Helper function to align timestamp to interval
fn align_to_interval(timestamp: DateTime<Utc>, interval_minutes: i32) -> DateTime<Utc> {
    let total_minutes = timestamp.timestamp() / 60;
    let aligned_minutes = (total_minutes / interval_minutes as i64) * interval_minutes as i64;
    DateTime::from_timestamp(aligned_minutes * 60, 0).unwrap_or(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_align_to_interval() {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T10:37:45Z").unwrap().with_timezone(&Utc);
        let aligned = align_to_interval(timestamp, 5);
        assert_eq!(aligned.minute(), 35); // Should align to 10:35
    }

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
