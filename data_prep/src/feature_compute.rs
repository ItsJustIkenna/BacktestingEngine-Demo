//! Feature computation for simulation ticks
//!
//! Computes and stores derived features directly in the binary file:
//! - Rolling volatility (1-minute window)
//! - Exponential moving average (5-minute)
//! - Order flow imbalance

use std::path::Path;
use std::fs::OpenOptions;
use anyhow::Result;
use memmap2::MmapMut;

use crate::binary_format::{BinaryFileHeader, HEADER_SIZE};
use crate::tick::SimulationTick;

/// Configuration for feature computation
#[derive(Debug, Clone)]
pub struct FeatureConfig {
    /// Window size for volatility calculation (in ticks)
    pub volatility_window: usize,
    /// EMA decay factor (alpha)
    pub ema_alpha: f64,
    /// Window size for imbalance calculation (in ticks)
    pub imbalance_window: usize,
}

impl Default for FeatureConfig {
    fn default() -> Self {
        Self {
            // Assuming ~1 tick per second, 60 ticks = 1 minute
            volatility_window: 60,
            // EMA alpha for 5-minute decay: 2/(300+1) ≈ 0.0066
            ema_alpha: 0.0066,
            // 30 ticks for imbalance
            imbalance_window: 30,
        }
    }
}

/// Computes and updates features in simulation tick files
pub struct FeatureComputer {
    config: FeatureConfig,
}

impl Default for FeatureComputer {
    fn default() -> Self {
        Self {
            config: FeatureConfig::default(),
        }
    }
}

impl FeatureComputer {
    /// Create with custom config
    pub fn new(config: FeatureConfig) -> Self {
        Self { config }
    }
    
    /// Compute features and update the binary file in-place
    pub fn compute_and_update(&self, path: &Path) -> Result<()> {
        // Open file for read+write
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)?;
        
        // Memory-map for efficient access
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        
        // Read header
        let header_bytes: [u8; HEADER_SIZE] = mmap[..HEADER_SIZE].try_into()
            .map_err(|_| anyhow::anyhow!("Failed to read header"))?;
        let mut header = BinaryFileHeader::from_bytes(&header_bytes);
        header.validate()?;
        
        let tick_count = header.tick_count as usize;
        if tick_count == 0 {
            return Ok(());
        }
        
        // Get mutable tick slice
        let tick_bytes = &mut mmap[HEADER_SIZE..];
        let ticks: &mut [SimulationTick] = bytemuck::cast_slice_mut(tick_bytes);
        
        // Compute features
        self.compute_volatility(ticks);
        self.compute_ema(ticks);
        self.compute_imbalance(ticks);
        
        // Update header to mark features as computed
        header.features_computed = 1;
        mmap[..HEADER_SIZE].copy_from_slice(&header.to_bytes());
        
        // Flush changes to disk
        mmap.flush()?;
        
        Ok(())
    }
    
    /// Compute rolling volatility (standard deviation of returns)
    fn compute_volatility(&self, ticks: &mut [SimulationTick]) {
        let window = self.config.volatility_window;
        
        if ticks.len() < 2 {
            return;
        }
        
        // Pre-compute log returns
        let mut returns: Vec<f64> = Vec::with_capacity(ticks.len());
        returns.push(0.0); // First tick has no return
        
        for i in 1..ticks.len() {
            let ret = if ticks[i-1].price > 0.0 {
                (ticks[i].price / ticks[i-1].price).ln()
            } else {
                0.0
            };
            returns.push(ret);
        }
        
        // Rolling volatility using Welford's online algorithm for efficiency
        for i in 0..ticks.len() {
            let start = if i >= window { i - window + 1 } else { 0 };
            let slice = &returns[start..=i];
            
            if slice.len() < 2 {
                ticks[i].volatility_1m = 0.0;
                continue;
            }
            
            // Calculate standard deviation
            let n = slice.len() as f64;
            let mean = slice.iter().sum::<f64>() / n;
            let variance = slice.iter()
                .map(|r| (r - mean).powi(2))
                .sum::<f64>() / (n - 1.0);
            
            ticks[i].volatility_1m = variance.sqrt() as f32;
        }
    }
    
    /// Compute exponential moving average of price
    fn compute_ema(&self, ticks: &mut [SimulationTick]) {
        if ticks.is_empty() {
            return;
        }
        
        let alpha = self.config.ema_alpha;
        let mut ema = ticks[0].price;
        
        for tick in ticks.iter_mut() {
            ema = alpha * tick.price + (1.0 - alpha) * ema;
            tick.ema_5m = ema as f32;
        }
    }
    
    /// Compute order flow imbalance
    /// Imbalance = (buy_volume - sell_volume) / (buy_volume + sell_volume)
    /// Range: [-1, 1] where 1 = all buys, -1 = all sells
    fn compute_imbalance(&self, ticks: &mut [SimulationTick]) {
        let window = self.config.imbalance_window;
        
        for i in 0..ticks.len() {
            let start = if i >= window { i - window + 1 } else { 0 };
            let slice = &ticks[start..=i];
            
            let mut buy_volume = 0.0;
            let mut sell_volume = 0.0;
            
            for t in slice {
                if t.is_buy() {
                    buy_volume += t.quantity;
                } else {
                    sell_volume += t.quantity;
                }
            }
            
            let total = buy_volume + sell_volume;
            ticks[i].imbalance = if total > 0.0 {
                ((buy_volume - sell_volume) / total) as f32
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary_format::BinaryFileWriter;
    use tempfile::tempdir;

    #[test]
    fn test_feature_computation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");
        
        // Create test file
        let mut writer = BinaryFileWriter::create(&path).unwrap();
        writer.set_metadata("TEST", "TEST");
        
        let ticks: Vec<SimulationTick> = (0..100)
            .map(|i| {
                let price = 100.0 + (i as f64 * 0.1);
                let side = i % 3 != 0; // 2/3 buys, 1/3 sells
                SimulationTick::new(i * 1000, price, 1.0, !side)
            })
            .collect();
        
        writer.write_ticks(&ticks).unwrap();
        writer.finalize().unwrap();
        
        // Compute features
        let computer = FeatureComputer::default();
        computer.compute_and_update(&path).unwrap();
        
        // Verify features were written
        let reader = crate::binary_format::BinaryFileReader::open(&path).unwrap();
        let read_ticks = reader.ticks();
        
        // Check that features are non-zero (except possibly first few)
        let last_tick = &read_ticks[99];
        assert!(last_tick.ema_5m > 0.0, "EMA should be computed");
        // Volatility might be very small with linear price increase
        // Imbalance should be positive (more buys)
        assert!(last_tick.imbalance > 0.0, "Imbalance should be positive (more buys)");
    }
}
