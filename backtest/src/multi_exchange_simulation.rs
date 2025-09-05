use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use bigdecimal::BigDecimal;
use crate::orderbook::OrderBook;
use crate::config::{BacktestConfig, ExchangeFeeConfig};

/// Multi-exchange order book manager for arbitrage backtesting
#[derive(Debug)]
pub struct MultiExchangeOrderBooks {
    orderbooks: HashMap<String, OrderBook>,
    exchange_configs: HashMap<String, ExchangeFeeConfig>,
    cross_exchange_latency_ms: f64,
    last_sync_time: HashMap<String, DateTime<Utc>>,
}

/// Cross-exchange execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossExchangeExecution {
    pub arbitrage_id: String,
    pub buy_exchange: String,
    pub sell_exchange: String,
    pub buy_execution: ExecutionLeg,
    pub sell_execution: ExecutionLeg,
    pub total_profit_loss: f64,
    pub execution_time_ms: f64,
    pub success: bool,
    pub failure_reason: Option<String>,
}

/// Individual execution leg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLeg {
    pub exchange: String,
    pub side: String,
    pub quantity: f64,
    pub price: f64,
    pub fees: f64,
    pub slippage_bps: f64,
    pub latency_ms: f64,
    pub filled: bool,
}

/// Market data snapshot across multiple exchanges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiExchangeSnapshot {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub exchanges: HashMap<String, ExchangeSnapshot>,
}

/// Individual exchange snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeSnapshot {
    pub exchange: String,
    pub best_bid: f64,
    pub best_ask: f64,
    pub bid_size: f64,
    pub ask_size: f64,
    pub mid_price: f64,
    pub spread_bps: f64,
    pub last_update: DateTime<Utc>,
}

impl MultiExchangeOrderBooks {
    pub fn new(config: &BacktestConfig, exchanges: Vec<String>) -> Self {
        let mut orderbooks = HashMap::new();
        let mut exchange_configs = HashMap::new();
        let mut last_sync_time = HashMap::new();
        
        for exchange in exchanges {
            // Create orderbook for each exchange
            orderbooks.insert(
                exchange.clone(),
                OrderBook::new(exchange.clone(), config.trading.exchange_fees.clone()),
            );
            
            // Set up exchange-specific configurations
            let fee_config = Self::get_exchange_fee_config(&exchange);
            exchange_configs.insert(exchange.clone(), fee_config);
            
            // Initialize sync times
            last_sync_time.insert(exchange.clone(), Utc::now());
        }
        
        Self {
            orderbooks,
            exchange_configs,
            cross_exchange_latency_ms: 50.0, // Default 50ms cross-exchange latency
            last_sync_time,
        }
    }
    
    /// Update market data for a specific exchange
    pub fn update_exchange_data(
        &mut self,
        exchange: &str,
        market_data: &crate::dataloader::MarketData,
    ) -> Result<(), String> {
        if let Some(orderbook) = self.orderbooks.get_mut(exchange) {
            // Update the orderbook with new market data
            // In a real implementation, this would reconstruct the order book
            // from historical order book snapshots or trades
            
            // For simulation, we'll create synthetic order book depth
            let mid_price = market_data.close.to_f64().unwrap_or(50000.0);
            self.generate_synthetic_depth(exchange, mid_price, market_data.timestamp)?;
            
            // Update sync time
            self.last_sync_time.insert(exchange.to_string(), market_data.timestamp);
            
            Ok(())
        } else {
            Err(format!("Exchange {} not found", exchange))
        }
    }
    
    /// Generate synthetic order book depth for simulation
    fn generate_synthetic_depth(
        &mut self,
        exchange: &str,
        mid_price: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<(), String> {
        if let Some(orderbook) = self.orderbooks.get_mut(exchange) {
            // Clear existing orders (simplified approach)
            // In reality, you'd want more sophisticated order book reconstruction
            
            // Generate realistic spread based on exchange characteristics
            let base_spread_bps = match exchange {
                "binance" => 5.0,      // Tight spreads, high liquidity
                "coinbase" => 8.0,     // Slightly wider spreads
                "kraken" => 12.0,      // Wider spreads, lower liquidity
                "ftx" => 6.0,          // Good spreads, high liquidity
                _ => 10.0,             // Default spread
            };
            
            // Add market volatility factor
            let volatility_factor = 1.0 + (fastrand::f64() - 0.5) * 0.2; // ±10% spread variation
            let spread_bps = base_spread_bps * volatility_factor;
            let spread = mid_price * (spread_bps / 10000.0);
            
            // Generate order book levels
            for level in 1..=10 {
                let level_spread = spread * (level as f64 * 0.3 + 1.0); // Increasing spread with depth
                
                let bid_price = mid_price - level_spread;
                let ask_price = mid_price + level_spread;
                
                // Generate realistic quantities based on exchange liquidity
                let base_quantity = match exchange {
                    "binance" => 2.0 + fastrand::f64() * 8.0,    // 2-10 BTC per level
                    "coinbase" => 1.0 + fastrand::f64() * 4.0,   // 1-5 BTC per level
                    "kraken" => 0.5 + fastrand::f64() * 2.0,     // 0.5-2.5 BTC per level
                    "ftx" => 1.5 + fastrand::f64() * 6.0,        // 1.5-7.5 BTC per level
                    _ => 1.0 + fastrand::f64() * 3.0,            // Default liquidity
                };
                
                // Simulate order placement (this is a simplified approach)
                // In a real system, you'd reconstruct from historical L2 data
                let _ = orderbook.submit_limit_order(
                    format!("BID_{}_{}", level, timestamp.timestamp_millis()),
                    crate::orderbook::OrderSide::Buy,
                    BigDecimal::from(base_quantity),
                    Some(BigDecimal::from(bid_price)),
                    timestamp,
                );
                
                let _ = orderbook.submit_limit_order(
                    format!("ASK_{}_{}", level, timestamp.timestamp_millis()),
                    crate::orderbook::OrderSide::Sell,
                    BigDecimal::from(base_quantity),
                    Some(BigDecimal::from(ask_price)),
                    timestamp,
                );
            }
            
            Ok(())
        } else {
            Err(format!("Exchange {} not found", exchange))
        }
    }
    
    /// Execute arbitrage across multiple exchanges
    pub fn execute_arbitrage(
        &mut self,
        arbitrage_id: String,
        buy_exchange: String,
        sell_exchange: String,
        quantity: f64,
        max_buy_price: f64,
        min_sell_price: f64,
        timestamp: DateTime<Utc>,
    ) -> Result<CrossExchangeExecution, String> {
        let execution_start = std::time::Instant::now();
        
        // Simulate cross-exchange latency
        let latency_variation = (fastrand::f64() - 0.5) * 0.3; // ±30% latency variation
        let actual_latency = self.cross_exchange_latency_ms * (1.0 + latency_variation);
        
        // Execute buy leg
        let buy_execution = self.execute_single_leg(
            &buy_exchange,
            "buy",
            quantity,
            Some(max_buy_price),
            timestamp,
        )?;
        
        // Execute sell leg (with additional latency)
        let sell_timestamp = timestamp + chrono::Duration::milliseconds(actual_latency as i64);
        let sell_execution = self.execute_single_leg(
            &sell_exchange,
            "sell",
            quantity,
            Some(min_sell_price),
            sell_timestamp,
        )?;
        
        // Calculate profit/loss
        let buy_cost = buy_execution.quantity * buy_execution.price + buy_execution.fees;
        let sell_proceeds = sell_execution.quantity * sell_execution.price - sell_execution.fees;
        let total_pnl = sell_proceeds - buy_cost;
        
        let success = buy_execution.filled && sell_execution.filled;
        let failure_reason = if !success {
            Some(format!("Buy filled: {}, Sell filled: {}", buy_execution.filled, sell_execution.filled))
        } else {
            None
        };
        
        Ok(CrossExchangeExecution {
            arbitrage_id,
            buy_exchange: buy_exchange.clone(),
            sell_exchange: sell_exchange.clone(),
            buy_execution,
            sell_execution,
            total_profit_loss: total_pnl,
            execution_time_ms: execution_start.elapsed().as_millis() as f64,
            success,
            failure_reason,
        })
    }
    
    /// Execute single leg of arbitrage
    fn execute_single_leg(
        &mut self,
        exchange: &str,
        side: &str,
        quantity: f64,
        limit_price: Option<f64>,
        timestamp: DateTime<Utc>,
    ) -> Result<ExecutionLeg, String> {
        if let Some(orderbook) = self.orderbooks.get_mut(exchange) {
            let order_side = match side {
                "buy" => crate::orderbook::OrderSide::Buy,
                "sell" => crate::orderbook::OrderSide::Sell,
                _ => return Err(format!("Invalid order side: {}", side)),
            };
            
            // Submit order to orderbook
            let order_id = format!("ARB_{}_{}_{}_{}", 
                side.to_uppercase(), exchange, timestamp.timestamp_millis(), fastrand::u32(1000..9999));
            
            let result = if let Some(price) = limit_price {
                orderbook.submit_limit_order(
                    order_id.clone(),
                    order_side,
                    BigDecimal::from(quantity),
                    Some(BigDecimal::from(price)),
                    timestamp,
                )
            } else {
                orderbook.submit_market_order(
                    order_id.clone(),
                    order_side,
                    BigDecimal::from(quantity),
                    timestamp,
                )
            };
            
            match result {
                Ok(_) => {
                    // Simulate order matching
                    let fills = orderbook.match_all_orders(timestamp);
                    
                    // Find our fill
                    if let Some(fill) = fills.iter().find(|f| f.order_id == order_id) {
                        let exchange_config = self.exchange_configs.get(exchange).unwrap();
                        let is_maker = limit_price.is_some(); // Simplified maker/taker determination
                        
                        let fee_rate = if is_maker { 
                            exchange_config.maker_fee 
                        } else { 
                            exchange_config.taker_fee 
                        };
                        
                        let fees = fill.quantity.to_f64().unwrap_or(0.0) * 
                                   fill.price.to_f64().unwrap_or(0.0) * fee_rate.abs();
                        
                        // Simulate latency based on exchange characteristics
                        let latency = match exchange {
                            "binance" => 15.0 + fastrand::f64() * 10.0,  // 15-25ms
                            "coinbase" => 25.0 + fastrand::f64() * 15.0, // 25-40ms
                            "kraken" => 40.0 + fastrand::f64() * 20.0,   // 40-60ms
                            "ftx" => 20.0 + fastrand::f64() * 10.0,      // 20-30ms
                            _ => 30.0 + fastrand::f64() * 20.0,          // 30-50ms default
                        };
                        
                        Ok(ExecutionLeg {
                            exchange: exchange.to_string(),
                            side: side.to_string(),
                            quantity: fill.quantity.to_f64().unwrap_or(0.0),
                            price: fill.price.to_f64().unwrap_or(0.0),
                            fees,
                            slippage_bps: 0.0, // Calculate from expected vs actual price
                            latency_ms: latency,
                            filled: true,
                        })
                    } else {
                        Ok(ExecutionLeg {
                            exchange: exchange.to_string(),
                            side: side.to_string(),
                            quantity: 0.0,
                            price: limit_price.unwrap_or(0.0),
                            fees: 0.0,
                            slippage_bps: 0.0,
                            latency_ms: 100.0,
                            filled: false,
                        })
                    }
                }
                Err(e) => Err(format!("Order submission failed: {}", e)),
            }
        } else {
            Err(format!("Exchange {} not found", exchange))
        }
    }
    
    /// Get current market snapshot across all exchanges
    pub fn get_market_snapshot(&self, symbol: &str) -> MultiExchangeSnapshot {
        let mut exchanges = HashMap::new();
        
        for (exchange_name, orderbook) in &self.orderbooks {
            // Get current best bid/ask from orderbook
            // This is a simplified version - real implementation would be more complex
            let (best_bid, bid_size) = orderbook.get_best_bid()
                .map(|o| (o.price.to_f64().unwrap_or(0.0), o.quantity.to_f64().unwrap_or(0.0)))
                .unwrap_or((0.0, 0.0));
                
            let (best_ask, ask_size) = orderbook.get_best_ask()
                .map(|o| (o.price.to_f64().unwrap_or(0.0), o.quantity.to_f64().unwrap_or(0.0)))
                .unwrap_or((0.0, 0.0));
            
            let mid_price = if best_bid > 0.0 && best_ask > 0.0 {
                (best_bid + best_ask) / 2.0
            } else {
                0.0
            };
            
            let spread_bps = if mid_price > 0.0 {
                ((best_ask - best_bid) / mid_price) * 10000.0
            } else {
                0.0
            };
            
            exchanges.insert(exchange_name.clone(), ExchangeSnapshot {
                exchange: exchange_name.clone(),
                best_bid,
                best_ask,
                bid_size,
                ask_size,
                mid_price,
                spread_bps,
                last_update: *self.last_sync_time.get(exchange_name).unwrap_or(&Utc::now()),
            });
        }
        
        MultiExchangeSnapshot {
            timestamp: Utc::now(),
            symbol: symbol.to_string(),
            exchanges,
        }
    }
    
    /// Get exchange-specific fee configuration
    fn get_exchange_fee_config(exchange: &str) -> ExchangeFeeConfig {
        match exchange.to_lowercase().as_str() {
            "binance" => ExchangeFeeConfig {
                exchange: "binance".to_string(),
                maker_fee: 0.001,   // 10 bps
                taker_fee: 0.001,   // 10 bps
                volume_tiers: vec![], // Simplified - no volume tiers
            },
            "coinbase" => ExchangeFeeConfig {
                exchange: "coinbase".to_string(),
                maker_fee: 0.005,   // 50 bps
                taker_fee: 0.005,   // 50 bps
                volume_tiers: vec![],
            },
            "kraken" => ExchangeFeeConfig {
                exchange: "kraken".to_string(),
                maker_fee: 0.0016,  // 16 bps
                taker_fee: 0.0026,  // 26 bps
                volume_tiers: vec![],
            },
            "ftx" => ExchangeFeeConfig {
                exchange: "ftx".to_string(),
                maker_fee: 0.0002,  // 2 bps
                taker_fee: 0.0007,  // 7 bps
                volume_tiers: vec![],
            },
            _ => ExchangeFeeConfig {
                exchange: exchange.to_string(),
                maker_fee: 0.002,   // 20 bps default
                taker_fee: 0.002,   // 20 bps default
                volume_tiers: vec![],
            },
        }
    }
    
    /// Check if exchanges are synchronized within acceptable time window
    pub fn check_synchronization(&self, max_lag_ms: i64) -> HashMap<String, bool> {
        let current_time = Utc::now();
        let mut sync_status = HashMap::new();
        
        for (exchange, last_sync) in &self.last_sync_time {
            let lag = (current_time - *last_sync).num_milliseconds();
            sync_status.insert(exchange.clone(), lag <= max_lag_ms);
        }
        
        sync_status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    
    #[test]
    fn test_multi_exchange_orderbooks_creation() {
        let config = BacktestConfig {
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                database: "test".to_string(),
                username: "test".to_string(),
                password: "test".to_string(),
                max_connections: None,
                connection_timeout: None,
            },
            trading: TradingConfig {
                commission_rate: 0.001,
                slippage_bps: 10.0,
                initial_capital: 100000.0,
                max_position_size: Some(0.1),
                risk_free_rate: 0.02,
                exchange_fees: None,
            },
            data: DataConfig {
                default_timeframe: "1h".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: false,
                cache_duration_hours: 24,
            },
            output: OutputConfig {
                results_dir: "results".to_string(),
                reports_dir: "reports".to_string(),
                charts_enabled: false,
                export_formats: vec!["json".to_string()],
            },
            analysis: AnalysisConfig {
                monte_carlo: MonteCarloConfig {
                    enabled: false,
                    num_simulations: 1000,
                    confidence_level: 0.95,
                },
                walk_forward: None,
            },
        };
        
        let exchanges = vec![
            "binance".to_string(),
            "coinbase".to_string(),
            "kraken".to_string(),
        ];
        
        let multi_ob = MultiExchangeOrderBooks::new(&config, exchanges);
        assert_eq!(multi_ob.orderbooks.len(), 3);
        assert_eq!(multi_ob.exchange_configs.len(), 3);
    }
}
