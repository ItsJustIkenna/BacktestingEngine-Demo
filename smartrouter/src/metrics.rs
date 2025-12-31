//! Routing metrics for tracking execution quality
//!
//! Tracks slippage, market impact, venue performance, and implementation shortfall

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Execution quality score
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionQuality {
    Excellent,  // < 1 bps slippage
    Good,       // 1-5 bps
    Average,    // 5-10 bps
    Poor,       // 10-25 bps
    VeryPoor,   // > 25 bps
}

impl ExecutionQuality {
    pub fn from_slippage_bps(slippage: f64) -> Self {
        let abs_slippage = slippage.abs();
        if abs_slippage < 1.0 {
            ExecutionQuality::Excellent
        } else if abs_slippage < 5.0 {
            ExecutionQuality::Good
        } else if abs_slippage < 10.0 {
            ExecutionQuality::Average
        } else if abs_slippage < 25.0 {
            ExecutionQuality::Poor
        } else {
            ExecutionQuality::VeryPoor
        }
    }
}

/// Cumulative routing metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingMetrics {
    /// Total orders routed
    pub total_orders: u64,
    /// Total volume routed (notional)
    pub total_volume: f64,
    /// Total fees paid
    pub total_fees: f64,
    /// Average slippage in basis points
    pub avg_slippage_bps: f64,
    /// Maximum slippage observed
    pub max_slippage_bps: f64,
    /// Minimum slippage observed
    pub min_slippage_bps: f64,
    /// Implementation shortfall (difference from benchmark)
    pub implementation_shortfall_bps: f64,
    /// Per-venue statistics
    pub venue_stats: HashMap<String, VenueStats>,
    /// Count by execution quality
    pub quality_breakdown: QualityBreakdown,
    /// Orders split across multiple venues
    pub multi_venue_orders: u64,
    /// Orders routed to single venue
    pub single_venue_orders: u64,
    /// Average venues per order
    pub avg_venues_per_order: f64,
    /// Savings from smart routing (vs worst venue)
    pub routing_savings: f64,
}

impl RoutingMetrics {
    /// Create new empty metrics
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add metrics from a single routing decision
    pub fn add(&mut self, other: &RoutingMetrics) {
        self.total_orders += other.total_orders;
        self.total_volume += other.total_volume;
        self.total_fees += other.total_fees;
        
        // Weighted average for slippage
        if self.total_orders > 0 {
            let prev_weight = (self.total_orders - other.total_orders) as f64;
            let new_weight = other.total_orders as f64;
            self.avg_slippage_bps = 
                (self.avg_slippage_bps * prev_weight + other.avg_slippage_bps * new_weight) 
                / self.total_orders as f64;
        }
        
        self.max_slippage_bps = self.max_slippage_bps.max(other.max_slippage_bps);
        if self.min_slippage_bps == 0.0 {
            self.min_slippage_bps = other.min_slippage_bps;
        } else if other.min_slippage_bps != 0.0 {
            self.min_slippage_bps = self.min_slippage_bps.min(other.min_slippage_bps);
        }
        
        // Merge venue stats
        for (venue, stats) in &other.venue_stats {
            self.venue_stats
                .entry(venue.clone())
                .and_modify(|s| s.merge(stats))
                .or_insert_with(|| stats.clone());
        }
        
        // Merge quality breakdown
        self.quality_breakdown.merge(&other.quality_breakdown);
        
        self.multi_venue_orders += other.multi_venue_orders;
        self.single_venue_orders += other.single_venue_orders;
        self.routing_savings += other.routing_savings;
        
        // Recalculate average venues per order
        if self.total_orders > 0 {
            let total_venues = self.venue_stats.values()
                .map(|s| s.orders_routed)
                .sum::<u64>();
            self.avg_venues_per_order = total_venues as f64 / self.total_orders as f64;
        }
    }
    
    /// Record a single order routing
    pub fn record_order(
        &mut self,
        volume: f64,
        fees: f64,
        slippage_bps: f64,
        venues_used: &[String],
        benchmark_price: f64,
        actual_price: f64,
    ) {
        self.total_orders += 1;
        self.total_volume += volume;
        self.total_fees += fees;
        
        // Update slippage stats
        let prev_avg = self.avg_slippage_bps;
        self.avg_slippage_bps = prev_avg + (slippage_bps - prev_avg) / self.total_orders as f64;
        self.max_slippage_bps = self.max_slippage_bps.max(slippage_bps);
        if self.min_slippage_bps == 0.0 || slippage_bps < self.min_slippage_bps {
            self.min_slippage_bps = slippage_bps;
        }
        
        // Implementation shortfall
        let shortfall = ((actual_price - benchmark_price) / benchmark_price) * 10000.0;
        self.implementation_shortfall_bps = 
            self.implementation_shortfall_bps + (shortfall - self.implementation_shortfall_bps) / self.total_orders as f64;
        
        // Venue tracking
        if venues_used.len() > 1 {
            self.multi_venue_orders += 1;
        } else {
            self.single_venue_orders += 1;
        }
        
        for venue in venues_used {
            self.venue_stats
                .entry(venue.clone())
                .and_modify(|s| s.orders_routed += 1)
                .or_insert(VenueStats { 
                    venue: venue.clone(),
                    orders_routed: 1,
                    volume_routed: 0.0,
                    avg_fill_price: 0.0,
                    total_fees: 0.0,
                    avg_slippage_bps: 0.0,
                    fill_rate: 1.0,
                });
        }
        
        // Quality tracking
        self.quality_breakdown.record(slippage_bps);
        
        // Update average venues
        self.avg_venues_per_order = 
            (self.avg_venues_per_order * (self.total_orders - 1) as f64 + venues_used.len() as f64) 
            / self.total_orders as f64;
    }
    
    /// Get overall execution quality
    pub fn overall_quality(&self) -> ExecutionQuality {
        ExecutionQuality::from_slippage_bps(self.avg_slippage_bps)
    }
    
    /// Get fee rate as percentage
    pub fn avg_fee_rate(&self) -> f64 {
        if self.total_volume > 0.0 {
            (self.total_fees / self.total_volume) * 100.0
        } else {
            0.0
        }
    }
}

/// Per-venue routing statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenueStats {
    pub venue: String,
    pub orders_routed: u64,
    pub volume_routed: f64,
    pub avg_fill_price: f64,
    pub total_fees: f64,
    pub avg_slippage_bps: f64,
    pub fill_rate: f64,  // Percentage of orders fully filled
}

impl VenueStats {
    pub fn merge(&mut self, other: &VenueStats) {
        let total_orders = self.orders_routed + other.orders_routed;
        if total_orders > 0 {
            self.avg_fill_price = 
                (self.avg_fill_price * self.orders_routed as f64 
                 + other.avg_fill_price * other.orders_routed as f64) 
                / total_orders as f64;
            self.avg_slippage_bps = 
                (self.avg_slippage_bps * self.orders_routed as f64 
                 + other.avg_slippage_bps * other.orders_routed as f64) 
                / total_orders as f64;
            self.fill_rate = 
                (self.fill_rate * self.orders_routed as f64 
                 + other.fill_rate * other.orders_routed as f64) 
                / total_orders as f64;
        }
        self.orders_routed = total_orders;
        self.volume_routed += other.volume_routed;
        self.total_fees += other.total_fees;
    }
}

/// Breakdown of execution quality
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualityBreakdown {
    pub excellent: u64,
    pub good: u64,
    pub average: u64,
    pub poor: u64,
    pub very_poor: u64,
}

impl QualityBreakdown {
    pub fn record(&mut self, slippage_bps: f64) {
        match ExecutionQuality::from_slippage_bps(slippage_bps) {
            ExecutionQuality::Excellent => self.excellent += 1,
            ExecutionQuality::Good => self.good += 1,
            ExecutionQuality::Average => self.average += 1,
            ExecutionQuality::Poor => self.poor += 1,
            ExecutionQuality::VeryPoor => self.very_poor += 1,
        }
    }
    
    pub fn merge(&mut self, other: &QualityBreakdown) {
        self.excellent += other.excellent;
        self.good += other.good;
        self.average += other.average;
        self.poor += other.poor;
        self.very_poor += other.very_poor;
    }
    
    pub fn total(&self) -> u64 {
        self.excellent + self.good + self.average + self.poor + self.very_poor
    }
    
    /// Percentage in good or excellent category
    pub fn good_execution_rate(&self) -> f64 {
        let total = self.total();
        if total > 0 {
            ((self.excellent + self.good) as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_execution_quality_from_slippage() {
        assert_eq!(ExecutionQuality::from_slippage_bps(0.5), ExecutionQuality::Excellent);
        assert_eq!(ExecutionQuality::from_slippage_bps(3.0), ExecutionQuality::Good);
        assert_eq!(ExecutionQuality::from_slippage_bps(7.0), ExecutionQuality::Average);
        assert_eq!(ExecutionQuality::from_slippage_bps(15.0), ExecutionQuality::Poor);
        assert_eq!(ExecutionQuality::from_slippage_bps(50.0), ExecutionQuality::VeryPoor);
    }
    
    #[test]
    fn test_metrics_recording() {
        let mut metrics = RoutingMetrics::new();
        metrics.record_order(10000.0, 10.0, 2.5, &["kraken".to_string()], 42000.0, 42010.0);
        
        assert_eq!(metrics.total_orders, 1);
        assert_eq!(metrics.total_volume, 10000.0);
        assert_eq!(metrics.single_venue_orders, 1);
        assert_eq!(metrics.quality_breakdown.good, 1);
    }
    
    #[test]
    fn test_quality_breakdown() {
        let mut breakdown = QualityBreakdown::default();
        breakdown.record(0.5);  // excellent
        breakdown.record(3.0);  // good
        breakdown.record(7.0);  // average
        
        assert_eq!(breakdown.total(), 3);
        assert!((breakdown.good_execution_rate() - 66.67).abs() < 1.0);
    }
}
