//! Chunk calculation for distributed backtesting
//!
//! This module provides algorithms for splitting large backtests into
//! smaller, independently processable chunks.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::{DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_HOURS, MAX_CHUNKS_PER_JOB};

/// Strategy for splitting a backtest into chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkingStrategy {
    /// Split by approximate event count (preferred for load balancing)
    ByEventCount {
        target_events_per_chunk: usize,
    },
    /// Split by time duration (simpler but less balanced)
    ByDuration {
        hours_per_chunk: i64,
    },
    /// Split by trading days (natural market boundaries)
    ByTradingDays {
        days_per_chunk: i64,
    },
    /// Adaptive: use event count if available, fall back to duration
    Adaptive {
        target_events_per_chunk: usize,
        fallback_hours: i64,
    },
}

impl Default for ChunkingStrategy {
    fn default() -> Self {
        ChunkingStrategy::Adaptive {
            target_events_per_chunk: DEFAULT_CHUNK_SIZE,
            fallback_hours: DEFAULT_CHUNK_HOURS,
        }
    }
}

/// Information about a single chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestChunkInfo {
    /// Chunk index (0-based)
    pub chunk_id: i32,
    /// Total chunks in job
    pub total_chunks: i32,
    /// Start time of this chunk
    pub start_time: DateTime<Utc>,
    /// End time of this chunk
    pub end_time: DateTime<Utc>,
    /// Estimated event count (if known)
    pub estimated_events: Option<i64>,
    /// Priority (higher chunks process first)
    pub priority: i32,
}

/// Calculator for splitting backtests into chunks
pub struct ChunkCalculator {
    strategy: ChunkingStrategy,
}

impl ChunkCalculator {
    /// Create a new calculator with the given strategy
    pub fn new(strategy: ChunkingStrategy) -> Self {
        Self { strategy }
    }
    
    /// Create with default adaptive strategy
    pub fn adaptive() -> Self {
        Self::new(ChunkingStrategy::default())
    }
    
    /// Calculate chunks for a time range
    /// 
    /// # Arguments
    /// * `start` - Start of the backtest period
    /// * `end` - End of the backtest period
    /// * `total_events` - Optional total event count (for event-based chunking)
    /// 
    /// # Returns
    /// Vector of chunk info, ordered by priority (highest first)
    pub fn calculate_chunks(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        total_events: Option<i64>,
    ) -> Vec<BacktestChunkInfo> {
        let chunks = match &self.strategy {
            ChunkingStrategy::ByEventCount { target_events_per_chunk } => {
                self.chunk_by_events(start, end, total_events, *target_events_per_chunk)
            }
            ChunkingStrategy::ByDuration { hours_per_chunk } => {
                self.chunk_by_duration(start, end, *hours_per_chunk)
            }
            ChunkingStrategy::ByTradingDays { days_per_chunk } => {
                self.chunk_by_days(start, end, *days_per_chunk)
            }
            ChunkingStrategy::Adaptive { target_events_per_chunk, fallback_hours } => {
                if total_events.is_some() {
                    self.chunk_by_events(start, end, total_events, *target_events_per_chunk)
                } else {
                    self.chunk_by_duration(start, end, *fallback_hours)
                }
            }
        };
        
        // Limit maximum chunks
        if chunks.len() > MAX_CHUNKS_PER_JOB {
            // Combine chunks to stay under limit
            self.combine_chunks(chunks, MAX_CHUNKS_PER_JOB)
        } else {
            chunks
        }
    }
    
    /// Chunk by event count (proportional time slicing)
    fn chunk_by_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        total_events: Option<i64>,
        target_per_chunk: usize,
    ) -> Vec<BacktestChunkInfo> {
        let total = total_events.unwrap_or(1_000_000) as usize;
        let num_chunks = ((total + target_per_chunk - 1) / target_per_chunk).max(1);
        
        let total_duration = end.signed_duration_since(start);
        let chunk_duration = total_duration / num_chunks as i32;
        
        let mut chunks = Vec::with_capacity(num_chunks);
        let mut current_start = start;
        
        for i in 0..num_chunks {
            let chunk_end = if i == num_chunks - 1 {
                end // Last chunk goes to the end
            } else {
                current_start + chunk_duration
            };
            
            let estimated_events = total_events.map(|t| t / num_chunks as i64);
            
            chunks.push(BacktestChunkInfo {
                chunk_id: i as i32,
                total_chunks: num_chunks as i32,
                start_time: current_start,
                end_time: chunk_end,
                estimated_events,
                priority: (num_chunks - i) as i32, // Earlier chunks have higher priority
            });
            
            current_start = chunk_end;
        }
        
        chunks
    }
    
    /// Chunk by fixed duration
    fn chunk_by_duration(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        hours: i64,
    ) -> Vec<BacktestChunkInfo> {
        let chunk_duration = Duration::hours(hours);
        let total_duration = end.signed_duration_since(start);
        let num_chunks = ((total_duration.num_hours() + hours - 1) / hours).max(1) as usize;
        
        let mut chunks = Vec::with_capacity(num_chunks);
        let mut current_start = start;
        let mut i = 0;
        
        while current_start < end {
            let chunk_end = (current_start + chunk_duration).min(end);
            
            chunks.push(BacktestChunkInfo {
                chunk_id: i,
                total_chunks: num_chunks as i32,
                start_time: current_start,
                end_time: chunk_end,
                estimated_events: None,
                priority: (num_chunks - i as usize) as i32,
            });
            
            current_start = chunk_end;
            i += 1;
        }
        
        // Update total_chunks with actual count
        let actual_count = chunks.len() as i32;
        for chunk in &mut chunks {
            chunk.total_chunks = actual_count;
        }
        
        chunks
    }
    
    /// Chunk by trading days
    fn chunk_by_days(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        days: i64,
    ) -> Vec<BacktestChunkInfo> {
        // For now, treat as 24-hour days
        // Could be extended to skip weekends for equity markets
        self.chunk_by_duration(start, end, days * 24)
    }
    
    /// Combine chunks to stay under maximum limit
    fn combine_chunks(&self, chunks: Vec<BacktestChunkInfo>, max: usize) -> Vec<BacktestChunkInfo> {
        if chunks.len() <= max {
            return chunks;
        }
        
        let combine_factor = (chunks.len() + max - 1) / max;
        let mut combined = Vec::with_capacity(max);
        
        for (i, group) in chunks.chunks(combine_factor).enumerate() {
            let first = &group[0];
            let last = group.last().unwrap();
            
            let estimated_events = group.iter()
                .filter_map(|c| c.estimated_events)
                .sum::<i64>();
            
            combined.push(BacktestChunkInfo {
                chunk_id: i as i32,
                total_chunks: max as i32,
                start_time: first.start_time,
                end_time: last.end_time,
                estimated_events: if estimated_events > 0 { Some(estimated_events) } else { None },
                priority: (max - i) as i32,
            });
        }
        
        combined
    }
}

/// Query to get event count for time range (for chunk planning)
pub fn event_count_query(symbol: &str, exchange: &str, start: &DateTime<Utc>, end: &DateTime<Utc>) -> String {
    format!(
        "SELECT COUNT(*) FROM orderbook_events 
         WHERE symbol = '{}' AND exchange = '{}' 
         AND timestamp >= '{}' AND timestamp < '{}'",
        symbol, exchange, start.to_rfc3339(), end.to_rfc3339()
    )
}

/// Query to get event counts per day (for adaptive chunking)
pub fn daily_event_counts_query(symbol: &str, exchange: &str, start: &DateTime<Utc>, end: &DateTime<Utc>) -> String {
    format!(
        "SELECT DATE(timestamp) as day, COUNT(*) as event_count 
         FROM orderbook_events 
         WHERE symbol = '{}' AND exchange = '{}' 
         AND timestamp >= '{}' AND timestamp < '{}'
         GROUP BY DATE(timestamp)
         ORDER BY day",
        symbol, exchange, start.to_rfc3339(), end.to_rfc3339()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    
    #[test]
    fn test_chunk_by_duration() {
        let calculator = ChunkCalculator::new(ChunkingStrategy::ByDuration { hours_per_chunk: 24 });
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 8, 0, 0, 0).unwrap();
        
        let chunks = calculator.calculate_chunks(start, end, None);
        
        assert_eq!(chunks.len(), 7);
        assert_eq!(chunks[0].chunk_id, 0);
        assert_eq!(chunks[0].total_chunks, 7);
        assert_eq!(chunks[0].start_time, start);
    }
    
    #[test]
    fn test_chunk_by_events() {
        let calculator = ChunkCalculator::new(ChunkingStrategy::ByEventCount { 
            target_events_per_chunk: 200_000 
        });
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 8, 0, 0, 0).unwrap();
        
        // 1 million events -> 5 chunks
        let chunks = calculator.calculate_chunks(start, end, Some(1_000_000));
        
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[0].estimated_events, Some(200_000));
    }
    
    #[test]
    fn test_max_chunks_limit() {
        let calculator = ChunkCalculator::new(ChunkingStrategy::ByDuration { hours_per_chunk: 1 });
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 4, 1, 0, 0, 0).unwrap(); // 90 days = 2160 hours
        
        let chunks = calculator.calculate_chunks(start, end, None);
        
        // Should be limited to MAX_CHUNKS_PER_JOB
        assert!(chunks.len() <= MAX_CHUNKS_PER_JOB);
    }
}
