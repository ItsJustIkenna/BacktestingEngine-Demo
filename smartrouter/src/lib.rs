//! Smart Order Router for Multi-Venue Order Execution
//!
//! This crate provides smart order routing capabilities for backtesting:
//! - **Multi-Venue Execution**: Split large orders across multiple exchanges
//! - **Best Venue Selection**: Route to optimal venue based on price, fees, liquidity
//! - **Cross-Venue Arbitrage**: Detect and execute arbitrage opportunities
//! - **Execution Algorithms**: TWAP, VWAP, Implementation Shortfall
//!
//! # Example
//! ```ignore
//! use smartrouter::{SmartRouter, RoutingMode, ExecutionStyle, ParentOrder};
//!
//! let router = SmartRouter::new(config);
//! router.add_venue("kraken", kraken_orderbook, kraken_fees);
//! router.add_venue("coinbase", coinbase_orderbook, coinbase_fees);
//!
//! let decision = router.route_order(parent_order, ExecutionStyle::Aggressive)?;
//! for child in decision.child_orders {
//!     println!("Execute {} on {}", child.quantity, child.venue);
//! }
//! ```

pub mod order;
pub mod venue;
pub mod algorithms;
pub mod metrics;
pub mod arbitrage;

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use thiserror::Error;

// Re-exports
pub use order::{ParentOrder, ChildOrder, OrderSide, OrderType, Fill};
pub use venue::{VenueSnapshot, VenueConfig};
pub use algorithms::{RoutingAlgorithm, route_best_venue, route_multi_venue, route_twap, route_vwap};
pub use metrics::{RoutingMetrics, ExecutionQuality};
pub use arbitrage::{ArbitrageOpportunity, ArbitrageRouter};

/// Errors that can occur during routing
#[derive(Error, Debug)]
pub enum RoutingError {
    #[error("No venues available for symbol {0}")]
    NoVenuesAvailable(String),
    
    #[error("Insufficient liquidity: need {needed}, available {available}")]
    InsufficientLiquidity { needed: f64, available: f64 },
    
    #[error("Venue {0} not found")]
    VenueNotFound(String),
    
    #[error("Invalid order: {0}")]
    InvalidOrder(String),
    
    #[error("Routing algorithm failed: {0}")]
    AlgorithmError(String),
}

/// Execution style / urgency for order routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStyle {
    /// Execute immediately at any cost - for arbitrage, urgent hedging
    Aggressive,
    /// Use limit orders, wait for fills - for rebalancing, low urgency
    Passive,
    /// Balance between speed and cost
    Balanced,
    /// Time-weighted execution over duration (milliseconds)
    TWAP { duration_ms: u64 },
    /// Volume-weighted execution
    VWAP { duration_ms: u64 },
}

/// Routing mode determining how orders are distributed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingMode {
    /// Route entire order to single best venue
    BestVenue,
    /// Split order across multiple venues for best execution
    MultiVenue {
        /// Maximum number of venues to use
        max_venues: usize,
        /// Minimum order fraction per venue (e.g., 0.1 = 10%)
        min_venue_allocation: f64,
    },
    /// Arbitrage mode: buy on one venue, sell on another
    Arbitrage {
        /// Minimum profit in basis points to execute
        min_profit_bps: f64,
        /// Maximum position holding time in milliseconds
        max_holding_time_ms: u64,
    },
}

impl Default for RoutingMode {
    fn default() -> Self {
        RoutingMode::BestVenue
    }
}

/// Result of a routing decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Original parent order ID
    pub parent_order_id: String,
    /// Child orders to execute on each venue
    pub child_orders: Vec<ChildOrder>,
    /// Expected total cost including fees
    pub expected_total_cost: f64,
    /// Expected average fill price
    pub expected_avg_price: f64,
    /// Expected slippage from mid price
    pub expected_slippage_bps: f64,
    /// Timestamp of decision
    pub decision_timestamp_ms: u64,
    /// Algorithm used
    pub algorithm: RoutingAlgorithm,
    /// Routing metrics
    pub metrics: RoutingMetrics,
}

/// Result of simulated order execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Parent order ID
    pub parent_order_id: String,
    /// Fills from all venues
    pub fills: Vec<Fill>,
    /// Total quantity filled
    pub total_filled: f64,
    /// Average fill price across all venues
    pub avg_fill_price: f64,
    /// Total fees paid
    pub total_fees: f64,
    /// Execution slippage vs benchmark
    pub slippage_bps: f64,
    /// Time to complete execution (simulated)
    pub execution_time_ms: u64,
    /// Per-venue breakdown
    pub venue_breakdown: HashMap<String, VenueFillSummary>,
}

/// Summary of fills on a single venue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueFillSummary {
    pub venue: String,
    pub quantity_filled: f64,
    pub avg_price: f64,
    pub fees: f64,
    pub fill_count: usize,
}

/// Smart Order Router - main entry point
pub struct SmartRouter {
    /// Configured venues with their current state
    venues: HashMap<String, VenueSnapshot>,
    /// Routing configuration
    routing_mode: RoutingMode,
    /// Default execution style
    default_style: ExecutionStyle,
    /// Cross-venue latency simulation (venue_a -> venue_b -> latency_ms)
    cross_venue_latency: HashMap<(String, String), f64>,
    /// Cumulative routing metrics
    cumulative_metrics: RoutingMetrics,
}

impl SmartRouter {
    /// Create a new SmartRouter with the specified routing mode
    pub fn new(routing_mode: RoutingMode) -> Self {
        Self {
            venues: HashMap::new(),
            routing_mode,
            default_style: ExecutionStyle::Balanced,
            cross_venue_latency: HashMap::new(),
            cumulative_metrics: RoutingMetrics::default(),
        }
    }
    
    /// Set the default execution style
    pub fn with_execution_style(mut self, style: ExecutionStyle) -> Self {
        self.default_style = style;
        self
    }
    
    /// Add or update a venue with its current orderbook state
    pub fn update_venue(&mut self, venue_name: &str, snapshot: VenueSnapshot) {
        self.venues.insert(venue_name.to_string(), snapshot);
    }
    
    /// Set cross-venue latency for simulation
    pub fn set_cross_venue_latency(&mut self, from: &str, to: &str, latency_ms: f64) {
        self.cross_venue_latency.insert((from.to_string(), to.to_string()), latency_ms);
    }
    
    /// Get all configured venues
    pub fn venues(&self) -> &HashMap<String, VenueSnapshot> {
        &self.venues
    }
    
    /// Get cumulative routing metrics
    pub fn metrics(&self) -> &RoutingMetrics {
        &self.cumulative_metrics
    }
    
    /// Route an order according to the configured mode
    pub fn route_order(
        &mut self,
        order: &ParentOrder,
        style: Option<ExecutionStyle>,
    ) -> Result<RoutingDecision, RoutingError> {
        let style = style.unwrap_or(self.default_style);
        
        // Get venues that support this symbol
        let available_venues: Vec<_> = self.venues.iter()
            .filter(|(_, v)| v.symbol == order.symbol)
            .collect();
        
        if available_venues.is_empty() {
            return Err(RoutingError::NoVenuesAvailable(order.symbol.clone()));
        }
        
        let decision = match &self.routing_mode {
            RoutingMode::BestVenue => {
                route_best_venue(order, &available_venues, style)?
            }
            RoutingMode::MultiVenue { max_venues, min_venue_allocation } => {
                route_multi_venue(order, &available_venues, style, *max_venues, *min_venue_allocation)?
            }
            RoutingMode::Arbitrage { .. } => {
                // Arbitrage uses different flow - see ArbitrageRouter
                return Err(RoutingError::AlgorithmError(
                    "Use ArbitrageRouter for arbitrage mode".to_string()
                ));
            }
        };
        
        // Update cumulative metrics
        self.cumulative_metrics.add(&decision.metrics);
        
        Ok(decision)
    }
    
    /// Simulate execution of a routing decision
    pub fn simulate_execution(
        &self,
        decision: &RoutingDecision,
        current_time_ms: u64,
    ) -> ExecutionResult {
        let mut fills = Vec::new();
        let mut venue_breakdown = HashMap::new();
        let mut total_filled = 0.0;
        let mut total_cost = 0.0;
        let mut total_fees = 0.0;
        
        for child in &decision.child_orders {
            if let Some(venue) = self.venues.get(&child.venue) {
                // Simulate the fill based on venue orderbook
                let (fill_price, fill_qty) = venue.simulate_fill(
                    child.side,
                    child.quantity,
                    child.limit_price,
                );
                
                let fee = fill_qty * fill_price * venue.config.taker_fee_rate;
                
                let fill = Fill {
                    order_id: child.id.clone(),
                    venue: child.venue.clone(),
                    side: child.side,
                    quantity: fill_qty,
                    price: fill_price,
                    fee,
                    timestamp_ms: current_time_ms + venue.config.latency_ms as u64,
                };
                
                total_filled += fill_qty;
                total_cost += fill_qty * fill_price;
                total_fees += fee;
                
                // Update venue breakdown
                venue_breakdown.entry(child.venue.clone())
                    .and_modify(|summary: &mut VenueFillSummary| {
                        let prev_qty = summary.quantity_filled;
                        let prev_avg = summary.avg_price;
                        summary.quantity_filled += fill_qty;
                        summary.avg_price = (prev_qty * prev_avg + fill_qty * fill_price) / summary.quantity_filled;
                        summary.fees += fee;
                        summary.fill_count += 1;
                    })
                    .or_insert(VenueFillSummary {
                        venue: child.venue.clone(),
                        quantity_filled: fill_qty,
                        avg_price: fill_price,
                        fees: fee,
                        fill_count: 1,
                    });
                
                fills.push(fill);
            }
        }
        
        let avg_fill_price = if total_filled > 0.0 { total_cost / total_filled } else { 0.0 };
        let slippage_bps = if decision.expected_avg_price > 0.0 {
            ((avg_fill_price - decision.expected_avg_price) / decision.expected_avg_price) * 10000.0
        } else {
            0.0
        };
        
        ExecutionResult {
            parent_order_id: decision.parent_order_id.clone(),
            fills,
            total_filled,
            avg_fill_price,
            total_fees,
            slippage_bps,
            execution_time_ms: 0, // Would be calculated based on algorithm
            venue_breakdown,
        }
    }
    
    /// Reset all venues (call between backtest iterations)
    pub fn clear_venues(&mut self) {
        self.venues.clear();
    }
    
    /// Reset metrics (call between backtest runs)
    pub fn reset_metrics(&mut self) {
        self.cumulative_metrics = RoutingMetrics::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_router_creation() {
        let router = SmartRouter::new(RoutingMode::BestVenue);
        assert!(router.venues().is_empty());
    }
    
    #[test]
    fn test_routing_mode_default() {
        let mode = RoutingMode::default();
        assert!(matches!(mode, RoutingMode::BestVenue));
    }
}
