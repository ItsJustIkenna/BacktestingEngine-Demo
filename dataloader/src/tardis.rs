//! Tardis.dev data loader for backtesting
//!
//! Loads trades and L2 orderbook snapshots from Tardis.dev CSV files.
//! Supports both raw gzipped files and combined gzipped files.
//!
//! Data format (trades):
//! exchange,symbol,timestamp,local_timestamp,id,side,price,amount
//!
//! Data format (book_snapshot_25):
//! exchange,symbol,timestamp,local_timestamp,asks[0].price,asks[0].amount,...,bids[0].price,bids[0].amount,...

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use thiserror::Error;

use crate::tick::Side;
use crate::Tick;

/// Errors that can occur when loading Tardis data
#[derive(Error, Debug)]
pub enum TardisError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error at line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("File not found: {0}")]
    FileNotFound(String),
}

/// A trade from Tardis.dev
#[derive(Debug, Clone)]
pub struct TardisTrade {
    /// Timestamp in microseconds since Unix epoch
    pub timestamp_us: i64,
    /// Local timestamp in nanoseconds (exchange receive time)
    pub local_timestamp_ns: i64,
    /// Trade ID from exchange
    pub trade_id: i64,
    /// Trade side (Buy or Sell - taker side)
    pub side: Side,
    /// Price as f64
    pub price: f64,
    /// Quantity as f64
    pub amount: f64,
}

impl Tick for TardisTrade {
    #[inline]
    fn timestamp(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.timestamp_us)
            .unwrap_or(DateTime::UNIX_EPOCH)
    }

    #[inline]
    fn timestamp_ms(&self) -> i64 {
        self.timestamp_us / 1000
    }

    #[inline]
    fn price(&self) -> f64 {
        self.price
    }

    #[inline]
    fn quantity(&self) -> f64 {
        self.amount
    }

    #[inline]
    fn side(&self) -> Side {
        self.side
    }

    #[inline]
    fn event_type(&self) -> u8 {
        3 // Trade
    }
}

/// A single price level in the orderbook
#[derive(Debug, Clone, Copy, Default)]
pub struct PriceLevel {
    pub price: f64,
    pub amount: f64,
}

/// L2 orderbook snapshot from Tardis.dev (25 levels each side)
#[derive(Debug, Clone)]
pub struct TardisOrderbookSnapshot {
    /// Timestamp in microseconds since Unix epoch
    pub timestamp_us: i64,
    /// Local timestamp in nanoseconds
    pub local_timestamp_ns: i64,
    /// Ask levels (best ask at index 0)
    pub asks: [PriceLevel; 25],
    /// Bid levels (best bid at index 0)
    pub bids: [PriceLevel; 25],
}

impl TardisOrderbookSnapshot {
    /// Get the best bid price
    #[inline]
    pub fn best_bid(&self) -> f64 {
        self.bids[0].price
    }

    /// Get the best ask price
    #[inline]
    pub fn best_ask(&self) -> f64 {
        self.asks[0].price
    }

    /// Get the mid price
    #[inline]
    pub fn mid_price(&self) -> f64 {
        (self.best_bid() + self.best_ask()) / 2.0
    }

    /// Get the spread
    #[inline]
    pub fn spread(&self) -> f64 {
        self.best_ask() - self.best_bid()
    }

    /// Get the spread in basis points
    #[inline]
    pub fn spread_bps(&self) -> f64 {
        self.spread() / self.mid_price() * 10000.0
    }

    /// Get total bid liquidity up to N levels
    pub fn bid_liquidity(&self, levels: usize) -> f64 {
        self.bids.iter().take(levels).map(|l| l.amount).sum()
    }

    /// Get total ask liquidity up to N levels
    pub fn ask_liquidity(&self, levels: usize) -> f64 {
        self.asks.iter().take(levels).map(|l| l.amount).sum()
    }

    /// Get bid liquidity weighted by price distance from mid
    pub fn bid_depth_weighted(&self, levels: usize) -> f64 {
        let mid = self.mid_price();
        self.bids
            .iter()
            .take(levels)
            .map(|l| l.amount * l.price / mid)
            .sum()
    }

    /// Get ask depth weighted by price distance from mid
    pub fn ask_depth_weighted(&self, levels: usize) -> f64 {
        let mid = self.mid_price();
        self.asks
            .iter()
            .take(levels)
            .map(|l| l.amount * l.price / mid)
            .sum()
    }

    /// Calculate order book imbalance (-1 to 1, positive = more bids)
    pub fn imbalance(&self, levels: usize) -> f64 {
        let bid_liq = self.bid_liquidity(levels);
        let ask_liq = self.ask_liquidity(levels);
        let total = bid_liq + ask_liq;
        if total == 0.0 {
            0.0
        } else {
            (bid_liq - ask_liq) / total
        }
    }

    /// Get timestamp as DateTime
    pub fn timestamp(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.timestamp_us)
            .unwrap_or(DateTime::UNIX_EPOCH)
    }
}

/// Loader for Tardis.dev data files
pub struct TardisLoader;

impl TardisLoader {
    /// Load trades from a gzipped CSV file
    ///
    /// # Arguments
    /// * `path` - Path to the .csv.gz file
    ///
    /// # Returns
    /// Vector of trades sorted by timestamp
    pub fn load_trades<P: AsRef<Path>>(path: P) -> Result<Vec<TardisTrade>, TardisError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TardisError::FileNotFound(path.display().to_string()));
        }

        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(1024 * 1024, decoder); // 1MB buffer

        let mut trades = Vec::with_capacity(100_000);

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result?;
            if line.is_empty() {
                continue;
            }

            match Self::parse_trade_line(&line) {
                Ok(trade) => trades.push(trade),
                Err(msg) => {
                    // Log warning but continue - some lines might be malformed
                    if line_num < 10 {
                        tracing::warn!("Skipping line {}: {}", line_num + 1, msg);
                    }
                }
            }
        }

        // Sort by timestamp (should already be sorted, but ensure it)
        trades.sort_by_key(|t| t.timestamp_us);

        tracing::info!("Loaded {} trades from {}", trades.len(), path.display());
        Ok(trades)
    }

    /// Load orderbook snapshots from a gzipped CSV file
    ///
    /// # Arguments
    /// * `path` - Path to the .csv.gz file
    ///
    /// # Returns
    /// Vector of orderbook snapshots sorted by timestamp
    pub fn load_orderbook<P: AsRef<Path>>(
        path: P,
    ) -> Result<Vec<TardisOrderbookSnapshot>, TardisError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TardisError::FileNotFound(path.display().to_string()));
        }

        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(1024 * 1024, decoder);

        let mut snapshots = Vec::with_capacity(1_000_000);

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result?;
            if line.is_empty() {
                continue;
            }

            match Self::parse_orderbook_line(&line) {
                Ok(snapshot) => snapshots.push(snapshot),
                Err(msg) => {
                    if line_num < 10 {
                        tracing::warn!("Skipping line {}: {}", line_num + 1, msg);
                    }
                }
            }
        }

        snapshots.sort_by_key(|s| s.timestamp_us);

        tracing::info!(
            "Loaded {} orderbook snapshots from {}",
            snapshots.len(),
            path.display()
        );
        Ok(snapshots)
    }

    /// Parse a single trade line
    /// Format: exchange,symbol,timestamp,local_timestamp,id,side,price,amount
    fn parse_trade_line(line: &str) -> Result<TardisTrade, String> {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 8 {
            return Err(format!("Expected 8 fields, got {}", parts.len()));
        }

        let timestamp_us: i64 = parts[2]
            .parse()
            .map_err(|_| format!("Invalid timestamp: {}", parts[2]))?;
        let local_timestamp_ns: i64 = parts[3]
            .parse()
            .map_err(|_| format!("Invalid local_timestamp: {}", parts[3]))?;
        let trade_id: i64 = parts[4]
            .parse()
            .map_err(|_| format!("Invalid trade_id: {}", parts[4]))?;
        let side = match parts[5].to_lowercase().as_str() {
            "sell" | "s" | "ask" | "a" => Side::Sell,
            _ => Side::Buy,
        };
        let price: f64 = parts[6]
            .parse()
            .map_err(|_| format!("Invalid price: {}", parts[6]))?;
        let amount: f64 = parts[7]
            .parse()
            .map_err(|_| format!("Invalid amount: {}", parts[7]))?;

        Ok(TardisTrade {
            timestamp_us,
            local_timestamp_ns,
            trade_id,
            side,
            price,
            amount,
        })
    }

    /// Parse a single orderbook snapshot line
    /// Format: exchange,symbol,timestamp,local_timestamp,asks[0].price,asks[0].amount,...(25 levels),bids[0].price,bids[0].amount,...(25 levels)
    fn parse_orderbook_line(line: &str) -> Result<TardisOrderbookSnapshot, String> {
        let parts: Vec<&str> = line.split(',').collect();
        // 4 header fields + 25 ask levels * 2 + 25 bid levels * 2 = 4 + 50 + 50 = 104
        if parts.len() < 104 {
            return Err(format!("Expected 104 fields, got {}", parts.len()));
        }

        let timestamp_us: i64 = parts[2]
            .parse()
            .map_err(|_| format!("Invalid timestamp: {}", parts[2]))?;
        let local_timestamp_ns: i64 = parts[3]
            .parse()
            .map_err(|_| format!("Invalid local_timestamp: {}", parts[3]))?;

        let mut asks = [PriceLevel::default(); 25];
        let mut bids = [PriceLevel::default(); 25];

        // Parse 25 ask levels starting at index 4
        for i in 0..25 {
            let base = 4 + i * 2;
            asks[i].price = parts[base].parse().unwrap_or(0.0);
            asks[i].amount = parts[base + 1].parse().unwrap_or(0.0);
        }

        // Parse 25 bid levels starting at index 54
        for i in 0..25 {
            let base = 54 + i * 2;
            bids[i].price = parts[base].parse().unwrap_or(0.0);
            bids[i].amount = parts[base + 1].parse().unwrap_or(0.0);
        }

        Ok(TardisOrderbookSnapshot {
            timestamp_us,
            local_timestamp_ns,
            asks,
            bids,
        })
    }

    /// Load trades with time range filter (avoids loading everything into memory)
    ///
    /// # Arguments
    /// * `path` - Path to the .csv.gz file
    /// * `start` - Start timestamp (inclusive)
    /// * `end` - End timestamp (exclusive)
    pub fn load_trades_range<P: AsRef<Path>>(
        path: P,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TardisTrade>, TardisError> {
        let start_us = start.timestamp_micros();
        let end_us = end.timestamp_micros();

        let path = path.as_ref();
        if !path.exists() {
            return Err(TardisError::FileNotFound(path.display().to_string()));
        }

        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(1024 * 1024, decoder);

        let mut trades = Vec::with_capacity(50_000);

        for line_result in reader.lines() {
            let line = line_result?;
            if line.is_empty() {
                continue;
            }

            if let Ok(trade) = Self::parse_trade_line(&line) {
                if trade.timestamp_us >= start_us && trade.timestamp_us < end_us {
                    trades.push(trade);
                } else if trade.timestamp_us >= end_us {
                    // Data is sorted, so we can stop early
                    break;
                }
            }
        }

        trades.sort_by_key(|t| t.timestamp_us);
        Ok(trades)
    }

    /// Load orderbook snapshots with time range filter
    pub fn load_orderbook_range<P: AsRef<Path>>(
        path: P,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TardisOrderbookSnapshot>, TardisError> {
        let start_us = start.timestamp_micros();
        let end_us = end.timestamp_micros();

        let path = path.as_ref();
        if !path.exists() {
            return Err(TardisError::FileNotFound(path.display().to_string()));
        }

        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(1024 * 1024, decoder);

        let mut snapshots = Vec::with_capacity(100_000);

        for line_result in reader.lines() {
            let line = line_result?;
            if line.is_empty() {
                continue;
            }

            if let Ok(snapshot) = Self::parse_orderbook_line(&line) {
                if snapshot.timestamp_us >= start_us && snapshot.timestamp_us < end_us {
                    snapshots.push(snapshot);
                } else if snapshot.timestamp_us >= end_us {
                    break;
                }
            }
        }

        snapshots.sort_by_key(|s| s.timestamp_us);
        Ok(snapshots)
    }
    
    /// Convert orderbook snapshots + trades into unified OrderBookEvent stream
    /// 
    /// This function:
    /// 1. Diffs consecutive orderbook snapshots to generate NEW/CANCEL events
    /// 2. Interleaves trade events with orderbook events by timestamp
    /// 3. Returns a unified event stream for the RealisticSimulator
    /// 
    /// # Arguments
    /// * `snapshots` - L2 orderbook snapshots (25 levels)
    /// * `trades` - Trade executions
    /// 
    /// # Returns
    /// Vector of OrderBookEvent sorted by timestamp
    pub fn to_order_events(
        snapshots: &[TardisOrderbookSnapshot],
        trades: &[TardisTrade],
    ) -> Vec<TardisOrderEvent> {
        let mut events = Vec::with_capacity(snapshots.len() * 50 + trades.len());
        
        // Track previous snapshot state for diffing
        let mut prev_bids: std::collections::HashMap<i64, (f64, f64)> = std::collections::HashMap::new(); // price_key -> (price, amount)
        let mut prev_asks: std::collections::HashMap<i64, (f64, f64)> = std::collections::HashMap::new();
        
        // Helper: convert price to integer key (8 decimal precision)
        let price_to_key = |p: f64| -> i64 { (p * 100_000_000.0) as i64 };
        
        // Track order ID counter for synthetic order IDs
        let mut order_id_counter: u64 = 0;
        
        for snapshot in snapshots {
            let ts_ms = snapshot.timestamp_us / 1000;
            
            // Build current state
            let mut curr_bids: std::collections::HashMap<i64, (f64, f64)> = std::collections::HashMap::new();
            let mut curr_asks: std::collections::HashMap<i64, (f64, f64)> = std::collections::HashMap::new();
            
            for level in &snapshot.bids {
                if level.price > 0.0 && level.amount > 0.0 {
                    curr_bids.insert(price_to_key(level.price), (level.price, level.amount));
                }
            }
            for level in &snapshot.asks {
                if level.price > 0.0 && level.amount > 0.0 {
                    curr_asks.insert(price_to_key(level.price), (level.price, level.amount));
                }
            }
            
            // Diff bids: find cancelled (in prev, not in curr) and new (in curr, not in prev or changed)
            for (key, (price, _amount)) in &prev_bids {
                if !curr_bids.contains_key(key) {
                    // Level removed - generate CANCEL
                    order_id_counter += 1;
                    events.push(TardisOrderEvent {
                        timestamp_ms: ts_ms,
                        event_type: TardisEventType::Cancel,
                        order_id: format!("B{}", order_id_counter),
                        side: Side::Buy,
                        price: *price,
                        quantity: 0.0,
                    });
                }
            }
            for (key, (price, amount)) in &curr_bids {
                match prev_bids.get(key) {
                    None => {
                        // New level - generate NEW
                        order_id_counter += 1;
                        events.push(TardisOrderEvent {
                            timestamp_ms: ts_ms,
                            event_type: TardisEventType::New,
                            order_id: format!("B{}", order_id_counter),
                            side: Side::Buy,
                            price: *price,
                            quantity: *amount,
                        });
                    }
                    Some((_, prev_amount)) if (amount - prev_amount).abs() > 1e-10 => {
                        // Quantity changed - generate MODIFY
                        order_id_counter += 1;
                        events.push(TardisOrderEvent {
                            timestamp_ms: ts_ms,
                            event_type: TardisEventType::Modify,
                            order_id: format!("B{}", order_id_counter),
                            side: Side::Buy,
                            price: *price,
                            quantity: *amount,
                        });
                    }
                    _ => {} // No change
                }
            }
            
            // Diff asks similarly
            for (key, (price, _amount)) in &prev_asks {
                if !curr_asks.contains_key(key) {
                    order_id_counter += 1;
                    events.push(TardisOrderEvent {
                        timestamp_ms: ts_ms,
                        event_type: TardisEventType::Cancel,
                        order_id: format!("A{}", order_id_counter),
                        side: Side::Sell,
                        price: *price,
                        quantity: 0.0,
                    });
                }
            }
            for (key, (price, amount)) in &curr_asks {
                match prev_asks.get(key) {
                    None => {
                        order_id_counter += 1;
                        events.push(TardisOrderEvent {
                            timestamp_ms: ts_ms,
                            event_type: TardisEventType::New,
                            order_id: format!("A{}", order_id_counter),
                            side: Side::Sell,
                            price: *price,
                            quantity: *amount,
                        });
                    }
                    Some((_, prev_amount)) if (amount - prev_amount).abs() > 1e-10 => {
                        order_id_counter += 1;
                        events.push(TardisOrderEvent {
                            timestamp_ms: ts_ms,
                            event_type: TardisEventType::Modify,
                            order_id: format!("A{}", order_id_counter),
                            side: Side::Sell,
                            price: *price,
                            quantity: *amount,
                        });
                    }
                    _ => {}
                }
            }
            
            prev_bids = curr_bids;
            prev_asks = curr_asks;
        }
        
        // Add trade events
        for trade in trades {
            events.push(TardisOrderEvent {
                timestamp_ms: trade.timestamp_us / 1000,
                event_type: TardisEventType::Trade,
                order_id: trade.trade_id.to_string(),
                side: trade.side,
                price: trade.price,
                quantity: trade.amount,
            });
        }
        
        // Sort all events by timestamp
        events.sort_by_key(|e| e.timestamp_ms);
        
        tracing::info!(
            "Generated {} order events from {} snapshots + {} trades",
            events.len(),
            snapshots.len(),
            trades.len()
        );
        
        events
    }
}

/// Event type for Tardis-derived order events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TardisEventType {
    New,
    Modify,
    Cancel,
    Trade,
}

/// Unified order event derived from Tardis data
#[derive(Debug, Clone)]
pub struct TardisOrderEvent {
    pub timestamp_ms: i64,
    pub event_type: TardisEventType,
    pub order_id: String,
    pub side: Side,
    pub price: f64,
    pub quantity: f64,
}

/// Iterator adapter for streaming trades without loading all into memory
pub struct TardisTradeIterator {
    reader: std::io::Lines<BufReader<GzDecoder<File>>>,
    line_num: usize,
}

impl TardisTradeIterator {
    /// Create a new streaming trade iterator
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, TardisError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(TardisError::FileNotFound(path.display().to_string()));
        }

        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(512 * 1024, decoder);

        Ok(Self {
            reader: reader.lines(),
            line_num: 0,
        })
    }
}

impl Iterator for TardisTradeIterator {
    type Item = Result<TardisTrade, TardisError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.reader.next() {
                Some(Ok(line)) => {
                    self.line_num += 1;
                    if line.is_empty() {
                        continue;
                    }
                    match TardisLoader::parse_trade_line(&line) {
                        Ok(trade) => return Some(Ok(trade)),
                        Err(msg) => {
                            return Some(Err(TardisError::Parse {
                                line: self.line_num,
                                message: msg,
                            }))
                        }
                    }
                }
                Some(Err(e)) => return Some(Err(TardisError::Io(e))),
                None => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trade_line() {
        let line = "binance-us,BTCUSD,1767150962413000,1767150962454098,89025727,buy,88404.76,0.0262";
        let trade = TardisLoader::parse_trade_line(line).unwrap();
        assert_eq!(trade.timestamp_us, 1767150962413000);
        assert_eq!(trade.trade_id, 89025727);
        assert!(matches!(trade.side, Side::Buy));
        assert!((trade.price - 88404.76).abs() < 0.001);
        assert!((trade.amount - 0.0262).abs() < 0.0001);
    }

    #[test]
    fn test_trade_tick_impl() {
        let trade = TardisTrade {
            timestamp_us: 1767150962413000,
            local_timestamp_ns: 1767150962454098,
            trade_id: 89025727,
            side: Side::Sell,
            price: 88404.76,
            amount: 0.0262,
        };

        assert!(trade.is_sell());
        assert!(!trade.is_buy());
        assert_eq!(trade.event_type(), 3);
        assert!((trade.value() - 2316.20).abs() < 0.1);
    }

    #[test]
    fn test_orderbook_methods() {
        let mut snapshot = TardisOrderbookSnapshot {
            timestamp_us: 1767150962413000,
            local_timestamp_ns: 1767150962454098,
            asks: [PriceLevel::default(); 25],
            bids: [PriceLevel::default(); 25],
        };

        // Set up some test levels
        snapshot.asks[0] = PriceLevel {
            price: 100.0,
            amount: 1.0,
        };
        snapshot.asks[1] = PriceLevel {
            price: 101.0,
            amount: 2.0,
        };
        snapshot.bids[0] = PriceLevel {
            price: 99.0,
            amount: 1.5,
        };
        snapshot.bids[1] = PriceLevel {
            price: 98.0,
            amount: 2.5,
        };

        assert_eq!(snapshot.best_ask(), 100.0);
        assert_eq!(snapshot.best_bid(), 99.0);
        assert_eq!(snapshot.mid_price(), 99.5);
        assert_eq!(snapshot.spread(), 1.0);

        // 2 levels: bids = 1.5 + 2.5 = 4.0, asks = 1.0 + 2.0 = 3.0
        let imbalance = snapshot.imbalance(2);
        assert!((imbalance - (4.0 - 3.0) / 7.0).abs() < 0.001);
    }
}
