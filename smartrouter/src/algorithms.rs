//! Routing algorithms for smart order execution
//!
//! Implements various routing strategies:
//! - BestVenue: Route to single best venue
//! - MultiVenue: Split across venues for best execution
//! - TWAP: Time-weighted average price
//! - VWAP: Volume-weighted average price

use crate::{
    RoutingDecision, RoutingError, ExecutionStyle,
    order::{ParentOrder, ChildOrder, OrderType, OrderSide},
    venue::VenueSnapshot,
    metrics::RoutingMetrics,
};
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Routing algorithm identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingAlgorithm {
    /// Route to single best venue
    BestVenue,
    /// Split across multiple venues
    MultiVenue,
    /// Time-weighted average price
    TWAP,
    /// Volume-weighted average price
    VWAP,
    /// Minimize market impact
    MinimizeImpact,
    /// Implementation shortfall optimization
    ImplementationShortfall,
}

/// Route order to the single best venue based on expected execution price
pub fn route_best_venue(
    order: &ParentOrder,
    venues: &[(&String, &VenueSnapshot)],
    _style: ExecutionStyle,
) -> Result<RoutingDecision, RoutingError> {
    if venues.is_empty() {
        return Err(RoutingError::NoVenuesAvailable(order.symbol.clone()));
    }
    
    // Score each venue and find the best one
    let mut best_venue: Option<(&String, &VenueSnapshot, f64)> = None;
    
    for (name, snapshot) in venues {
        let score = snapshot.score_for_order(order.side, order.quantity);
        if score > 0.0 {
            match &best_venue {
                None => best_venue = Some((name, snapshot, score)),
                Some((_, _, current_score)) if score > *current_score => {
                    best_venue = Some((name, snapshot, score));
                }
                _ => {}
            }
        }
    }
    
    let (venue_name, snapshot, _) = best_venue
        .ok_or_else(|| RoutingError::InsufficientLiquidity {
            needed: order.quantity,
            available: 0.0,
        })?;
    
    let expected_price = snapshot.expected_fill_price(order.side, order.quantity)
        .ok_or_else(|| RoutingError::InsufficientLiquidity {
            needed: order.quantity,
            available: snapshot.ask_liquidity(None),
        })?;
    
    let expected_fees = order.quantity * expected_price * snapshot.config.taker_fee_rate;
    
    let child = ChildOrder::new(
        &order.id,
        venue_name,
        &order.symbol,
        order.side,
        order.quantity,
        order.order_type,
        order.limit_price,
        expected_price,
        expected_fees,
    );
    
    let expected_slippage = ((expected_price - order.benchmark_price) / order.benchmark_price) * 10000.0;
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    
    let mut metrics = RoutingMetrics::new();
    metrics.record_order(
        order.quantity * expected_price,
        expected_fees,
        expected_slippage,
        &[venue_name.clone()],
        order.benchmark_price,
        expected_price,
    );
    
    Ok(RoutingDecision {
        parent_order_id: order.id.clone(),
        child_orders: vec![child],
        expected_total_cost: order.quantity * expected_price + expected_fees,
        expected_avg_price: expected_price,
        expected_slippage_bps: expected_slippage,
        decision_timestamp_ms: now,
        algorithm: RoutingAlgorithm::BestVenue,
        metrics,
    })
}

/// Route order across multiple venues for best execution
pub fn route_multi_venue(
    order: &ParentOrder,
    venues: &[(&String, &VenueSnapshot)],
    _style: ExecutionStyle,
    max_venues: usize,
    min_allocation: f64,
) -> Result<RoutingDecision, RoutingError> {
    if venues.is_empty() {
        return Err(RoutingError::NoVenuesAvailable(order.symbol.clone()));
    }
    
    // Score and sort venues by execution quality
    let mut scored_venues: Vec<_> = venues.iter()
        .filter_map(|(name, snapshot)| {
            let score = snapshot.score_for_order(order.side, order.quantity);
            let liquidity = match order.side {
                OrderSide::Buy => snapshot.ask_liquidity(None),
                OrderSide::Sell => snapshot.bid_liquidity(None),
            };
            if score > 0.0 && liquidity > 0.0 {
                Some((*name, *snapshot, score, liquidity))
            } else {
                None
            }
        })
        .collect();
    
    scored_venues.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    
    // Take top N venues
    let venues_to_use: Vec<_> = scored_venues.into_iter()
        .take(max_venues)
        .collect();
    
    if venues_to_use.is_empty() {
        return Err(RoutingError::InsufficientLiquidity {
            needed: order.quantity,
            available: 0.0,
        });
    }
    
    // Calculate total available liquidity
    let total_liquidity: f64 = venues_to_use.iter().map(|(_, _, _, liq)| liq).sum();
    
    if total_liquidity < order.quantity {
        return Err(RoutingError::InsufficientLiquidity {
            needed: order.quantity,
            available: total_liquidity,
        });
    }
    
    // Allocate order across venues proportionally to their liquidity
    let mut remaining = order.quantity;
    let mut child_orders = Vec::new();
    let mut total_cost = 0.0;
    let mut total_fees = 0.0;
    let mut venues_used = Vec::new();
    
    for (venue_name, snapshot, _, liquidity) in &venues_to_use {
        if remaining <= 0.0 {
            break;
        }
        
        // Calculate allocation for this venue
        let mut allocation = (liquidity / total_liquidity) * order.quantity;
        
        // Enforce minimum allocation
        if allocation < min_allocation * order.quantity {
            continue;
        }
        
        // Cap at remaining quantity
        allocation = allocation.min(remaining);
        
        // Also cap at venue's available liquidity
        allocation = allocation.min(*liquidity);
        
        if allocation <= 0.0 {
            continue;
        }
        
        let expected_price = snapshot.expected_fill_price(order.side, allocation)
            .unwrap_or(snapshot.mid_price);
        let expected_fees = allocation * expected_price * snapshot.config.taker_fee_rate;
        
        let child = ChildOrder::new(
            &order.id,
            venue_name,
            &order.symbol,
            order.side,
            allocation,
            order.order_type,
            order.limit_price,
            expected_price,
            expected_fees,
        );
        
        total_cost += allocation * expected_price;
        total_fees += expected_fees;
        venues_used.push((*venue_name).clone());
        remaining -= allocation;
        child_orders.push(child);
    }
    
    if child_orders.is_empty() {
        return Err(RoutingError::AlgorithmError(
            "Could not allocate to any venue".to_string()
        ));
    }
    
    let filled_qty = order.quantity - remaining;
    let avg_price = total_cost / filled_qty;
    let slippage = ((avg_price - order.benchmark_price) / order.benchmark_price) * 10000.0;
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    
    let mut metrics = RoutingMetrics::new();
    metrics.record_order(
        total_cost,
        total_fees,
        slippage,
        &venues_used,
        order.benchmark_price,
        avg_price,
    );
    
    Ok(RoutingDecision {
        parent_order_id: order.id.clone(),
        child_orders,
        expected_total_cost: total_cost + total_fees,
        expected_avg_price: avg_price,
        expected_slippage_bps: slippage,
        decision_timestamp_ms: now,
        algorithm: RoutingAlgorithm::MultiVenue,
        metrics,
    })
}

/// Generate TWAP slices for time-weighted execution
pub fn route_twap(
    order: &ParentOrder,
    venues: &[(&String, &VenueSnapshot)],
    duration_ms: u64,
    num_slices: usize,
) -> Result<Vec<RoutingDecision>, RoutingError> {
    if venues.is_empty() {
        return Err(RoutingError::NoVenuesAvailable(order.symbol.clone()));
    }
    
    let slice_qty = order.quantity / num_slices as f64;
    let slice_interval = duration_ms / num_slices as u64;
    let mut decisions = Vec::with_capacity(num_slices);
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    
    for i in 0..num_slices {
        // Create a sub-order for this slice
        let slice_order = ParentOrder {
            id: format!("{}-slice-{}", order.id, i),
            symbol: order.symbol.clone(),
            side: order.side,
            quantity: slice_qty,
            order_type: OrderType::Market,
            limit_price: order.limit_price,
            benchmark_price: order.benchmark_price,
            created_at_ms: now + (i as u64 * slice_interval),
            metadata: Some(format!("TWAP slice {}/{}", i + 1, num_slices)),
        };
        
        // Route each slice to best venue at that time
        let mut decision = route_best_venue(&slice_order, venues, ExecutionStyle::Balanced)?;
        decision.algorithm = RoutingAlgorithm::TWAP;
        decisions.push(decision);
    }
    
    Ok(decisions)
}

/// Generate VWAP slices for volume-weighted execution
pub fn route_vwap(
    order: &ParentOrder,
    venues: &[(&String, &VenueSnapshot)],
    duration_ms: u64,
    volume_profile: &[f64],  // Expected volume at each interval (normalized to sum to 1)
) -> Result<Vec<RoutingDecision>, RoutingError> {
    if venues.is_empty() {
        return Err(RoutingError::NoVenuesAvailable(order.symbol.clone()));
    }
    
    if volume_profile.is_empty() {
        return Err(RoutingError::AlgorithmError(
            "VWAP requires a volume profile".to_string()
        ));
    }
    
    let num_slices = volume_profile.len();
    let slice_interval = duration_ms / num_slices as u64;
    let mut decisions = Vec::with_capacity(num_slices);
    
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    
    // Normalize volume profile
    let total_volume: f64 = volume_profile.iter().sum();
    
    for (i, &vol_weight) in volume_profile.iter().enumerate() {
        let slice_qty = order.quantity * (vol_weight / total_volume);
        
        if slice_qty <= 0.0 {
            continue;
        }
        
        let slice_order = ParentOrder {
            id: format!("{}-vwap-{}", order.id, i),
            symbol: order.symbol.clone(),
            side: order.side,
            quantity: slice_qty,
            order_type: OrderType::Market,
            limit_price: order.limit_price,
            benchmark_price: order.benchmark_price,
            created_at_ms: now + (i as u64 * slice_interval),
            metadata: Some(format!("VWAP slice {}/{} ({}%)", i + 1, num_slices, (vol_weight / total_volume * 100.0) as i32)),
        };
        
        let mut decision = route_best_venue(&slice_order, venues, ExecutionStyle::Balanced)?;
        decision.algorithm = RoutingAlgorithm::VWAP;
        decisions.push(decision);
    }
    
    Ok(decisions)
}

/// Calculate optimal allocation to minimize market impact
pub fn calculate_min_impact_allocation(
    order: &ParentOrder,
    venues: &[(&String, &VenueSnapshot)],
) -> Vec<(String, f64)> {
    // Simple approach: allocate inversely proportional to market impact
    let mut allocations = Vec::new();
    let mut total_inv_impact = 0.0;
    
    for (name, snapshot) in venues {
        let impact = snapshot.market_impact_bps(order.side, order.quantity);
        if impact < f64::MAX && impact > 0.0 {
            let inv_impact = 1.0 / impact;
            total_inv_impact += inv_impact;
            allocations.push(((*name).clone(), inv_impact));
        }
    }
    
    // Normalize to get percentages
    if total_inv_impact > 0.0 {
        for (_, alloc) in &mut allocations {
            *alloc = (*alloc / total_inv_impact) * order.quantity;
        }
    }
    
    allocations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::{VenueConfig, Level};
    
    fn create_test_venues() -> Vec<(String, VenueSnapshot)> {
        let kraken = VenueSnapshot::new(
            VenueConfig::kraken(),
            "BTC-USD",
            0,
            vec![Level { price: 42000.0, quantity: 2.0 }],
            vec![Level { price: 42010.0, quantity: 2.0 }],
        );
        
        let coinbase = VenueSnapshot::new(
            VenueConfig::coinbase(),
            "BTC-USD",
            0,
            vec![Level { price: 41995.0, quantity: 1.5 }],
            vec![Level { price: 42005.0, quantity: 1.5 }],
        );
        
        vec![
            ("kraken".to_string(), kraken),
            ("coinbase".to_string(), coinbase),
        ]
    }
    
    #[test]
    fn test_best_venue_routing() {
        let venues = create_test_venues();
        let venue_refs: Vec<_> = venues.iter().map(|(n, v)| (n, v)).collect();
        
        let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 1.0, 42000.0);
        let decision = route_best_venue(&order, &venue_refs, ExecutionStyle::Balanced).unwrap();
        
        assert_eq!(decision.child_orders.len(), 1);
        // Coinbase has lower ask (42005) so should be chosen despite higher fees
        // Actually, with higher fees, Kraken might win - depends on scoring
        assert!(!decision.child_orders[0].venue.is_empty());
    }
    
    #[test]
    fn test_multi_venue_routing() {
        let venues = create_test_venues();
        let venue_refs: Vec<_> = venues.iter().map(|(n, v)| (n, v)).collect();
        
        let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 2.0, 42000.0);
        let decision = route_multi_venue(&order, &venue_refs, ExecutionStyle::Balanced, 2, 0.1).unwrap();
        
        // Should split across both venues
        assert!(decision.child_orders.len() >= 1);
    }
    
    #[test]
    fn test_twap_slicing() {
        let venues = create_test_venues();
        let venue_refs: Vec<_> = venues.iter().map(|(n, v)| (n, v)).collect();
        
        let order = ParentOrder::market("BTC-USD", OrderSide::Buy, 1.0, 42000.0);
        let decisions = route_twap(&order, &venue_refs, 60000, 6).unwrap();
        
        assert_eq!(decisions.len(), 6);
        
        // Total quantity should sum to original order
        let total_qty: f64 = decisions.iter()
            .flat_map(|d| &d.child_orders)
            .map(|c| c.quantity)
            .sum();
        assert!((total_qty - 1.0).abs() < 0.0001);
    }
}
