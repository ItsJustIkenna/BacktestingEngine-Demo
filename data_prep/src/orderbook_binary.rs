//! Binary format for order book events
//!
//! This format stores all order book event types (new, modify, cancel, trade)
//! for realistic order book reconstruction and simulation.
//!
//! File structure:
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Header (256 bytes)                                          │
//! │  - Magic number: "OBEVENTS" (8 bytes)                       │
//! │  - Version: u32 (4 bytes)                                   │
//! │  - Event count: u64 (8 bytes)                               │
//! │  - Event size: u32 (4 bytes) - sizeof(OrderBookEventTick)   │
//! │  - Symbol: [u8; 32] - null-terminated string                │
//! │  - Exchange: [u8; 32] - null-terminated string              │
//! │  - Start timestamp: i64 (8 bytes)                           │
//! │  - End timestamp: i64 (8 bytes)                             │
//! │  - Reserved: padding to 256 bytes                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Data section                                                │
//! │  - Array of OrderBookEventTick (event_count * event_size)   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use anyhow::{anyhow, Result};
use memmap2::{Mmap, MmapOptions};
use bytemuck::{Pod, Zeroable};

/// Magic number for file identification
const MAGIC: [u8; 8] = *b"OBEVENTS";

/// Current file format version
const VERSION: u32 = 1;

/// Header size (fixed for forward compatibility)
pub const HEADER_SIZE: usize = 256;

/// Event type encoded as u8
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct EventTypeCode(pub u8);

impl EventTypeCode {
    pub const NEW: Self = Self(0);
    pub const MODIFY: Self = Self(1);
    pub const CANCEL: Self = Self(2);
    pub const TRADE: Self = Self(3);
    
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "NEW" | "ADD" => Self::NEW,
            "MODIFY" | "UPDATE" => Self::MODIFY,
            "CANCEL" | "DELETE" => Self::CANCEL,
            "TRADE" | "FILL" | "EXECUTION" => Self::TRADE,
            _ => Self::NEW, // Default
        }
    }
    
    pub fn to_str(&self) -> &'static str {
        match self.0 {
            0 => "new",
            1 => "modify",
            2 => "cancel",
            3 => "trade",
            _ => "unknown",
        }
    }
}

/// Side encoded as u8
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct SideCode(pub u8);

impl SideCode {
    pub const BUY: Self = Self(0);
    pub const SELL: Self = Self(1);
    
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "BUY" | "BID" | "B" => Self::BUY,
            "SELL" | "ASK" | "S" | "OFFER" => Self::SELL,
            _ => Self::BUY, // Default
        }
    }
    
    pub fn is_buy(&self) -> bool {
        self.0 == 0
    }
    
    pub fn is_sell(&self) -> bool {
        self.0 == 1
    }
}

/// Order book event stored in binary format
/// 
/// Layout (64 bytes):
/// - timestamp_ms: i64 (8 bytes) - Unix milliseconds
/// - order_id_hash: u64 (8 bytes) - Hash of order ID for matching
/// - price: f64 (8 bytes) - Price level
/// - quantity: f64 (8 bytes) - Order quantity
/// - prev_price: f64 (8 bytes) - Previous price (for modify events)
/// - prev_quantity: f64 (8 bytes) - Previous quantity (for modify events)
/// - event_type: u8 (1 byte) - Event type code
/// - side: u8 (1 byte) - Side code
/// - _padding: [u8; 14] - Alignment padding
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct OrderBookEventTick {
    pub timestamp_ms: i64,
    pub order_id_hash: u64,
    pub price: f64,
    pub quantity: f64,
    pub prev_price: f64,
    pub prev_quantity: f64,
    pub event_type: EventTypeCode,
    pub side: SideCode,
    pub _padding: [u8; 14],
}

// Verify size at compile time
const _: () = assert!(std::mem::size_of::<OrderBookEventTick>() == 64);

impl OrderBookEventTick {
    pub fn new(
        timestamp_ms: i64,
        order_id: &str,
        event_type: &str,
        side: &str,
        price: f64,
        quantity: f64,
        prev_price: Option<f64>,
        prev_quantity: Option<f64>,
    ) -> Self {
        // Simple hash of order_id for fast comparison
        let order_id_hash = Self::hash_order_id(order_id);
        
        Self {
            timestamp_ms,
            order_id_hash,
            price,
            quantity,
            prev_price: prev_price.unwrap_or(0.0),
            prev_quantity: prev_quantity.unwrap_or(0.0),
            event_type: EventTypeCode::from_str(event_type),
            side: SideCode::from_str(side),
            _padding: [0; 14],
        }
    }
    
    fn hash_order_id(order_id: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        order_id.hash(&mut hasher);
        hasher.finish()
    }
    
    pub fn is_new(&self) -> bool {
        self.event_type == EventTypeCode::NEW
    }
    
    pub fn is_modify(&self) -> bool {
        self.event_type == EventTypeCode::MODIFY
    }
    
    pub fn is_cancel(&self) -> bool {
        self.event_type == EventTypeCode::CANCEL
    }
    
    pub fn is_trade(&self) -> bool {
        self.event_type == EventTypeCode::TRADE
    }
    
    pub fn is_buy(&self) -> bool {
        self.side.is_buy()
    }
    
    pub fn is_sell(&self) -> bool {
        self.side.is_sell()
    }
}

/// Binary file header for order book events
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct OrderBookBinaryHeader {
    pub magic: [u8; 8],
    pub event_count: u64,
    pub start_timestamp_ms: i64,
    pub end_timestamp_ms: i64,
    pub file_size_bytes: u64,
    pub symbol: [u8; 32],
    pub exchange: [u8; 32],
    pub version: u32,
    pub event_size: u32,
    pub _reserved: [u8; 144],
}

// Verify header size at compile time
const _: () = assert!(std::mem::size_of::<OrderBookBinaryHeader>() == HEADER_SIZE);

impl OrderBookBinaryHeader {
    pub fn zeroed() -> Self {
        Self {
            magic: [0; 8],
            event_count: 0,
            start_timestamp_ms: 0,
            end_timestamp_ms: 0,
            file_size_bytes: 0,
            symbol: [0; 32],
            exchange: [0; 32],
            version: 0,
            event_size: 0,
            _reserved: [0; 144],
        }
    }
    
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        unsafe { std::mem::transmute_copy(self) }
    }
    
    pub fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> Self {
        unsafe { std::mem::transmute_copy(bytes) }
    }
    
    pub fn new(symbol: &str, exchange: &str) -> Self {
        let mut header = Self::zeroed();
        header.magic = MAGIC;
        header.version = VERSION;
        header.event_size = std::mem::size_of::<OrderBookEventTick>() as u32;
        
        // Copy symbol
        let symbol_bytes = symbol.as_bytes();
        let len = symbol_bytes.len().min(31);
        header.symbol[..len].copy_from_slice(&symbol_bytes[..len]);
        
        // Copy exchange
        let exchange_bytes = exchange.as_bytes();
        let len = exchange_bytes.len().min(31);
        header.exchange[..len].copy_from_slice(&exchange_bytes[..len]);
        
        header
    }
    
    pub fn validate(&self) -> Result<()> {
        if self.magic != MAGIC {
            return Err(anyhow!("Invalid magic number"));
        }
        // Copy values from packed struct before using
        let version = self.version;
        let event_size = self.event_size;
        if version != VERSION {
            return Err(anyhow!("Unsupported version: {} (expected {})", version, VERSION));
        }
        if event_size != std::mem::size_of::<OrderBookEventTick>() as u32 {
            return Err(anyhow!("Event size mismatch: {} (expected {})", 
                event_size, std::mem::size_of::<OrderBookEventTick>()));
        }
        Ok(())
    }
}

/// Writer for order book binary files
pub struct OrderBookBinaryWriter {
    writer: BufWriter<File>,
    header: OrderBookBinaryHeader,
    event_count: u64,
    first_timestamp: Option<i64>,
    last_timestamp: i64,
}

impl OrderBookBinaryWriter {
    pub fn create<P: AsRef<Path>>(path: P, symbol: &str, exchange: &str) -> Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        
        let header = OrderBookBinaryHeader::new(symbol, exchange);
        
        // Write placeholder header (will update at finalize)
        writer.write_all(&header.to_bytes())?;
        
        Ok(Self {
            writer,
            header,
            event_count: 0,
            first_timestamp: None,
            last_timestamp: 0,
        })
    }
    
    pub fn write_event(&mut self, event: &OrderBookEventTick) -> Result<()> {
        let bytes: &[u8] = bytemuck::bytes_of(event);
        self.writer.write_all(bytes)?;
        
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(event.timestamp_ms);
        }
        self.last_timestamp = event.timestamp_ms;
        self.event_count += 1;
        
        Ok(())
    }
    
    /// Write a batch of events efficiently
    pub fn write_events(&mut self, events: &[OrderBookEventTick]) -> Result<()> {
        for event in events {
            self.write_event(event)?;
        }
        Ok(())
    }
    
    pub fn finalize(mut self) -> Result<OrderBookBinaryHeader> {
        self.writer.flush()?;
        
        // Get file size
        let file_size = self.writer.seek(SeekFrom::End(0))?;
        
        // Update header
        self.header.event_count = self.event_count;
        self.header.start_timestamp_ms = self.first_timestamp.unwrap_or(0);
        self.header.end_timestamp_ms = self.last_timestamp;
        self.header.file_size_bytes = file_size;
        
        // Seek back to start and write final header
        self.writer.seek(SeekFrom::Start(0))?;
        self.writer.write_all(&self.header.to_bytes())?;
        self.writer.flush()?;
        
        Ok(self.header)
    }
}

/// Reader for order book binary files (memory-mapped)
pub struct OrderBookBinaryReader {
    mmap: Mmap,
    header: OrderBookBinaryHeader,
}

impl OrderBookBinaryReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        
        if mmap.len() < HEADER_SIZE {
            return Err(anyhow!("File too small for header"));
        }
        
        let header_bytes: [u8; HEADER_SIZE] = mmap[..HEADER_SIZE].try_into()?;
        let header = OrderBookBinaryHeader::from_bytes(&header_bytes);
        header.validate()?;
        
        Ok(Self { mmap, header })
    }
    
    pub fn len(&self) -> usize {
        self.header.event_count as usize
    }
    
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    pub fn get(&self, index: usize) -> Option<&OrderBookEventTick> {
        if index >= self.len() {
            return None;
        }
        
        let offset = HEADER_SIZE + index * std::mem::size_of::<OrderBookEventTick>();
        let end = offset + std::mem::size_of::<OrderBookEventTick>();
        
        if end > self.mmap.len() {
            return None;
        }
        
        let event: &OrderBookEventTick = bytemuck::from_bytes(&self.mmap[offset..end]);
        Some(event)
    }
    
    pub fn events(&self) -> &[OrderBookEventTick] {
        if self.len() == 0 {
            return &[];
        }
        
        let data = &self.mmap[HEADER_SIZE..];
        bytemuck::cast_slice(data)
    }
    
    pub fn symbol(&self) -> String {
        let bytes = &self.header.symbol;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).to_string()
    }
    
    pub fn exchange(&self) -> String {
        let bytes = &self.header.exchange;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_write_read_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_events.bin");
        
        // Write events
        {
            let mut writer = OrderBookBinaryWriter::create(&path, "BTC/USD", "Kraken").unwrap();
            
            let event1 = OrderBookEventTick::new(
                1000, "order1", "new", "buy", 100.0, 1.0, None, None
            );
            writer.write_event(&event1).unwrap();
            
            let event2 = OrderBookEventTick::new(
                2000, "order2", "trade", "sell", 101.0, 0.5, None, None
            );
            writer.write_event(&event2).unwrap();
            
            let header = writer.finalize().unwrap();
            let event_count = header.event_count;
            assert_eq!(event_count, 2);
        }
        
        // Read events
        let reader = OrderBookBinaryReader::open(&path).unwrap();
        assert_eq!(reader.len(), 2);
        assert_eq!(reader.symbol(), "BTC/USD");
        assert_eq!(reader.exchange(), "Kraken");
        
        let event1 = reader.get(0).unwrap();
        assert!(event1.is_new());
        assert!(event1.is_buy());
        assert_eq!(event1.price, 100.0);
        
        let event2 = reader.get(1).unwrap();
        assert!(event2.is_trade());
        assert!(event2.is_sell());
        assert_eq!(event2.price, 101.0);
    }
}
