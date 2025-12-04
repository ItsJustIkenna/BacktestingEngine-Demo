use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Aggregation functions for market data processing
pub struct DataAggregator;

impl DataAggregator {
    /// Calculate OHLCV (Open, High, Low, Close, Volume) data from tick data
    pub fn calculate_ohlcv(
        ticks: &[(DateTime<Utc>, BigDecimal, BigDecimal)], // (timestamp, price, volume)
        interval_seconds: i64,
    ) -> Result<Vec<OHLCVData>> {
        if ticks.is_empty() {
            return Ok(vec![]);
        }

        let mut ohlcv_data = Vec::new();
        let mut current_interval_start = ticks[0].0;
        let mut open = ticks[0].1.clone();
        let mut high = ticks[0].1.clone();
        let mut low = ticks[0].1.clone();
        let mut close = ticks[0].1.clone();
        let mut volume = BigDecimal::from(0);

        for (timestamp, price, tick_volume) in ticks {
            // Check if we need to start a new interval
            let interval_duration = chrono::Duration::seconds(interval_seconds);
            if *timestamp >= current_interval_start + interval_duration {
                // Save current interval data
                ohlcv_data.push(OHLCVData {
                    timestamp: current_interval_start,
                    open: open.clone(),
                    high: high.clone(),
                    low: low.clone(),
                    close: close.clone(),
                    volume: volume.clone(),
                });

                // Start new interval
                current_interval_start = *timestamp;
                open = price.clone();
                high = price.clone();
                low = price.clone();
                close = price.clone();
                volume = tick_volume.clone();
            } else {
                // Update current interval data
                if price > &high {
                    high = price.clone();
                }
                if price < &low {
                    low = price.clone();
                }
                close = price.clone();
                volume += tick_volume;
            }
        }

        // Add the last interval
        if !ticks.is_empty() {
            ohlcv_data.push(OHLCVData {
                timestamp: current_interval_start,
                open,
                high,
                low,
                close,
                volume,
            });
        }

        Ok(ohlcv_data)
    }

    /// Calculate moving average
    pub fn calculate_moving_average(
        prices: &[BigDecimal],
        window_size: usize,
    ) -> Result<Vec<BigDecimal>> {
        if prices.len() < window_size || window_size == 0 {
            return Ok(vec![]);
        }

        let mut moving_averages = Vec::new();
        
        for i in window_size.saturating_sub(1)..prices.len() {
            let start_idx = i.saturating_sub(window_size.saturating_sub(1));
            let window_sum: BigDecimal = prices[start_idx..=i]
                .iter()
                .sum();
            let window_size_bigint = (window_size as i64).into();
            let window_size_decimal = BigDecimal::new(window_size_bigint, 0);
            let avg = &window_sum / &window_size_decimal;
            moving_averages.push(avg);
        }

        Ok(moving_averages)
    }

    /// Calculate volume-weighted average price (VWAP)
    pub fn calculate_vwap(
        price_volume_data: &[(BigDecimal, BigDecimal)], // (price, volume)
    ) -> Result<BigDecimal> {
        if price_volume_data.is_empty() {
            return Ok(BigDecimal::from(0));
        }

        let mut total_pv = BigDecimal::from(0);
        let mut total_volume = BigDecimal::from(0);

        for (price, volume) in price_volume_data {
            total_pv += price * volume;
            total_volume += volume;
        }

        if total_volume == BigDecimal::from(0) {
            Ok(BigDecimal::from(0))
        } else {
            Ok(total_pv / total_volume)
        }
    }

    /// Aggregate trade data by symbol
    pub fn aggregate_by_symbol(
        trades: &[(String, BigDecimal, BigDecimal, DateTime<Utc>)], // (symbol, price, volume, timestamp)
    ) -> HashMap<String, Vec<(BigDecimal, BigDecimal, DateTime<Utc>)>> {
        let mut aggregated: HashMap<String, Vec<(BigDecimal, BigDecimal, DateTime<Utc>)>> = HashMap::new();

        for (symbol, price, volume, timestamp) in trades {
            aggregated
                .entry(symbol.clone())
                .or_insert_with(Vec::new)
                .push((price.clone(), volume.clone(), *timestamp));
        }

        aggregated
    }
}

/// OHLCV data structure
#[derive(Debug, Clone)]
pub struct OHLCVData {
    pub timestamp: DateTime<Utc>,
    pub open: BigDecimal,
    pub high: BigDecimal,
    pub low: BigDecimal,
    pub close: BigDecimal,
    pub volume: BigDecimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_calculate_moving_average() {
        let prices = vec![
            BigDecimal::from(100),
            BigDecimal::from(102),
            BigDecimal::from(104),
            BigDecimal::from(103),
            BigDecimal::from(105),
        ];

        let ma = DataAggregator::calculate_moving_average(&prices, 3).unwrap();
        assert_eq!(ma.len(), 3);
        
        // First MA should be (100 + 102 + 104) / 3 = 102
        assert_eq!(ma[0], BigDecimal::from(102));
    }

    #[test]
    fn test_calculate_vwap() {
        let price_volume_data = vec![
            (BigDecimal::from(100), BigDecimal::from(10)),  // 100 * 10 = 1000
            (BigDecimal::from(102), BigDecimal::from(20)),  // 102 * 20 = 2040
            (BigDecimal::from(104), BigDecimal::from(15)),  // 104 * 15 = 1560
        ];
        // Total PV = 4600, Total Volume = 45, VWAP = 4600/45 ≈ 102.22

        let vwap = DataAggregator::calculate_vwap(&price_volume_data).unwrap();
        // Using integer division, should be close to 102.22
        assert!(vwap > BigDecimal::from(102) && vwap < BigDecimal::from(103));
    }

    #[test]
    fn test_calculate_ohlcv() {
        let ticks = vec![
            (Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 0).unwrap(), BigDecimal::from(100), BigDecimal::from(10)),
            (Utc.with_ymd_and_hms(2023, 1, 1, 10, 0, 30).unwrap(), BigDecimal::from(102), BigDecimal::from(15)),
            (Utc.with_ymd_and_hms(2023, 1, 1, 10, 1, 0).unwrap(), BigDecimal::from(101), BigDecimal::from(12)),
            (Utc.with_ymd_and_hms(2023, 1, 1, 10, 1, 30).unwrap(), BigDecimal::from(103), BigDecimal::from(8)),
        ];

        let ohlcv = DataAggregator::calculate_ohlcv(&ticks, 60).unwrap(); // 60-second intervals
        assert_eq!(ohlcv.len(), 2);
        
        // First interval (10:00:00 - 10:01:00)
        assert_eq!(ohlcv[0].open, BigDecimal::from(100));
        assert_eq!(ohlcv[0].high, BigDecimal::from(102));
        assert_eq!(ohlcv[0].low, BigDecimal::from(100));
        assert_eq!(ohlcv[0].close, BigDecimal::from(102));
        assert_eq!(ohlcv[0].volume, BigDecimal::from(25)); // 10 + 15
    }
}
