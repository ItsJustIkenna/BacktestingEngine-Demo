//! Binary file format for simulation data
//!
//! File structure:
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Header (256 bytes)                                          │
//! │  - Magic number: "SIMTICK\0" (8 bytes)                      │
//! │  - Version: u32 (4 bytes)                                   │
//! │  - Tick count: u64 (8 bytes)                                │
//! │  - Tick size: u32 (4 bytes) - sizeof(SimulationTick)        │
//! │  - Symbol: [u8; 32] - null-terminated string                │
//! │  - Exchange: [u8; 32] - null-terminated string              │
//! │  - Start timestamp: i64 (8 bytes)                           │
//! │  - End timestamp: i64 (8 bytes)                             │
//! │  - Features computed: u8 (1 byte)                           │
//! │  - Compressed: u8 (1 byte)                                  │
//! │  - Reserved: padding to 256 bytes                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Data section                                                │
//! │  - Array of SimulationTick (tick_count * tick_size bytes)   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use anyhow::{anyhow, Result};
use memmap2::{Mmap, MmapOptions};

use crate::tick::SimulationTick;

/// Magic number for file identification
const MAGIC: [u8; 8] = *b"SIMTICK\0";

/// Current file format version
const VERSION: u32 = 1;

/// Header size (fixed for forward compatibility)
pub const HEADER_SIZE: usize = 256;

/// Binary file header
/// 
/// Layout is designed to avoid hidden padding by ordering fields from largest to smallest alignment.
/// We use manual serialization to ensure exact binary format.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BinaryFileHeader {
    /// Magic number for file identification
    pub magic: [u8; 8],            // 8 bytes, offset 0
    /// Number of ticks in file
    pub tick_count: u64,           // 8 bytes, offset 8
    /// First tick timestamp (Unix ms)
    pub start_timestamp_ms: i64,   // 8 bytes, offset 16
    /// Last tick timestamp (Unix ms)
    pub end_timestamp_ms: i64,     // 8 bytes, offset 24
    /// File size in bytes (for quick validation)
    pub file_size_bytes: u64,      // 8 bytes, offset 32
    /// Symbol (null-terminated)
    pub symbol: [u8; 32],          // 32 bytes, offset 40
    /// Exchange (null-terminated)
    pub exchange: [u8; 32],        // 32 bytes, offset 72
    /// File format version
    pub version: u32,              // 4 bytes, offset 104
    /// Size of each tick in bytes
    pub tick_size: u32,            // 4 bytes, offset 108
    /// Whether features have been computed
    pub features_computed: u8,     // 1 byte, offset 112
    /// Whether data is compressed (future use)
    pub compressed: u8,            // 1 byte, offset 113
    /// Reserved for future use
    pub _reserved: [u8; 142],      // 142 bytes, offset 114
}                                  // Total: 256 bytes

// Verify header size at compile time
const _: () = assert!(std::mem::size_of::<BinaryFileHeader>() == HEADER_SIZE);

impl BinaryFileHeader {
    /// Create a zeroed header
    pub fn zeroed() -> Self {
        Self {
            magic: [0; 8],
            tick_count: 0,
            start_timestamp_ms: 0,
            end_timestamp_ms: 0,
            file_size_bytes: 0,
            symbol: [0; 32],
            exchange: [0; 32],
            version: 0,
            tick_size: 0,
            features_computed: 0,
            compressed: 0,
            _reserved: [0; 142],
        }
    }
    
    /// Convert header to bytes for writing
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        // Safety: We're using repr(C, packed) and the struct is exactly HEADER_SIZE bytes
        unsafe { std::mem::transmute_copy(self) }
    }
    
    /// Create header from bytes
    pub fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> Self {
        // Safety: We're using repr(C, packed) and assuming the bytes are valid
        unsafe { std::mem::transmute_copy(bytes) }
    }
    
    /// Create a new header
    pub fn new(symbol: &str, exchange: &str) -> Self {
        let mut header = Self::zeroed();
        header.magic = MAGIC;
        header.version = VERSION;
        header.tick_size = std::mem::size_of::<SimulationTick>() as u32;
        
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
    
    /// Get symbol as string
    pub fn symbol_str(&self) -> &str {
        let end = self.symbol.iter().position(|&b| b == 0).unwrap_or(32);
        std::str::from_utf8(&self.symbol[..end]).unwrap_or("")
    }
    
    /// Get exchange as string
    pub fn exchange_str(&self) -> &str {
        let end = self.exchange.iter().position(|&b| b == 0).unwrap_or(32);
        std::str::from_utf8(&self.exchange[..end]).unwrap_or("")
    }
    
    /// Validate header
    pub fn validate(&self) -> Result<()> {
        if self.magic != MAGIC {
            return Err(anyhow!("Invalid magic number"));
        }
        // Copy to local variables to avoid packed field alignment issues
        let version = self.version;
        if version > VERSION {
            return Err(anyhow!("Unsupported version: {} (max: {})", version, VERSION));
        }
        let tick_size = self.tick_size;
        let expected_tick_size = std::mem::size_of::<SimulationTick>() as u32;
        if tick_size != expected_tick_size {
            return Err(anyhow!(
                "Tick size mismatch: file has {}, expected {}",
                tick_size,
                expected_tick_size
            ));
        }
        Ok(())
    }
}

/// Writer for binary simulation data files
pub struct BinaryFileWriter {
    writer: BufWriter<File>,
    header: BinaryFileHeader,
    ticks_written: u64,
    first_timestamp: Option<i64>,
    last_timestamp: Option<i64>,
}

impl BinaryFileWriter {
    /// Create a new binary file for writing
    pub fn create(path: &Path) -> Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::with_capacity(1024 * 1024, file); // 1MB buffer
        
        // Write placeholder header (will update on finalize)
        let header = BinaryFileHeader::zeroed();
        writer.write_all(&header.to_bytes())?;
        
        Ok(Self {
            writer,
            header: BinaryFileHeader::new("", ""),
            ticks_written: 0,
            first_timestamp: None,
            last_timestamp: None,
        })
    }
    
    /// Set symbol and exchange metadata
    pub fn set_metadata(&mut self, symbol: &str, exchange: &str) {
        self.header = BinaryFileHeader::new(symbol, exchange);
    }
    
    /// Write a chunk of ticks
    pub fn write_ticks(&mut self, ticks: &[SimulationTick]) -> Result<()> {
        if ticks.is_empty() {
            return Ok(());
        }
        
        // Track timestamps
        if self.first_timestamp.is_none() {
            self.first_timestamp = Some(ticks[0].timestamp_ms);
        }
        self.last_timestamp = Some(ticks.last().unwrap().timestamp_ms);
        
        // Write tick data
        let bytes = bytemuck::cast_slice(ticks);
        self.writer.write_all(bytes)?;
        
        self.ticks_written += ticks.len() as u64;
        Ok(())
    }
    
    /// Finalize the file and write the header
    pub fn finalize(mut self) -> Result<BinaryFileHeader> {
        self.writer.flush()?;
        
        // Get file size
        let file = self.writer.into_inner()?;
        let file_size = file.metadata()?.len();
        
        // Prepare final header
        self.header.tick_count = self.ticks_written;
        self.header.start_timestamp_ms = self.first_timestamp.unwrap_or(0);
        self.header.end_timestamp_ms = self.last_timestamp.unwrap_or(0);
        self.header.file_size_bytes = file_size;
        
        // Seek to beginning and write header
        let mut file = file;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&self.header.to_bytes())?;
        file.sync_all()?;
        
        Ok(self.header)
    }
}

/// Reader for binary simulation data files
pub struct BinaryFileReader {
    mmap: Mmap,
    header: BinaryFileHeader,
}

impl BinaryFileReader {
    /// Open a binary file for reading
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };
        
        if mmap.len() < HEADER_SIZE {
            return Err(anyhow!("File too small for header"));
        }
        
        // Read and validate header
        let header_bytes: [u8; HEADER_SIZE] = mmap[..HEADER_SIZE].try_into()
            .map_err(|_| anyhow!("Failed to read header"))?;
        let header = BinaryFileHeader::from_bytes(&header_bytes);
        header.validate()?;
        
        // Validate file size
        let expected_size = HEADER_SIZE + (header.tick_count as usize * header.tick_size as usize);
        if mmap.len() < expected_size {
            return Err(anyhow!(
                "File truncated: expected {} bytes, got {}",
                expected_size,
                mmap.len()
            ));
        }
        
        Ok(Self { mmap, header })
    }
    
    /// Get file header
    pub fn header(&self) -> &BinaryFileHeader {
        &self.header
    }
    
    /// Get total tick count
    pub fn tick_count(&self) -> u64 {
        self.header.tick_count
    }
    
    /// Get all ticks as a slice (zero-copy)
    pub fn ticks(&self) -> &[SimulationTick] {
        let data = &self.mmap[HEADER_SIZE..];
        bytemuck::cast_slice(data)
    }
    
    /// Get a range of ticks
    pub fn ticks_range(&self, start: usize, end: usize) -> &[SimulationTick] {
        let all = self.ticks();
        let end = end.min(all.len());
        let start = start.min(end);
        &all[start..end]
    }
    
    /// Iterate over ticks in chunks (for memory-constrained processing)
    pub fn chunks(&self, chunk_size: usize) -> impl Iterator<Item = &[SimulationTick]> {
        self.ticks().chunks(chunk_size)
    }
}

// Implement TickSource for BinaryFileReader (zero-copy mmap access)
impl dataloader::TickSource for BinaryFileReader {
    type TickType = SimulationTick;
    
    fn len(&self) -> usize {
        self.header.tick_count as usize
    }
    
    fn get(&self, index: usize) -> Option<&Self::TickType> {
        self.ticks().get(index)
    }
    
    fn iter(&self) -> impl Iterator<Item = &Self::TickType> {
        self.ticks().iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");
        
        // Write
        let mut writer = BinaryFileWriter::create(&path).unwrap();
        writer.set_metadata("BTC/USD", "Kraken");
        
        let ticks = vec![
            SimulationTick::new(1000, 50000.0, 1.0, false),
            SimulationTick::new(2000, 50100.0, 0.5, true),
            SimulationTick::new(3000, 50050.0, 2.0, false),
        ];
        writer.write_ticks(&ticks).unwrap();
        let header = writer.finalize().unwrap();
        
        // Copy packed fields to local variables to avoid unaligned access
        let tick_count = header.tick_count;
        assert_eq!(tick_count, 3);
        assert_eq!(header.symbol_str(), "BTC/USD");
        assert_eq!(header.exchange_str(), "Kraken");
        
        // Read
        let reader = BinaryFileReader::open(&path).unwrap();
        assert_eq!(reader.tick_count(), 3);
        
        let read_ticks = reader.ticks();
        assert_eq!(read_ticks.len(), 3);
        assert_eq!(read_ticks[0].timestamp_ms, 1000);
        assert!((read_ticks[1].price - 50100.0).abs() < 0.001);
        assert!(read_ticks[1].is_sell());
    }
}
