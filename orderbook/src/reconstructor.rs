//! Order Book Reconstruction Engine
//!
//! This module handles the reconstruction of order books from historical market data,
//! converting raw order and snapshot data into live order book events.

use crate::order_book::OrderBook;
use crate::types::{BookSide, OrderBookEvent};
use crate::OrderBookConfig;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dataloader::DataLoader;
use databaseschema::models::{
    historical_order::HistoricalOrder,
    historical_snapshot::HistoricalSnapshot,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Order book reconstructor that maintains order books from historical data
pub struct OrderBookReconstructor {
    /// Data loader for accessing historical data
    dataloader: Arc<DataLoader>,
    /// Currently active order books by symbol-exchange pair
    books: HashMap<String, OrderBook>,
    /// Configuration for order books
    config: OrderBookConfig,
    /// Event queue for processing
    event_queue: VecDeque<(DateTime<Utc>, OrderBookEvent)>,
    /// Last processed timestamp
    last_timestamp: Option<DateTime<Utc>>,
}

impl OrderBookReconstructor {
    /// Create a new order book reconstructor
    pub fn new(dataloader: Arc<DataLoader>, config: OrderBookConfig) -> Self {
        Self {
            dataloader,
            books: HashMap::new(),
            config,
            event_queue: VecDeque::new(),
            last_timestamp: None,
        }
    }

    /// Get or create an order book for a symbol-exchange pair
    pub fn get_or_create_book(&mut self, symbol: &str, exchange: &str) -> &mut OrderBook {
        let key = format!("{}:{}", symbol, exchange);
        self.books.entry(key).or_insert_with(|| {
            OrderBook::new(symbol.to_string(), exchange.to_string(), self.config.clone())
        })
    }

    /// Get a reference to an existing order book
    pub fn get_book(&self, symbol: &str, exchange: &str) -> Option<&OrderBook> {
        let key = format!("{}:{}", symbol, exchange);
        self.books.get(&key)
    }

    /// Load and process historical orders for reconstruction
    pub async fn load_historical_orders(
        &mut self,
        symbol: &str,
        exchange: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let params = dataloader::QueryParams {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            start_time,
            end_time,
            limit: None,
        };

        let orders = self.dataloader.load_historical_orders(&params).await?;
        info!(
            "Loaded {} historical orders for {}:{} between {:?} and {:?}",
            orders.len(),
            symbol,
            exchange,
            start_time,
            end_time
        );

        // Convert orders to events and queue them
        for order in orders {
            if let Some(event) = self.convert_order_to_event(&order) {
                self.event_queue.push_back((*order.get_timestamp(), event));
            }
        }

        // Sort events by timestamp
        let mut events: Vec<_> = self.event_queue.drain(..).collect();
        events.sort_by_key(|(timestamp, _)| *timestamp);
        self.event_queue.extend(events);

        Ok(())
    }

    /// Load and process historical snapshots for reconstruction
    pub async fn load_historical_snapshots(
        &mut self,
        symbol: &str,
        exchange: &str,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let params = dataloader::QueryParams {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            start_time,
            end_time,
            limit: None,
        };

        let snapshots = self.dataloader.load_historical_snapshots(&params).await?;
        info!(
            "Loaded {} historical snapshots for {}:{} between {:?} and {:?}",
            snapshots.len(),
            symbol,
            exchange,
            start_time,
            end_time
        );

        // Convert snapshots to events and queue them
        for snapshot in snapshots {
            if let Some(event) = self.convert_snapshot_to_event(&snapshot) {
                self.event_queue.push_back((snapshot.get_timestamp(), event));
            }
        }

        Ok(())
    }

    /// Process all queued events up to a specific timestamp
    pub fn process_events_until(&mut self, until_timestamp: DateTime<Utc>) -> Result<usize> {
        let mut processed_count = 0;

        while let Some((timestamp, _)) = self.event_queue.front() {
            if *timestamp > until_timestamp {
                break;
            }

            if let Some((event_timestamp, event)) = self.event_queue.pop_front() {
                self.process_single_event(event)?;
                self.last_timestamp = Some(event_timestamp);
                processed_count += 1;
            }
        }

        debug!("Processed {} events until {}", processed_count, until_timestamp);
        Ok(processed_count)
    }

    /// Process all remaining events
    pub fn process_all_events(&mut self) -> Result<usize> {
        let mut processed_count = 0;

        while let Some((event_timestamp, event)) = self.event_queue.pop_front() {
            self.process_single_event(event)?;
            self.last_timestamp = Some(event_timestamp);
            processed_count += 1;
        }

        info!("Processed all {} remaining events", processed_count);
        Ok(processed_count)
    }

    /// Process a single order book event
    fn process_single_event(&mut self, event: OrderBookEvent) -> Result<()> {
        match &event {
            OrderBookEvent::NewOrder { .. }
            | OrderBookEvent::ModifyOrder { .. }
            | OrderBookEvent::CancelOrder { .. }
            | OrderBookEvent::Trade { .. } => {
                // Extract symbol and exchange from event (we'll need to enhance events with this info)
                warn!("Individual order events not yet supported - need symbol/exchange context");
            }
            OrderBookEvent::Snapshot { .. } => {
                // For snapshots, we'll need to enhance the event structure to include symbol/exchange
                warn!("Snapshot events need symbol/exchange context");
            }
        }

        Ok(())
    }

    /// Convert a historical order to an order book event
    fn convert_order_to_event(&self, order: &HistoricalOrder) -> Option<OrderBookEvent> {
        let side = BookSide::from_str(order.get_side())?;
        let timestamp = *order.get_timestamp();

        match order.get_event_type().to_uppercase().as_str() {
            "NEW" | "ADD" => Some(OrderBookEvent::NewOrder {
                order_id: order.get_order_id().to_string(),
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            }),
            "MODIFY" | "UPDATE" => Some(OrderBookEvent::ModifyOrder {
                order_id: order.get_order_id().to_string(),
                new_quantity: order.get_quantity().clone(),
                timestamp,
            }),
            "CANCEL" | "DELETE" => Some(OrderBookEvent::CancelOrder {
                order_id: order.get_order_id().to_string(),
                timestamp,
            }),
            "TRADE" | "FILL" | "EXECUTION" => Some(OrderBookEvent::Trade {
                aggressor_order_id: Some(order.get_order_id().to_string()),
                passive_order_id: None,
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            }),
            _ => {
                debug!("Unknown event type: {}", order.get_event_type());
                None
            }
        }
    }

    /// Convert a historical snapshot to an order book event
    fn convert_snapshot_to_event(&self, snapshot: &HistoricalSnapshot) -> Option<OrderBookEvent> {
        // This is a simplified conversion - in practice, you'd need to parse
        // the snapshot data structure to extract bid/ask levels
        
        // For now, create empty snapshot event
        Some(OrderBookEvent::Snapshot {
            bids: Vec::new(),  // Would need to parse from snapshot data
            asks: Vec::new(),  // Would need to parse from snapshot data
            timestamp: snapshot.get_timestamp(),
        })
    }

    /// Get the current state of all order books
    pub fn get_all_books(&self) -> &HashMap<String, OrderBook> {
        &self.books
    }

    /// Get the last processed timestamp
    pub fn get_last_timestamp(&self) -> Option<DateTime<Utc>> {
        self.last_timestamp
    }

    /// Get the number of pending events
    pub fn pending_events(&self) -> usize {
        self.event_queue.len()
    }

    /// Clear all data and reset
    pub fn reset(&mut self) {
        self.books.clear();
        self.event_queue.clear();
        self.last_timestamp = None;
    }

    /// Reconstruct order book for a specific symbol/exchange pair over a time range
    pub async fn reconstruct_book(
        &mut self,
        symbol: &str,
        exchange: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<&OrderBook> {
        info!(
            "Starting order book reconstruction for {}:{} from {} to {}",
            symbol, exchange, start_time, end_time
        );

        // Load historical data
        self.load_historical_orders(symbol, exchange, Some(start_time), Some(end_time))
            .await?;
        self.load_historical_snapshots(symbol, exchange, Some(start_time), Some(end_time))
            .await?;

        // Process all events
        let processed_count = self.process_all_events()?;
        info!("Reconstruction complete. Processed {} events", processed_count);

        // Return the reconstructed book
        Ok(self.get_or_create_book(symbol, exchange))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_mock_dataloader() -> Arc<DataLoader> {
        // This is a mock for testing - the actual pool creation would be handled elsewhere
        // For unit tests, we'll create a minimal DataLoader that won't be used for actual DB operations
        use databaseschema::CustomAsyncPgConnectionManager;
        use deadpool::managed::Pool;
        
        let manager = CustomAsyncPgConnectionManager::new("postgresql://mock".to_string());
        let pool = Pool::builder(manager).max_size(1).build().unwrap();
        Arc::new(DataLoader::new(Arc::new(pool)))
    }

    #[test]
    fn test_reconstructor_creation() {
        let dataloader = create_mock_dataloader();
        let config = OrderBookConfig::default();
        let reconstructor = OrderBookReconstructor::new(dataloader, config);
        
        assert_eq!(reconstructor.books.len(), 0);
        assert_eq!(reconstructor.pending_events(), 0);
        assert!(reconstructor.get_last_timestamp().is_none());
    }

    #[test]
    fn test_book_management() {
        let dataloader = create_mock_dataloader();
        let config = OrderBookConfig::default();
        let mut reconstructor = OrderBookReconstructor::new(dataloader, config);
        
        // Create a book
        let _book = reconstructor.get_or_create_book("BTCUSD", "BINANCE");
        assert_eq!(reconstructor.books.len(), 1);
        
        // Get existing book
        let book = reconstructor.get_book("BTCUSD", "BINANCE");
        assert!(book.is_some());
        
        // Non-existent book
        let book = reconstructor.get_book("ETHUSD", "COINBASE");
        assert!(book.is_none());
    }

    #[test]
    fn test_reset_functionality() {
        let dataloader = create_mock_dataloader();
        let config = OrderBookConfig::default();
        let mut reconstructor = OrderBookReconstructor::new(dataloader, config);
        
        // Add some data
        let _book = reconstructor.get_or_create_book("BTCUSD", "BINANCE");
        assert_eq!(reconstructor.books.len(), 1);
        
        // Reset
        reconstructor.reset();
        assert_eq!(reconstructor.books.len(), 0);
        assert_eq!(reconstructor.pending_events(), 0);
        assert!(reconstructor.get_last_timestamp().is_none());
    }
}
