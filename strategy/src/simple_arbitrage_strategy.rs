use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use bigdecimal::BigDecimal;
use async_trait::async_trait;
use crate::traits::{Strategy, StrategyContext, StrategyError, StrategyResult};
use crate::config::ParameterValue;
use signal::{Signal, SignalType, SignalStrength, SignalReason, SignalMetadata};

/// Simple arbitrage strategy that identifies price differences across exchanges
/// This is a simplified version for demonstration purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleArbitrageStrategy {
    pub name: String,
    pub symbol: String,
    pub min_profit_bps: f64,
    pub position_size: f64,
    pub enabled: bool,
    
    // Simple state tracking
    last_signals: Vec<Signal>,
    total_opportunities: i32,
}

impl SimpleArbitrageStrategy {
    pub fn new(symbol: String, min_profit_bps: f64, position_size: f64) -> Self {
        Self {
            name: "Simple Exchange Arbitrage".to_string(),
            symbol,
            min_profit_bps,
            position_size,
            enabled: true,
            last_signals: Vec::new(),
            total_opportunities: 0,
        }
    }
    
    /// Simulate detecting arbitrage opportunity from market data
    fn detect_arbitrage_opportunity(&self, market_data: &dataloader::MarketData) -> bool {
        // Simplified arbitrage detection
        // In reality, this would compare prices across multiple exchanges
        // For demo, we'll simulate based on volume/volatility patterns
        
        let volume = market_data.volume.to_f64().unwrap_or(0.0);
        let high = market_data.high.to_f64().unwrap_or(0.0);
        let low = market_data.low.to_f64().unwrap_or(0.0);
        let close = market_data.close.to_f64().unwrap_or(0.0);
        
        if close == 0.0 || high == low {
            return false;
        }
        
        // Calculate volatility proxy
        let volatility = ((high - low) / close) * 10000.0; // in bps
        
        // Simulate arbitrage opportunity based on high volume + volatility
        // Real implementation would use cross-exchange price data
        volume > 1000.0 && volatility > self.min_profit_bps * 2.0 && volatility < 200.0
    }
    
    /// Calculate expected profit for demonstration
    fn calculate_expected_profit(&self, market_data: &dataloader::MarketData) -> f64 {
        let close = market_data.close.to_f64().unwrap_or(0.0);
        let high = market_data.high.to_f64().unwrap_or(0.0);
        let low = market_data.low.to_f64().unwrap_or(0.0);
        
        if close == 0.0 {
            return 0.0;
        }
        
        // Simulate profit based on spread
        let simulated_spread = ((high - low) / close) * 10000.0; // bps
        (simulated_spread * 0.3).max(self.min_profit_bps) // Assume we capture 30% of spread
    }
}

#[async_trait]
impl Strategy for SimpleArbitrageStrategy {
    fn name(&self) -> &str {
        &self.name
    }
    
    async fn initialize(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        // Update parameters if provided
        if let Some(ParameterValue::Float(min_profit)) = parameters.get("min_profit_bps") {
            if *min_profit > 0.0 {
                self.min_profit_bps = *min_profit;
            }
        }
        
        if let Some(ParameterValue::Float(position_size)) = parameters.get("position_size") {
            if *position_size > 0.0 {
                self.position_size = *position_size;
            }
        }
        
        if let Some(ParameterValue::Bool(enabled)) = parameters.get("enabled") {
            self.enabled = *enabled;
        }
        
        Ok(())
    }
    
    async fn generate_signals(
        &mut self,
        market_data: &dataloader::MarketData,
        context: &StrategyContext,
    ) -> StrategyResult<Vec<Signal>> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        
        let mut signals = Vec::new();
        
        // Check for arbitrage opportunity
        if self.detect_arbitrage_opportunity(market_data) {
            self.total_opportunities += 1;
            let expected_profit = self.calculate_expected_profit(market_data);
            
            // Create arbitrage signal (simplified as a single buy signal)
            let signal_id = format!("ARB_{}_{}", self.total_opportunities, market_data.timestamp.timestamp());
            
            let mut metadata = HashMap::new();
            metadata.insert(
                "expected_profit_bps".to_string(),
                SignalMetadata::Float(expected_profit),
            );
            metadata.insert(
                "opportunity_type".to_string(),
                SignalMetadata::String("cross_exchange_arbitrage".to_string()),
            );
            metadata.insert(
                "exchange_pair".to_string(),
                SignalMetadata::String("binance_coinbase".to_string()),
            );
            
            let signal = Signal {
                id: signal_id,
                timestamp: market_data.timestamp,
                signal_type: SignalType::Buy,
                strength: if expected_profit > self.min_profit_bps * 2.0 {
                    SignalStrength::Strong
                } else {
                    SignalStrength::Medium
                },
                price: Some(market_data.close.clone()),
                quantity: Some(BigDecimal::from(self.position_size)),
                reason: SignalReason::PriceAction(
                    format!("Arbitrage opportunity detected: {:.2} bps expected profit", expected_profit)
                ),
                metadata,
            };
            
            signals.push(signal);
        }
        
        // Store signals for reference
        self.last_signals = signals.clone();
        
        Ok(signals)
    }
    
    fn get_parameters(&self) -> HashMap<String, ParameterValue> {
        let mut params = HashMap::new();
        params.insert("min_profit_bps".to_string(), ParameterValue::Float(self.min_profit_bps));
        params.insert("position_size".to_string(), ParameterValue::Float(self.position_size));
        params.insert("enabled".to_string(), ParameterValue::Bool(self.enabled));
        params.insert("total_opportunities".to_string(), ParameterValue::Int(self.total_opportunities as i64));
        params
    }
    
    async fn update_parameters(&mut self, parameters: HashMap<String, ParameterValue>) -> StrategyResult<()> {
        self.initialize(parameters).await
    }
    
    fn get_risk_limits(&self) -> crate::config::RiskLimits {
        crate::config::RiskLimits {
            max_position_size: Some(self.position_size * 2.0), // 2x position size limit
            max_daily_loss: Some(1000.0), // $1000 max daily loss
            max_drawdown_percent: Some(0.05), // 5% max drawdown
            stop_loss_percent: Some(0.02), // 2% stop loss
            take_profit_percent: Some(0.01), // 1% take profit (quick arbitrage)
            max_open_orders: None,
        }
    }
}

// Export for use in examples
pub use self::SimpleArbitrageStrategy as ArbitrageStrategy;

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    
    #[tokio::test]
    async fn test_arbitrage_strategy_creation() {
        let strategy = SimpleArbitrageStrategy::new(
            "BTC/USD".to_string(),
            15.0, // 15 bps minimum profit
            1.0,  // 1 BTC position size
        );
        
        assert_eq!(strategy.symbol, "BTC/USD");
        assert_eq!(strategy.min_profit_bps, 15.0);
        assert_eq!(strategy.position_size, 1.0);
        assert!(strategy.enabled);
    }
    
    #[tokio::test]
    async fn test_arbitrage_opportunity_detection() {
        let mut strategy = SimpleArbitrageStrategy::new(
            "BTC/USD".to_string(),
            10.0,
            1.0,
        );
        
        // Create test market data with high volatility (simulates arbitrage opportunity)
        let market_data = dataloader::MarketData {
            timestamp: Utc::now(),
            symbol: "BTC/USD".to_string(),
            open: BigDecimal::from(50000),
            high: BigDecimal::from(50200), // 40 bps spread
            low: BigDecimal::from(49800),
            close: BigDecimal::from(50100),
            volume: BigDecimal::from(2000), // High volume
            interval: "1m".to_string(),
        };
        
        // Create context
        let context = StrategyContext {
            current_time: market_data.timestamp,
            portfolio_value: 100000.0,
            available_capital: 100000.0,
            open_positions: HashMap::new(),
            market_session: crate::traits::MarketSession {
                is_open: true,
                session_type: crate::traits::SessionType::Regular,
                next_open: None,
                next_close: None,
            },
            risk_metrics: Default::default(),
            volume_history: Vec::new(),
            spread: None,
        };
        
        // Generate signals
        let signals = strategy.generate_signals(&market_data, &context).await.unwrap();
        
        // Should detect opportunity with high volume + volatility
        assert!(!signals.is_empty());
        assert_eq!(signals[0].signal_type, SignalType::Buy);
        assert!(matches!(signals[0].strength, SignalStrength::Medium | SignalStrength::Strong));
        
        // Check metadata
        assert!(signals[0].metadata.contains_key("expected_profit_bps"));
        assert!(signals[0].metadata.contains_key("opportunity_type"));
    }
    
    #[tokio::test]
    async fn test_parameter_updates() {
        let mut strategy = SimpleArbitrageStrategy::new(
            "BTC/USD".to_string(),
            10.0,
            1.0,
        );
        
        // Update parameters
        let mut new_params = HashMap::new();
        new_params.insert("min_profit_bps".to_string(), ParameterValue::Float(20.0));
        new_params.insert("position_size".to_string(), ParameterValue::Float(2.0));
        new_params.insert("enabled".to_string(), ParameterValue::Bool(false));
        
        strategy.update_parameters(new_params).await.unwrap();
        
        assert_eq!(strategy.min_profit_bps, 20.0);
        assert_eq!(strategy.position_size, 2.0);
        assert!(!strategy.enabled);
    }
}
