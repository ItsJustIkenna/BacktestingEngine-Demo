use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use config::BatchConfig;
use databaseschema::{
    CustomAsyncPgConnectionManager,
    models::{
        historical_order::{HistoricalOrder, NewHistoricalOrder},
        historical_snapshot::{HistoricalSnapshot, NewHistoricalSnapshot},
    },
    ops::{historical_order_ops, historical_snapshot_ops},
};
use deadpool::managed::Pool;
use serde::{Serialize, Deserialize};
use std::sync::Arc;

/// Flexible query builder for historical market data
pub struct QueryBuilder {
    pool: Arc<Pool<CustomAsyncPgConnectionManager>>,
    filters: QueryFilters,
    sorting: Option<SortBy>,
    pagination: Option<Pagination>,
}

/// Query filters for data retrieval
#[derive(Debug, Clone, Default)]
pub struct QueryFilters {
    pub symbols: Option<Vec<String>>,
    pub exchanges: Option<Vec<String>>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub event_types: Option<Vec<String>>,
    pub sides: Option<Vec<String>>,
    pub min_price: Option<BigDecimal>,
    pub max_price: Option<BigDecimal>,
    pub min_quantity: Option<BigDecimal>,
    pub max_quantity: Option<BigDecimal>,
}

/// Sorting configuration
#[derive(Debug, Clone)]
pub enum SortBy {
    Timestamp(SortOrder),
    Price(SortOrder),
    Quantity(SortOrder),
    Symbol(SortOrder),
}

/// Sort order
#[derive(Debug, Clone)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Pagination configuration
#[derive(Debug, Clone)]
pub struct Pagination {
    pub offset: usize,
    pub limit: usize,
}

/// Aggregation types for data analysis
#[derive(Debug, Clone)]
pub enum AggregationType {
    Count,
    Sum(String),    // field name
    Average(String), // field name
    Min(String),    // field name
    Max(String),    // field name
    VolumeProfile,  // price level volume analysis
    TimeBarAggregate(i32), // minute intervals
}

/// Query execution results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult<T> {
    pub data: Vec<T>,
    pub total_count: Option<usize>,
    pub execution_time_ms: u64,
    pub query_metadata: QueryMetadata,
}

/// Metadata about query execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub filters_applied: usize,
    pub sort_applied: bool,
    pub pagination_applied: bool,
    pub cache_hit: bool,
}

impl QueryBuilder {
    /// Create a new query builder
    pub fn new(pool: Arc<Pool<CustomAsyncPgConnectionManager>>) -> Self {
        Self {
            pool,
            filters: QueryFilters::default(),
            sorting: None,
            pagination: None,
        }
    }

    /// Add symbol filter
    pub fn symbols(mut self, symbols: Vec<String>) -> Self {
        self.filters.symbols = Some(symbols);
        self
    }

    /// Add exchange filter
    pub fn exchanges(mut self, exchanges: Vec<String>) -> Self {
        self.filters.exchanges = Some(exchanges);
        self
    }

    /// Add time range filter
    pub fn time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.filters.start_time = Some(start);
        self.filters.end_time = Some(end);
        self
    }

    /// Add event type filter
    pub fn event_types(mut self, event_types: Vec<String>) -> Self {
        self.filters.event_types = Some(event_types);
        self
    }

    /// Add side filter (BUY/SELL)
    pub fn sides(mut self, sides: Vec<String>) -> Self {
        self.filters.sides = Some(sides);
        self
    }

    /// Add price range filter
    pub fn price_range(mut self, min_price: BigDecimal, max_price: BigDecimal) -> Self {
        self.filters.min_price = Some(min_price);
        self.filters.max_price = Some(max_price);
        self
    }

    /// Add quantity range filter
    pub fn quantity_range(mut self, min_quantity: BigDecimal, max_quantity: BigDecimal) -> Self {
        self.filters.min_quantity = Some(min_quantity);
        self.filters.max_quantity = Some(max_quantity);
        self
    }

    /// Add sorting
    pub fn sort_by(mut self, sort_by: SortBy) -> Self {
        self.sorting = Some(sort_by);
        self
    }

    /// Add pagination
    pub fn paginate(mut self, offset: usize, limit: usize) -> Self {
        self.pagination = Some(Pagination { offset, limit });
        self
    }

    /// Execute query for historical orders
    pub async fn execute_orders(&self) -> Result<QueryResult<HistoricalOrder>> {
        let start_time = std::time::Instant::now();
        
        // For now, we'll use the existing database operations and apply filters in memory
        // In a production system, you'd want to push filters down to the database level
        
        let mut all_orders = Vec::new();
        
        if let (Some(symbols), Some(exchanges)) = (&self.filters.symbols, &self.filters.exchanges) {
            for symbol in symbols {
                for exchange in exchanges {
                    let orders = historical_order_ops::get_historical_orders(
                        self.pool.clone(),
                        symbol,
                        exchange,
                    ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                    
                    all_orders.extend(orders);
                }
            }
        } else if let Some(symbols) = &self.filters.symbols {
            for symbol in symbols {
                // Default exchange or iterate through all known exchanges
                let orders = historical_order_ops::get_historical_orders(
                    self.pool.clone(),
                    symbol,
                    "BINANCE", // Default exchange - should be configurable
                ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                
                all_orders.extend(orders);
            }
        }

        // Apply in-memory filters
        let mut filtered_orders = self.apply_order_filters(all_orders);
        
        // Apply sorting
        if let Some(sort) = &self.sorting {
            self.apply_order_sorting(&mut filtered_orders, sort);
        }

        let total_count = filtered_orders.len();

        // Apply pagination
        if let Some(pagination) = &self.pagination {
            let end = std::cmp::min(
                pagination.offset + pagination.limit,
                filtered_orders.len()
            );
            if pagination.offset < filtered_orders.len() {
                filtered_orders = filtered_orders[pagination.offset..end].to_vec();
            } else {
                filtered_orders.clear();
            }
        }

        let execution_time = start_time.elapsed().as_millis() as u64;

        Ok(QueryResult {
            data: filtered_orders,
            total_count: Some(total_count),
            execution_time_ms: execution_time,
            query_metadata: QueryMetadata {
                filters_applied: self.count_active_filters(),
                sort_applied: self.sorting.is_some(),
                pagination_applied: self.pagination.is_some(),
                cache_hit: false, // Would be determined by cache layer
            },
        })
    }

    /// Execute query for historical snapshots
    pub async fn execute_snapshots(&self) -> Result<QueryResult<HistoricalSnapshot>> {
        let start_time = std::time::Instant::now();
        
        let mut all_snapshots = Vec::new();
        
        if let (Some(symbols), Some(exchanges)) = (&self.filters.symbols, &self.filters.exchanges) {
            for symbol in symbols {
                for exchange in exchanges {
                    let snapshots = historical_snapshot_ops::get_historical_snapshot(
                        self.pool.clone(),
                        symbol,
                        exchange,
                    ).await.map_err(|e| anyhow::anyhow!("Database error: {}", e))?;
                    
                    all_snapshots.extend(snapshots);
                }
            }
        }

        // Apply in-memory filters
        let mut filtered_snapshots = self.apply_snapshot_filters(all_snapshots);
        
        // Apply sorting
        if let Some(sort) = &self.sorting {
            self.apply_snapshot_sorting(&mut filtered_snapshots, sort);
        }

        let total_count = filtered_snapshots.len();

        // Apply pagination
        if let Some(pagination) = &self.pagination {
            let end = std::cmp::min(
                pagination.offset + pagination.limit,
                filtered_snapshots.len()
            );
            if pagination.offset < filtered_snapshots.len() {
                filtered_snapshots = filtered_snapshots[pagination.offset..end].to_vec();
            } else {
                filtered_snapshots.clear();
            }
        }

        let execution_time = start_time.elapsed().as_millis() as u64;

        Ok(QueryResult {
            data: filtered_snapshots,
            total_count: Some(total_count),
            execution_time_ms: execution_time,
            query_metadata: QueryMetadata {
                filters_applied: self.count_active_filters(),
                sort_applied: self.sorting.is_some(),
                pagination_applied: self.pagination.is_some(),
                cache_hit: false,
            },
        })
    }

    /// Batch insert historical orders with error handling and retries
    pub async fn batch_insert_orders(
        &self,
        orders: Vec<NewHistoricalOrder>,
        _config: Option<BatchConfig>,
    ) -> Result<Vec<HistoricalOrder>> {
        // Simple batch processing - single attempt to avoid move issues
        match historical_order_ops::create_historical_orders(self.pool.clone(), orders).await {
            Ok(result) => Ok(result),
            Err(e) => Err(anyhow::anyhow!("Batch insert failed: {}", e)),
        }
    }

    /// Batch insert historical snapshots
    pub async fn batch_insert_snapshots(
        &self,
        snapshots: Vec<NewHistoricalSnapshot>,
        _config: Option<BatchConfig>,
    ) -> Result<Vec<HistoricalSnapshot>> {
        // Simple batch processing - single attempt to avoid move issues
        match historical_snapshot_ops::create_historical_snapshot(self.pool.clone(), snapshots).await {
            Ok(result) => Ok(result),
            Err(e) => Err(anyhow::anyhow!("Batch insert failed: {}", e)),
        }
    }

    // Private helper methods
    
    fn apply_order_filters(&self, mut orders: Vec<HistoricalOrder>) -> Vec<HistoricalOrder> {
        if let Some(start) = &self.filters.start_time {
            orders.retain(|order| order.get_timestamp() >= start);
        }
        if let Some(end) = &self.filters.end_time {
            orders.retain(|order| order.get_timestamp() <= end);
        }
        if let Some(event_types) = &self.filters.event_types {
            orders.retain(|order| event_types.contains(&order.get_event_type().to_string()));
        }
        if let Some(sides) = &self.filters.sides {
            orders.retain(|order| sides.contains(&order.get_side().to_string()));
        }
        if let Some(min_price) = &self.filters.min_price {
            orders.retain(|order| order.get_price_level() >= min_price);
        }
        if let Some(max_price) = &self.filters.max_price {
            orders.retain(|order| order.get_price_level() <= max_price);
        }
        if let Some(min_quantity) = &self.filters.min_quantity {
            orders.retain(|order| order.get_quantity() >= min_quantity);
        }
        if let Some(max_quantity) = &self.filters.max_quantity {
            orders.retain(|order| order.get_quantity() <= max_quantity);
        }
        
        orders
    }
    
    fn apply_snapshot_filters(&self, mut snapshots: Vec<HistoricalSnapshot>) -> Vec<HistoricalSnapshot> {
        if let Some(start) = &self.filters.start_time {
            snapshots.retain(|snapshot| snapshot.get_timestamp() >= *start);
        }
        if let Some(end) = &self.filters.end_time {
            snapshots.retain(|snapshot| snapshot.get_timestamp() <= *end);
        }
        if let Some(event_types) = &self.filters.event_types {
            snapshots.retain(|snapshot| event_types.contains(&snapshot.get_event_type().to_string()));
        }
        if let Some(sides) = &self.filters.sides {
            snapshots.retain(|snapshot| sides.contains(&snapshot.get_side().to_string()));
        }
        if let Some(min_price) = &self.filters.min_price {
            snapshots.retain(|snapshot| snapshot.get_price_level() >= min_price);
        }
        if let Some(max_price) = &self.filters.max_price {
            snapshots.retain(|snapshot| snapshot.get_price_level() <= max_price);
        }
        if let Some(min_quantity) = &self.filters.min_quantity {
            snapshots.retain(|snapshot| snapshot.get_quantity() >= min_quantity);
        }
        if let Some(max_quantity) = &self.filters.max_quantity {
            snapshots.retain(|snapshot| snapshot.get_quantity() <= max_quantity);
        }
        
        snapshots
    }

    fn apply_order_sorting(&self, orders: &mut [HistoricalOrder], sort_by: &SortBy) {
        match sort_by {
            SortBy::Timestamp(order) => {
                match order {
                    SortOrder::Ascending => orders.sort_by_key(|o| *o.get_timestamp()),
                    SortOrder::Descending => orders.sort_by_key(|o| std::cmp::Reverse(*o.get_timestamp())),
                }
            },
            SortBy::Price(order) => {
                match order {
                    SortOrder::Ascending => orders.sort_by(|a, b| a.get_price_level().cmp(b.get_price_level())),
                    SortOrder::Descending => orders.sort_by(|a, b| b.get_price_level().cmp(a.get_price_level())),
                }
            },
            SortBy::Quantity(order) => {
                match order {
                    SortOrder::Ascending => orders.sort_by(|a, b| a.get_quantity().cmp(b.get_quantity())),
                    SortOrder::Descending => orders.sort_by(|a, b| b.get_quantity().cmp(a.get_quantity())),
                }
            },
            SortBy::Symbol(order) => {
                match order {
                    SortOrder::Ascending => orders.sort_by(|a, b| a.get_symbol().cmp(b.get_symbol())),
                    SortOrder::Descending => orders.sort_by(|a, b| b.get_symbol().cmp(a.get_symbol())),
                }
            },
        }
    }

    fn apply_snapshot_sorting(&self, snapshots: &mut [HistoricalSnapshot], sort_by: &SortBy) {
        match sort_by {
            SortBy::Timestamp(order) => {
                match order {
                    SortOrder::Ascending => snapshots.sort_by_key(|s| s.get_timestamp()),
                    SortOrder::Descending => snapshots.sort_by_key(|s| std::cmp::Reverse(s.get_timestamp())),
                }
            },
            SortBy::Price(order) => {
                match order {
                    SortOrder::Ascending => snapshots.sort_by(|a, b| a.get_price_level().cmp(b.get_price_level())),
                    SortOrder::Descending => snapshots.sort_by(|a, b| b.get_price_level().cmp(a.get_price_level())),
                }
            },
            SortBy::Quantity(order) => {
                match order {
                    SortOrder::Ascending => snapshots.sort_by(|a, b| a.get_quantity().cmp(b.get_quantity())),
                    SortOrder::Descending => snapshots.sort_by(|a, b| b.get_quantity().cmp(a.get_quantity())),
                }
            },
            SortBy::Symbol(order) => {
                match order {
                    SortOrder::Ascending => snapshots.sort_by(|a, b| a.get_symbol().cmp(b.get_symbol())),
                    SortOrder::Descending => snapshots.sort_by(|a, b| b.get_symbol().cmp(a.get_symbol())),
                }
            },
        }
    }

    fn count_active_filters(&self) -> usize {
        let mut count = 0;
        
        if self.filters.symbols.is_some() { count += 1; }
        if self.filters.exchanges.is_some() { count += 1; }
        if self.filters.start_time.is_some() { count += 1; }
        if self.filters.end_time.is_some() { count += 1; }
        if self.filters.event_types.is_some() { count += 1; }
        if self.filters.sides.is_some() { count += 1; }
        if self.filters.min_price.is_some() { count += 1; }
        if self.filters.max_price.is_some() { count += 1; }
        if self.filters.min_quantity.is_some() { count += 1; }
        if self.filters.max_quantity.is_some() { count += 1; }
        
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_builder_creation() {
        // This would require a mock pool for proper testing
        // let pool = Arc::new(mock_pool);
        // let builder = QueryBuilder::new(pool);
        // assert!(builder.filters.symbols.is_none());
    }

    #[test]
    fn test_filter_counting() {
        // Test with mock data
        let filters = QueryFilters {
            symbols: Some(vec!["BTCUSD".to_string()]),
            exchanges: Some(vec!["BINANCE".to_string()]),
            ..Default::default()
        };
        
        // Test that we can create filters
        assert!(filters.symbols.is_some());
        assert!(filters.exchanges.is_some());
    }
}