pub mod cache_simple;
pub mod aggregation;
pub mod query;

pub use cache_simple as cache;

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

    /// Convert historical orders to trade data
    pub async fn load_trade_data(&self, params: &QueryParams) -> Result<Vec<TradeData>> {
        let orders = self.load_historical_orders(params).await?;
        
        let trades: Vec<TradeData> = orders
            .into_iter()
            .filter(|order| order.get_event_type() == "TRADE" || order.get_event_type() == "FILL")
            .map(|order| TradeData {
                timestamp: *order.get_timestamp(),
                symbol: order.get_symbol().to_string(),
                exchange: order.get_exchange().to_string(),
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                side: order.get_side().to_string(),
                trade_id: order.get_order_id().to_string(),
            })
            .collect();

        Ok(trades)
    }

    /// Generate OHLCV candles from trade data
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
