use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use bigdecimal::BigDecimal;
use crate::traits::{Strategy, StrategyContext, StrategyError};
use crate::signal::{Signal, SignalType as SignalAction};

/// Exchange arbitrage strategy that identifies and exploits price differences across exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageStrategy {
    pub symbol: String,
    pub min_profit_bps: f64,              // Minimum profit threshold in basis points
    pub max_position_size: f64,           // Maximum position size per arbitrage
    pub max_holding_time_seconds: u64,    // Maximum time to hold positions
    pub cross_exchange_latency_ms: f64,   // Estimated cross-exchange execution latency
    pub transaction_cost_bps: f64,        // Total transaction costs (fees + slippage)
    
    // Strategy state
    active_arbitrages: HashMap<String, ActiveArbitrage>,
    exchange_prices: HashMap<String, ExchangePrice>,
    last_update_time: DateTime<Utc>,
    profit_history: Vec<f64>,
    
    // Configuration
    enabled_exchanges: Vec<String>,
    risk_limits: ArbitrageRiskLimits,
}

/// Active arbitrage position tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveArbitrage {
    id: String,
    buy_exchange: String,
    sell_exchange: String,
    entry_price_buy: f64,
    entry_price_sell: f64,
    position_size: f64,
    entry_time: DateTime<Utc>,
    expected_profit_bps: f64,
    status: ArbitrageStatus,
}

/// Arbitrage position status
#[derive(Debug, Clone, Serialize, Deserialize)]
enum ArbitrageStatus {
    Pending,        // Orders submitted but not filled
    Active,         // Both legs filled, holding position
    Closing,        // Closing position
    Completed,      // Both legs closed
    Failed,         // Failed to execute
}

/// Exchange price tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangePrice {
    pub exchange: String,
    pub best_bid: f64,
    pub best_ask: f64,
    pub bid_size: f64,
    pub ask_size: f64,
    pub timestamp: DateTime<Utc>,
    pub latency_ms: f64,
}

/// Risk management limits for arbitrage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageRiskLimits {
    pub max_open_positions: u32,
    pub max_daily_loss: f64,
    pub max_exchange_exposure: f64,      // Maximum exposure per exchange
    pub min_liquidity_ratio: f64,        // Minimum liquidity vs position size
    pub max_correlation_threshold: f64,  // Maximum price correlation between exchanges
}

impl Default for ArbitrageRiskLimits {
    fn default() -> Self {
        Self {
            max_open_positions: 5,
            max_daily_loss: 1000.0,
            max_exchange_exposure: 10000.0,
            min_liquidity_ratio: 5.0,
            max_correlation_threshold: 0.95,
        }
    }
}

impl ArbitrageStrategy {
    pub fn new(
        symbol: String,
        min_profit_bps: f64,
        max_position_size: f64,
        max_holding_time_seconds: u64,
        enabled_exchanges: Vec<String>,
    ) -> Result<Self, StrategyError> {
        if min_profit_bps <= 0.0 {
            return Err(StrategyError::InvalidParameter("min_profit_bps must be positive".to_string()));
        }
        
        if enabled_exchanges.len() < 2 {
            return Err(StrategyError::InvalidParameter("At least 2 exchanges required for arbitrage".to_string()));
        }

        Ok(Self {
            symbol,
            min_profit_bps,
            max_position_size,
            max_holding_time_seconds,
            cross_exchange_latency_ms: 50.0, // Default 50ms
            transaction_cost_bps: 20.0,      // Default 20bps total costs
            
            active_arbitrages: HashMap::new(),
            exchange_prices: HashMap::new(),
            last_update_time: Utc::now(),
            profit_history: Vec::new(),
            
            enabled_exchanges,
            risk_limits: ArbitrageRiskLimits::default(),
        })
    }
    
    /// Update exchange prices and detect arbitrage opportunities
    pub fn update_exchange_prices(&mut self, prices: Vec<ExchangePrice>) {
        self.last_update_time = Utc::now();
        
        // Update price cache
        for price in prices {
            self.exchange_prices.insert(price.exchange.clone(), price);
        }
        
        // Clean up stale prices (older than 5 seconds)
        let cutoff = Utc::now() - chrono::Duration::seconds(5);
        self.exchange_prices.retain(|_, price| price.timestamp > cutoff);
    }
    
    /// Detect arbitrage opportunities across exchanges
    fn detect_arbitrage_opportunities(&self) -> Vec<ArbitrageOpportunity> {
        let mut opportunities = Vec::new();
        let exchanges: Vec<_> = self.exchange_prices.keys().cloned().collect();
        
        // Compare all exchange pairs
        for i in 0..exchanges.len() {
            for j in (i+1)..exchanges.len() {
                let exchange1 = &exchanges[i];
                let exchange2 = &exchanges[j];
                
                if let (Some(price1), Some(price2)) = (
                    self.exchange_prices.get(exchange1),
                    self.exchange_prices.get(exchange2),
                ) {
                    // Check arbitrage: buy on exchange1, sell on exchange2
                    let profit1 = self.calculate_arbitrage_profit(price1, price2);
                    if profit1.profit_bps > self.min_profit_bps {
                        opportunities.push(profit1);
                    }
                    
                    // Check arbitrage: buy on exchange2, sell on exchange1
                    let profit2 = self.calculate_arbitrage_profit(price2, price1);
                    if profit2.profit_bps > self.min_profit_bps {
                        opportunities.push(profit2);
                    }
                }
            }
        }
        
        // Sort by profitability (highest first)
        opportunities.sort_by(|a, b| b.profit_bps.partial_cmp(&a.profit_bps).unwrap());
        opportunities
    }
    
    /// Calculate arbitrage profit between two exchanges
    fn calculate_arbitrage_profit(&self, buy_exchange: &ExchangePrice, sell_exchange: &ExchangePrice) -> ArbitrageOpportunity {
        // Buy at ask price on buy_exchange, sell at bid price on sell_exchange
        let buy_price = buy_exchange.best_ask;
        let sell_price = sell_exchange.best_bid;
        
        // Calculate gross profit
        let gross_profit = sell_price - buy_price;
        let gross_profit_bps = (gross_profit / buy_price) * 10000.0;
        
        // Subtract transaction costs
        let net_profit_bps = gross_profit_bps - self.transaction_cost_bps;
        
        // Determine maximum tradeable size based on liquidity
        let max_size = (buy_exchange.ask_size.min(sell_exchange.bid_size) * 0.5)
            .min(self.max_position_size);
        
        ArbitrageOpportunity {
            buy_exchange: buy_exchange.exchange.clone(),
            sell_exchange: sell_exchange.exchange.clone(),
            buy_price,
            sell_price,
            profit_bps: net_profit_bps,
            max_size,
            confidence: self.calculate_opportunity_confidence(buy_exchange, sell_exchange),
            expiry_time: Utc::now() + chrono::Duration::milliseconds(self.cross_exchange_latency_ms as i64),
        }
    }
    
    /// Calculate confidence score for arbitrage opportunity
    fn calculate_opportunity_confidence(&self, buy_ex: &ExchangePrice, sell_ex: &ExchangePrice) -> f64 {
        let mut confidence = 1.0;
        
        // Reduce confidence based on price staleness
        let max_age = chrono::Duration::seconds(2);
        let buy_age = Utc::now() - buy_ex.timestamp;
        let sell_age = Utc::now() - sell_ex.timestamp;
        
        if buy_age > max_age || sell_age > max_age {
            confidence *= 0.5; // Stale prices are less reliable
        }
        
        // Reduce confidence based on latency
        let avg_latency = (buy_ex.latency_ms + sell_ex.latency_ms) / 2.0;
        if avg_latency > 100.0 {
            confidence *= 0.7; // High latency reduces reliability
        }
        
        // Increase confidence for larger spreads (more robust opportunities)
        let spread_factor = ((sell_ex.best_bid - buy_ex.best_ask) / buy_ex.best_ask).max(0.0);
        confidence *= (1.0 + spread_factor * 10.0).min(1.5);
        
        confidence.min(1.0).max(0.0)
    }
    
    /// Check if we should close existing arbitrage positions
    fn check_position_exits(&mut self) -> Vec<Signal> {
        let mut signals = Vec::new();
        let current_time = Utc::now();
        
        for (id, arbitrage) in self.active_arbitrages.iter_mut() {
            let holding_time = (current_time - arbitrage.entry_time).num_seconds() as u64;
            
            // Check if we should close based on time limit
            if holding_time > self.max_holding_time_seconds {
                // Generate closing signals
                signals.push(Signal {
                    id: format!("ARB_CLOSE_BUY_{}", id),
                    strategy_id: "arbitrage".to_string(),
                    symbol: self.symbol.clone(),
                    action: SignalAction::Sell,
                    quantity: arbitrage.position_size,
                    price: None, // Market order
                    timestamp: current_time,
                    confidence: 0.9,
                    exchange: Some(arbitrage.buy_exchange.clone()),
                    metadata: Some(format!("Closing arbitrage position due to time limit")),
                });
                
                signals.push(Signal {
                    id: format!("ARB_CLOSE_SELL_{}", id),
                    strategy_id: "arbitrage".to_string(),
                    symbol: self.symbol.clone(),
                    action: SignalAction::Buy,
                    quantity: arbitrage.position_size,
                    price: None, // Market order
                    timestamp: current_time,
                    confidence: 0.9,
                    exchange: Some(arbitrage.sell_exchange.clone()),
                    metadata: Some(format!("Closing arbitrage position due to time limit")),
                });
                
                arbitrage.status = ArbitrageStatus::Closing;
            }
        }
        
        signals
    }
    
    /// Validate arbitrage opportunity against risk limits
    fn validate_opportunity(&self, opportunity: &ArbitrageOpportunity) -> bool {
        // Check maximum open positions
        if self.active_arbitrages.len() >= self.risk_limits.max_open_positions as usize {
            return false;
        }
        
        // Check exchange exposure limits
        let buy_exposure = self.get_exchange_exposure(&opportunity.buy_exchange);
        let sell_exposure = self.get_exchange_exposure(&opportunity.sell_exchange);
        
        if buy_exposure + opportunity.max_size > self.risk_limits.max_exchange_exposure ||
           sell_exposure + opportunity.max_size > self.risk_limits.max_exchange_exposure {
            return false;
        }
        
        // Check liquidity requirements
        if let (Some(buy_price), Some(sell_price)) = (
            self.exchange_prices.get(&opportunity.buy_exchange),
            self.exchange_prices.get(&opportunity.sell_exchange),
        ) {
            let min_liquidity = opportunity.max_size * self.risk_limits.min_liquidity_ratio;
            if buy_price.ask_size < min_liquidity || sell_price.bid_size < min_liquidity {
                return false;
            }
        }
        
        true
    }
    
    /// Get current exposure on an exchange
    fn get_exchange_exposure(&self, exchange: &str) -> f64 {
        self.active_arbitrages
            .values()
            .filter(|arb| &arb.buy_exchange == exchange || &arb.sell_exchange == exchange)
            .map(|arb| arb.position_size)
            .sum()
    }
}

/// Detected arbitrage opportunity
#[derive(Debug, Clone)]
struct ArbitrageOpportunity {
    buy_exchange: String,
    sell_exchange: String,
    buy_price: f64,
    sell_price: f64,
    profit_bps: f64,
    max_size: f64,
    confidence: f64,
    expiry_time: DateTime<Utc>,
}

impl Strategy for ArbitrageStrategy {
    fn process_market_data(&mut self, context: &StrategyContext) -> Result<Vec<Signal>, StrategyError> {
        let mut signals = Vec::new();
        
        // First, check for position exits
        signals.extend(self.check_position_exits());
        
        // Skip new opportunities if we have too many active positions
        if self.active_arbitrages.len() >= self.risk_limits.max_open_positions as usize {
            return Ok(signals);
        }
        
        // Detect arbitrage opportunities
        let opportunities = self.detect_arbitrage_opportunities();
        
        // Process the best opportunities
        for opportunity in opportunities.into_iter().take(3) { // Limit to top 3 opportunities
            if !self.validate_opportunity(&opportunity) {
                continue;
            }
            
            // Generate arbitrage signals (simultaneous buy and sell)
            let arbitrage_id = format!("ARB_{}_{}_{}_{}", 
                opportunity.buy_exchange, 
                opportunity.sell_exchange,
                Utc::now().timestamp_millis(),
                fastrand::u32(1000..9999)
            );
            
            // Buy signal (buy on cheaper exchange)
            signals.push(Signal {
                id: format!("ARB_BUY_{}", arbitrage_id),
                strategy_id: "arbitrage".to_string(),
                symbol: self.symbol.clone(),
                action: SignalAction::Buy,
                quantity: opportunity.max_size,
                price: Some(opportunity.buy_price * 1.001), // Slightly higher limit to ensure fill
                timestamp: Utc::now(),
                confidence: opportunity.confidence,
                exchange: Some(opportunity.buy_exchange.clone()),
                metadata: Some(format!("Arbitrage buy leg - expected profit: {:.2} bps", opportunity.profit_bps)),
            });
            
            // Sell signal (sell on more expensive exchange)
            signals.push(Signal {
                id: format!("ARB_SELL_{}", arbitrage_id),
                strategy_id: "arbitrage".to_string(),
                symbol: self.symbol.clone(),
                action: SignalAction::Sell,
                quantity: opportunity.max_size,
                price: Some(opportunity.sell_price * 0.999), // Slightly lower limit to ensure fill
                timestamp: Utc::now(),
                confidence: opportunity.confidence,
                exchange: Some(opportunity.sell_exchange.clone()),
                metadata: Some(format!("Arbitrage sell leg - expected profit: {:.2} bps", opportunity.profit_bps)),
            });
            
            // Track the arbitrage
            self.active_arbitrages.insert(arbitrage_id.clone(), ActiveArbitrage {
                id: arbitrage_id,
                buy_exchange: opportunity.buy_exchange,
                sell_exchange: opportunity.sell_exchange,
                entry_price_buy: opportunity.buy_price,
                entry_price_sell: opportunity.sell_price,
                position_size: opportunity.max_size,
                entry_time: Utc::now(),
                expected_profit_bps: opportunity.profit_bps,
                status: ArbitrageStatus::Pending,
            });
        }
        
        Ok(signals)
    }
    
    fn get_name(&self) -> &str {
        "Exchange Arbitrage Strategy"
    }
    
    fn get_parameters(&self) -> HashMap<String, f64> {
        let mut params = HashMap::new();
        params.insert("min_profit_bps".to_string(), self.min_profit_bps);
        params.insert("max_position_size".to_string(), self.max_position_size);
        params.insert("max_holding_time_seconds".to_string(), self.max_holding_time_seconds as f64);
        params.insert("transaction_cost_bps".to_string(), self.transaction_cost_bps);
        params.insert("cross_exchange_latency_ms".to_string(), self.cross_exchange_latency_ms);
        params
    }
    
    fn update_parameters(&mut self, params: HashMap<String, f64>) {
        if let Some(&value) = params.get("min_profit_bps") {
            if value > 0.0 {
                self.min_profit_bps = value;
            }
        }
        if let Some(&value) = params.get("max_position_size") {
            if value > 0.0 {
                self.max_position_size = value;
            }
        }
        if let Some(&value) = params.get("max_holding_time_seconds") {
            if value > 0.0 {
                self.max_holding_time_seconds = value as u64;
            }
        }
        if let Some(&value) = params.get("transaction_cost_bps") {
            if value >= 0.0 {
                self.transaction_cost_bps = value;
            }
        }
        if let Some(&value) = params.get("cross_exchange_latency_ms") {
            if value >= 0.0 {
                self.cross_exchange_latency_ms = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_arbitrage_strategy_creation() {
        let strategy = ArbitrageStrategy::new(
            "BTC/USD".to_string(),
            10.0, // 10 bps minimum profit
            1.0,  // 1 BTC max position
            300,  // 5 minutes max holding
            vec!["binance".to_string(), "coinbase".to_string()],
        );
        
        assert!(strategy.is_ok());
        let strategy = strategy.unwrap();
        assert_eq!(strategy.symbol, "BTC/USD");
        assert_eq!(strategy.min_profit_bps, 10.0);
        assert_eq!(strategy.enabled_exchanges.len(), 2);
    }
    
    #[test]
    fn test_arbitrage_opportunity_detection() {
        let mut strategy = ArbitrageStrategy::new(
            "BTC/USD".to_string(),
            5.0,  // 5 bps minimum
            1.0,
            300,
            vec!["binance".to_string(), "coinbase".to_string()],
        ).unwrap();
        
        // Add mock exchange prices with arbitrage opportunity
        let prices = vec![
            ExchangePrice {
                exchange: "binance".to_string(),
                best_bid: 50000.0,
                best_ask: 50001.0,
                bid_size: 10.0,
                ask_size: 10.0,
                timestamp: Utc::now(),
                latency_ms: 20.0,
            },
            ExchangePrice {
                exchange: "coinbase".to_string(),
                best_bid: 50005.0, // Higher bid creates arbitrage opportunity
                best_ask: 50006.0,
                bid_size: 8.0,
                ask_size: 8.0,
                timestamp: Utc::now(),
                latency_ms: 30.0,
            },
        ];
        
        strategy.update_exchange_prices(prices);
        let opportunities = strategy.detect_arbitrage_opportunities();
        
        assert!(!opportunities.is_empty());
        assert!(opportunities[0].profit_bps > 5.0); // Should exceed minimum threshold
    }
}
