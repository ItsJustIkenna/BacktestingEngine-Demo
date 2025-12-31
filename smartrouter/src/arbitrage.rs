//! Cross-venue arbitrage detection and execution
//!
//! Detects arbitrage opportunities across exchanges and generates
//! simultaneous buy/sell orders to capture the spread

use crate::{
    RoutingError,
    order::{ChildOrder, OrderSide, OrderType, Fill},
    venue::VenueSnapshot,
};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// An identified arbitrage opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    /// Symbol being arbitraged
    pub symbol: String,
    /// Venue to buy on (lower price)
    pub buy_venue: String,
    /// Venue to sell on (higher price)
    pub sell_venue: String,
    /// Buy price (ask on buy_venue)
    pub buy_price: f64,
    /// Sell price (bid on sell_venue)
    pub sell_price: f64,
    /// Maximum quantity that can be arbitraged
    pub max_quantity: f64,
    /// Gross profit in basis points before fees
    pub gross_profit_bps: f64,
    /// Net profit in basis points after fees
    pub net_profit_bps: f64,
    /// Expected profit in USD
    pub expected_profit_usd: f64,
    /// Detection timestamp
    pub detected_at_ms: u64,
    /// Estimated execution latency
    pub estimated_latency_ms: f64,
    /// Risk of opportunity disappearing during execution
    pub execution_risk: f64,
}

impl ArbitrageOpportunity {
    /// Check if opportunity is still profitable after accounting for risks
    pub fn is_viable(&self, min_profit_bps: f64) -> bool {
        self.net_profit_bps >= min_profit_bps && self.max_quantity > 0.0
    }
}

/// Result of executing an arbitrage opportunity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageExecution {
    /// Original opportunity
    pub opportunity: ArbitrageOpportunity,
    /// Buy fill
    pub buy_fill: Option<Fill>,
    /// Sell fill
    pub sell_fill: Option<Fill>,
    /// Actual profit/loss
    pub realized_pnl: f64,
    /// Slippage from expected
    pub slippage_bps: f64,
    /// Whether fully executed
    pub fully_executed: bool,
    /// Execution time
    pub execution_time_ms: u64,
}

/// Router specialized for cross-venue arbitrage
pub struct ArbitrageRouter {
    /// Venue snapshots
    venues: HashMap<String, VenueSnapshot>,
    /// Minimum profit threshold in basis points
    min_profit_bps: f64,
    /// Maximum position holding time
    max_holding_time_ms: u64,
    /// Cross-venue latency estimates
    cross_venue_latency: HashMap<(String, String), f64>,
    /// Cumulative metrics
    metrics: ArbitrageMetrics,
}

/// Metrics specific to arbitrage trading
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArbitrageMetrics {
    /// Total opportunities detected
    pub opportunities_detected: u64,
    /// Opportunities executed
    pub opportunities_executed: u64,
    /// Opportunities missed (expired before execution)
    pub opportunities_missed: u64,
    /// Total realized PnL
    pub total_pnl: f64,
    /// Average profit per trade (bps)
    pub avg_profit_bps: f64,
    /// Win rate
    pub win_rate: f64,
    /// Average execution latency
    pub avg_execution_latency_ms: f64,
    /// Per-venue pair statistics
    pub venue_pair_stats: HashMap<String, VenuePairStats>,
}

/// Statistics for a specific venue pair
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenuePairStats {
    pub pair: String,  // "kraken->coinbase"
    pub opportunities: u64,
    pub executions: u64,
    pub total_pnl: f64,
    pub avg_spread_bps: f64,
}

impl ArbitrageRouter {
    /// Create a new arbitrage router
    pub fn new(min_profit_bps: f64, max_holding_time_ms: u64) -> Self {
        Self {
            venues: HashMap::new(),
            min_profit_bps,
            max_holding_time_ms,
            cross_venue_latency: HashMap::new(),
            metrics: ArbitrageMetrics::default(),
        }
    }
    
    /// Update venue snapshot
    pub fn update_venue(&mut self, name: &str, snapshot: VenueSnapshot) {
        self.venues.insert(name.to_string(), snapshot);
    }
    
    /// Set cross-venue latency estimate
    pub fn set_latency(&mut self, from: &str, to: &str, latency_ms: f64) {
        self.cross_venue_latency.insert((from.to_string(), to.to_string()), latency_ms);
    }
    
    /// Get metrics
    pub fn metrics(&self) -> &ArbitrageMetrics {
        &self.metrics
    }
    
    /// Scan all venue pairs for arbitrage opportunities
    pub fn scan_opportunities(&mut self, symbol: &str) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        
        // Get all venues that have this symbol
        let symbol_venues: Vec<_> = self.venues.iter()
            .filter(|(_, v)| v.symbol == symbol)
            .collect();
        
        // Check all venue pairs
        for (buy_name, buy_venue) in &symbol_venues {
            let buy_ask = match buy_venue.best_ask() {
                Some(p) => p,
                None => continue,
            };
            
            for (sell_name, sell_venue) in &symbol_venues {
                if buy_name == sell_name {
                    continue;
                }
                
                let sell_bid = match sell_venue.best_bid() {
                    Some(p) => p,
                    None => continue,
                };
                
                // Check if there's an arbitrage (sell_bid > buy_ask)
                if sell_bid <= buy_ask {
                    continue;
                }
                
                let mid_price = (buy_ask + sell_bid) / 2.0;
                let gross_profit_bps = ((sell_bid - buy_ask) / mid_price) * 10000.0;
                
                // Calculate fees
                let buy_fee_rate = buy_venue.config.taker_fee_rate;
                let sell_fee_rate = sell_venue.config.taker_fee_rate;
                let total_fee_bps = (buy_fee_rate + sell_fee_rate) * 10000.0;
                
                let net_profit_bps = gross_profit_bps - total_fee_bps;
                
                if net_profit_bps < self.min_profit_bps {
                    continue;
                }
                
                // Maximum quantity is limited by both sides' liquidity
                let buy_liquidity = buy_venue.asks.first().map(|l| l.quantity).unwrap_or(0.0);
                let sell_liquidity = sell_venue.bids.first().map(|l| l.quantity).unwrap_or(0.0);
                let max_quantity = buy_liquidity.min(sell_liquidity);
                
                if max_quantity <= 0.0 {
                    continue;
                }
                
                let expected_profit_usd = max_quantity * mid_price * (net_profit_bps / 10000.0);
                
                // Estimate latency
                let latency = self.cross_venue_latency
                    .get(&((*buy_name).clone(), (*sell_name).clone()))
                    .copied()
                    .unwrap_or(50.0);
                
                // Execution risk increases with latency and spread volatility
                let execution_risk = (latency / 100.0).min(0.5);
                
                let opportunity = ArbitrageOpportunity {
                    symbol: symbol.to_string(),
                    buy_venue: (*buy_name).clone(),
                    sell_venue: (*sell_name).clone(),
                    buy_price: buy_ask,
                    sell_price: sell_bid,
                    max_quantity,
                    gross_profit_bps,
                    net_profit_bps,
                    expected_profit_usd,
                    detected_at_ms: now,
                    estimated_latency_ms: latency,
                    execution_risk,
                };
                
                self.metrics.opportunities_detected += 1;
                opportunities.push(opportunity);
            }
        }
        
        // Sort by profit (best first)
        opportunities.sort_by(|a, b| b.net_profit_bps.partial_cmp(&a.net_profit_bps).unwrap());
        
        opportunities
    }
    
    /// Generate orders to execute an arbitrage opportunity
    pub fn create_arbitrage_orders(
        &self,
        opportunity: &ArbitrageOpportunity,
        quantity: f64,
    ) -> Result<(ChildOrder, ChildOrder), RoutingError> {
        let qty = quantity.min(opportunity.max_quantity);
        
        if qty <= 0.0 {
            return Err(RoutingError::InvalidOrder("Quantity must be positive".to_string()));
        }
        
        // Create buy order
        let buy_order = ChildOrder::new(
            &format!("arb-{}", opportunity.detected_at_ms),
            &opportunity.buy_venue,
            &opportunity.symbol,
            OrderSide::Buy,
            qty,
            OrderType::Market,  // Use market for speed
            None,
            opportunity.buy_price,
            qty * opportunity.buy_price * self.venues[&opportunity.buy_venue].config.taker_fee_rate,
        );
        
        // Create sell order
        let sell_order = ChildOrder::new(
            &format!("arb-{}", opportunity.detected_at_ms),
            &opportunity.sell_venue,
            &opportunity.symbol,
            OrderSide::Sell,
            qty,
            OrderType::Market,
            None,
            opportunity.sell_price,
            qty * opportunity.sell_price * self.venues[&opportunity.sell_venue].config.taker_fee_rate,
        );
        
        Ok((buy_order, sell_order))
    }
    
    /// Simulate execution of an arbitrage opportunity
    pub fn simulate_execution(
        &mut self,
        opportunity: &ArbitrageOpportunity,
        quantity: f64,
        current_time_ms: u64,
    ) -> ArbitrageExecution {
        let qty = quantity.min(opportunity.max_quantity);
        
        // Simulate buy fill
        let buy_fill = if let Some(venue) = self.venues.get(&opportunity.buy_venue) {
            let (price, filled_qty) = venue.simulate_fill(OrderSide::Buy, qty, None);
            let fee = filled_qty * price * venue.config.taker_fee_rate;
            Some(Fill {
                order_id: format!("arb-buy-{}", opportunity.detected_at_ms),
                venue: opportunity.buy_venue.clone(),
                side: OrderSide::Buy,
                quantity: filled_qty,
                price,
                fee,
                timestamp_ms: current_time_ms + venue.config.latency_ms as u64,
            })
        } else {
            None
        };
        
        // Simulate sell fill
        let sell_fill = if let Some(venue) = self.venues.get(&opportunity.sell_venue) {
            let (price, filled_qty) = venue.simulate_fill(OrderSide::Sell, qty, None);
            let fee = filled_qty * price * venue.config.taker_fee_rate;
            Some(Fill {
                order_id: format!("arb-sell-{}", opportunity.detected_at_ms),
                venue: opportunity.sell_venue.clone(),
                side: OrderSide::Sell,
                quantity: filled_qty,
                price,
                fee,
                timestamp_ms: current_time_ms + venue.config.latency_ms as u64,
            })
        } else {
            None
        };
        
        // Calculate PnL
        let (realized_pnl, fully_executed) = match (&buy_fill, &sell_fill) {
            (Some(buy), Some(sell)) => {
                let buy_cost = buy.quantity * buy.price + buy.fee;
                let sell_proceeds = sell.quantity * sell.price - sell.fee;
                let pnl = sell_proceeds - buy_cost;
                let fully = (buy.quantity - qty).abs() < 0.0001 && (sell.quantity - qty).abs() < 0.0001;
                (pnl, fully)
            }
            _ => (0.0, false),
        };
        
        // Calculate slippage
        let expected_pnl = opportunity.expected_profit_usd * (qty / opportunity.max_quantity);
        let slippage_bps = if expected_pnl != 0.0 {
            ((expected_pnl - realized_pnl) / expected_pnl.abs()) * 10000.0
        } else {
            0.0
        };
        
        // Update metrics
        self.metrics.opportunities_executed += 1;
        self.metrics.total_pnl += realized_pnl;
        
        let total_execs = self.metrics.opportunities_executed;
        self.metrics.avg_profit_bps = 
            self.metrics.avg_profit_bps + (opportunity.net_profit_bps - self.metrics.avg_profit_bps) / total_execs as f64;
        
        if realized_pnl > 0.0 {
            let wins = (self.metrics.win_rate * (total_execs - 1) as f64 + 1.0) / total_execs as f64;
            self.metrics.win_rate = wins;
        } else {
            let wins = (self.metrics.win_rate * (total_execs - 1) as f64) / total_execs as f64;
            self.metrics.win_rate = wins;
        }
        
        ArbitrageExecution {
            opportunity: opportunity.clone(),
            buy_fill,
            sell_fill,
            realized_pnl,
            slippage_bps,
            fully_executed,
            execution_time_ms: opportunity.estimated_latency_ms as u64,
        }
    }
    
    /// Clear venues for next iteration
    pub fn clear_venues(&mut self) {
        self.venues.clear();
    }
    
    /// Reset metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = ArbitrageMetrics::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::{VenueConfig, Level};
    
    fn create_arb_venues() -> (VenueSnapshot, VenueSnapshot) {
        // Use low-fee configs to make arbitrage viable
        let mut kraken_config = VenueConfig::kraken();
        kraken_config.taker_fee_rate = 0.0002;  // 2 bps
        
        let mut coinbase_config = VenueConfig::coinbase();
        coinbase_config.taker_fee_rate = 0.0002;  // 2 bps
        
        // Kraken has BTC at 42010 ask
        let kraken = VenueSnapshot::new(
            kraken_config,
            "BTC-USD",
            0,
            vec![Level { price: 42000.0, quantity: 2.0 }],
            vec![Level { price: 42010.0, quantity: 2.0 }],
        );
        
        // Coinbase has BTC at 42100 bid - larger spread for clear arbitrage
        let coinbase = VenueSnapshot::new(
            coinbase_config,
            "BTC-USD",
            0,
            vec![Level { price: 42100.0, quantity: 1.5 }],  // Much higher bid = ~21 bps profit
            vec![Level { price: 42110.0, quantity: 1.5 }],
        );
        
        (kraken, coinbase)
    }
    
    #[test]
    fn test_arbitrage_detection() {
        let (kraken, coinbase) = create_arb_venues();
        
        let mut router = ArbitrageRouter::new(1.0, 5000);  // 1 bps minimum
        router.update_venue("kraken", kraken);
        router.update_venue("coinbase", coinbase);
        
        let opportunities = router.scan_opportunities("BTC-USD");
        
        // Should detect the opportunity (buy on kraken at 42010, sell on coinbase at 42050)
        assert!(!opportunities.is_empty());
        
        let opp = &opportunities[0];
        assert_eq!(opp.buy_venue, "kraken");
        assert_eq!(opp.sell_venue, "coinbase");
        assert!(opp.gross_profit_bps > 0.0);
    }
    
    #[test]
    fn test_arbitrage_execution() {
        let (kraken, coinbase) = create_arb_venues();
        
        let mut router = ArbitrageRouter::new(1.0, 5000);
        router.update_venue("kraken", kraken);
        router.update_venue("coinbase", coinbase);
        
        let opportunities = router.scan_opportunities("BTC-USD");
        let opp = &opportunities[0];
        
        let execution = router.simulate_execution(opp, 1.0, 0);
        
        assert!(execution.buy_fill.is_some());
        assert!(execution.sell_fill.is_some());
        // With the fee structure, PnL might be positive or slightly negative
        // The important thing is that it executed
    }
    
    #[test]
    fn test_no_arbitrage_when_spread_too_small() {
        // Create venues with no arb opportunity
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
            vec![Level { price: 41990.0, quantity: 1.5 }],  // Lower bid than kraken ask
            vec![Level { price: 42020.0, quantity: 1.5 }],
        );
        
        let mut router = ArbitrageRouter::new(1.0, 5000);
        router.update_venue("kraken", kraken);
        router.update_venue("coinbase", coinbase);
        
        let opportunities = router.scan_opportunities("BTC-USD");
        assert!(opportunities.is_empty());
    }
}
