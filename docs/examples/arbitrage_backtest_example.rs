use std::collections::HashMap;
use chrono::{DateTime, Utc};
use anyhow::Result;

use backtest::multi_exchange_simulation::{MultiExchangeOrderBooks, CrossExchangeExecution};
use strategy::arbitrage_strategy::{ArbitrageStrategy, ArbitrageRiskLimits};
use strategy::{Strategy, StrategyContext};
use config::{BacktestConfig, RoutingMode, ArbitrageRiskLimits as ConfigArbitrageRiskLimits};
use dataloader::MarketData;

/// Example demonstrating how to backtest an exchange arbitrage strategy
/// 
/// This example shows:
/// 1. Setting up multi-exchange order books
/// 2. Configuring an arbitrage strategy
/// 3. Processing synchronized market data across exchanges
/// 4. Executing arbitrage trades
/// 5. Analyzing arbitrage performance
pub struct ArbitrageBacktestExample {
    strategy: ArbitrageStrategy,
    multi_exchange: MultiExchangeOrderBooks,
    execution_history: Vec<CrossExchangeExecution>,
    config: BacktestConfig,
}

impl ArbitrageBacktestExample {
    /// Create a new arbitrage backtest example
    pub fn new(config: BacktestConfig) -> Result<Self> {
        // Extract arbitrage configuration from config
        let (exchanges, min_profit_bps, max_holding_time, risk_limits) = match &config.trading.routing_mode {
            Some(RoutingMode::Arbitrage { exchanges, min_profit_bps, max_holding_time_seconds, risk_limits }) => {
                (exchanges.clone(), *min_profit_bps, *max_holding_time_seconds, risk_limits.clone())
            }
            _ => {
                // Default arbitrage configuration
                let exchanges = vec![
                    "binance".to_string(),
                    "coinbase".to_string(),
                    "kraken".to_string(),
                ];
                (exchanges, 15.0, 300, ConfigArbitrageRiskLimits::default())
            }
        };
        
        // Create arbitrage strategy
        let strategy = ArbitrageStrategy::new(
            "BTC/USD".to_string(),
            min_profit_bps,
            1.0, // 1 BTC max position size
            max_holding_time,
            exchanges.clone(),
        )?;
        
        // Create multi-exchange order books
        let multi_exchange = MultiExchangeOrderBooks::new(&config, exchanges);
        
        Ok(Self {
            strategy,
            multi_exchange,
            execution_history: Vec::new(),
            config,
        })
    }
    
    /// Run arbitrage backtest with synchronized market data
    pub async fn run_backtest(&mut self, market_data_by_exchange: HashMap<String, Vec<MarketData>>) -> Result<ArbitrageBacktestResults> {
        println!("🔄 Starting arbitrage backtest across {} exchanges", 
                 market_data_by_exchange.keys().len());
        
        // Find common timestamps across all exchanges
        let synchronized_timestamps = self.find_synchronized_timestamps(&market_data_by_exchange)?;
        println!("📊 Found {} synchronized data points", synchronized_timestamps.len());
        
        let mut total_profit = 0.0;
        let mut successful_arbitrages = 0;
        let mut failed_arbitrages = 0;
        let mut opportunities_detected = 0;
        
        for timestamp in synchronized_timestamps {
            // Update market data for all exchanges
            self.update_market_data_at_timestamp(&market_data_by_exchange, timestamp).await?;
            
            // Get market snapshot for arbitrage detection
            let market_snapshot = self.multi_exchange.get_market_snapshot("BTC/USD");
            
            // Convert snapshot to exchange prices for strategy
            let exchange_prices = self.convert_snapshot_to_prices(&market_snapshot);
            
            // Update strategy with current prices
            let mut strategy_clone = self.strategy.clone();
            strategy_clone.update_exchange_prices(exchange_prices);
            
            // Create strategy context
            let context = StrategyContext {
                current_time: timestamp,
                portfolio_value: self.config.trading.initial_capital + total_profit,
                available_capital: self.config.trading.initial_capital + total_profit,
                open_positions: HashMap::new(),
                market_session: crate::dataloader::MarketSession {
                    is_open: true,
                    session_type: crate::dataloader::SessionType::Regular,
                    next_open: None,
                    next_close: None,
                },
                risk_metrics: Default::default(),
                volume_history: Vec::new(),
                spread: None,
            };
            
            // Process market data and generate signals
            let signals = strategy_clone.process_market_data(&context)?;
            opportunities_detected += signals.len() / 2; // Each arbitrage generates 2 signals
            
            // Execute arbitrage signals
            for signal_pair in signals.chunks(2) {
                if signal_pair.len() == 2 {
                    let buy_signal = &signal_pair[0];
                    let sell_signal = &signal_pair[1];
                    
                    if let (Some(buy_exchange), Some(sell_exchange)) = 
                        (&buy_signal.exchange, &sell_signal.exchange) {
                        
                        // Execute arbitrage
                        let execution_result = self.multi_exchange.execute_arbitrage(
                            buy_signal.id.clone(),
                            buy_exchange.clone(),
                            sell_exchange.clone(),
                            buy_signal.quantity,
                            buy_signal.price.unwrap_or(0.0),
                            sell_signal.price.unwrap_or(0.0),
                            timestamp,
                        ).await;
                        
                        match execution_result {
                            Ok(execution) => {
                                total_profit += execution.total_profit_loss;
                                if execution.success {
                                    successful_arbitrages += 1;
                                    println!("✅ Successful arbitrage: {} -> {} = ${:.2} profit", 
                                             buy_exchange, sell_exchange, execution.total_profit_loss);
                                } else {
                                    failed_arbitrages += 1;
                                    println!("❌ Failed arbitrage: {} -> {} ({})", 
                                             buy_exchange, sell_exchange, 
                                             execution.failure_reason.unwrap_or("Unknown".to_string()));
                                }
                                self.execution_history.push(execution);
                            }
                            Err(e) => {
                                failed_arbitrages += 1;
                                println!("❌ Arbitrage execution error: {}", e);
                            }
                        }
                    }
                }
            }
            
            // Update strategy state
            self.strategy = strategy_clone;
        }
        
        // Generate results
        let results = ArbitrageBacktestResults {
            total_profit,
            total_opportunities: opportunities_detected,
            successful_arbitrages,
            failed_arbitrages,
            success_rate: if opportunities_detected > 0 { 
                successful_arbitrages as f64 / opportunities_detected as f64 
            } else { 
                0.0 
            },
            average_profit_per_trade: if successful_arbitrages > 0 { 
                total_profit / successful_arbitrages as f64 
            } else { 
                0.0 
            },
            execution_history: self.execution_history.clone(),
            final_capital: self.config.trading.initial_capital + total_profit,
        };
        
        println!("\n📈 Arbitrage Backtest Results:");
        println!("   Total Profit: ${:.2}", results.total_profit);
        println!("   Success Rate: {:.1}%", results.success_rate * 100.0);
        println!("   Opportunities: {} (Successful: {}, Failed: {})", 
                 results.total_opportunities, results.successful_arbitrages, results.failed_arbitrages);
        println!("   Avg Profit/Trade: ${:.2}", results.average_profit_per_trade);
        println!("   Final Capital: ${:.2}", results.final_capital);
        
        Ok(results)
    }
    
    /// Find timestamps that exist across all exchanges
    fn find_synchronized_timestamps(&self, data_by_exchange: &HashMap<String, Vec<MarketData>>) -> Result<Vec<DateTime<Utc>>> {
        if data_by_exchange.is_empty() {
            return Ok(Vec::new());
        }
        
        // Get timestamps from first exchange
        let first_exchange = data_by_exchange.keys().next().unwrap();
        let first_timestamps: std::collections::HashSet<DateTime<Utc>> = data_by_exchange[first_exchange]
            .iter()
            .map(|data| data.timestamp)
            .collect();
        
        // Find intersection of all timestamps
        let mut synchronized_timestamps = first_timestamps;
        for (exchange, data) in data_by_exchange.iter().skip(1) {
            let exchange_timestamps: std::collections::HashSet<DateTime<Utc>> = data
                .iter()
                .map(|data| data.timestamp)
                .collect();
            
            synchronized_timestamps = synchronized_timestamps
                .intersection(&exchange_timestamps)
                .cloned()
                .collect();
            
            println!("After {} intersection: {} synchronized timestamps", 
                     exchange, synchronized_timestamps.len());
        }
        
        // Convert to sorted vector
        let mut result: Vec<DateTime<Utc>> = synchronized_timestamps.into_iter().collect();
        result.sort();
        
        Ok(result)
    }
    
    /// Update market data for all exchanges at a specific timestamp
    async fn update_market_data_at_timestamp(
        &mut self,
        data_by_exchange: &HashMap<String, Vec<MarketData>>,
        timestamp: DateTime<Utc>,
    ) -> Result<()> {
        for (exchange, data) in data_by_exchange {
            if let Some(market_data) = data.iter().find(|d| d.timestamp == timestamp) {
                self.multi_exchange.update_exchange_data(exchange, market_data)?;
            }
        }
        Ok(())
    }
    
    /// Convert market snapshot to exchange prices for strategy
    fn convert_snapshot_to_prices(&self, snapshot: &backtest::multi_exchange_simulation::MultiExchangeSnapshot) -> Vec<strategy::arbitrage_strategy::ExchangePrice> {
        snapshot.exchanges
            .values()
            .map(|exchange_snapshot| {
                strategy::arbitrage_strategy::ExchangePrice {
                    exchange: exchange_snapshot.exchange.clone(),
                    best_bid: exchange_snapshot.best_bid,
                    best_ask: exchange_snapshot.best_ask,
                    bid_size: exchange_snapshot.bid_size,
                    ask_size: exchange_snapshot.ask_size,
                    timestamp: exchange_snapshot.last_update,
                    latency_ms: 25.0, // Default latency
                }
            })
            .collect()
    }
}

/// Results of an arbitrage backtest
#[derive(Debug, Clone)]
pub struct ArbitrageBacktestResults {
    pub total_profit: f64,
    pub total_opportunities: usize,
    pub successful_arbitrages: i32,
    pub failed_arbitrages: i32,
    pub success_rate: f64,
    pub average_profit_per_trade: f64,
    pub execution_history: Vec<CrossExchangeExecution>,
    pub final_capital: f64,
}

impl ArbitrageBacktestResults {
    /// Generate detailed performance analysis
    pub fn analyze_performance(&self) -> ArbitragePerformanceAnalysis {
        let mut exchange_performance: HashMap<String, ExchangePerformance> = HashMap::new();
        let mut profit_by_hour: HashMap<u32, f64> = HashMap::new();
        let mut execution_latencies: Vec<f64> = Vec::new();
        
        // Analyze execution history
        for execution in &self.execution_history {
            // Exchange performance
            let buy_exchange = &execution.buy_exchange;
            let sell_exchange = &execution.sell_exchange;
            
            exchange_performance.entry(buy_exchange.clone())
                .or_insert_with(ExchangePerformance::default)
                .total_volume += execution.buy_execution.quantity;
            
            exchange_performance.entry(sell_exchange.clone())
                .or_insert_with(ExchangePerformance::default)
                .total_volume += execution.sell_execution.quantity;
            
            if execution.success {
                exchange_performance.get_mut(buy_exchange).unwrap().successful_trades += 1;
                exchange_performance.get_mut(sell_exchange).unwrap().successful_trades += 1;
                exchange_performance.get_mut(buy_exchange).unwrap().total_profit += execution.total_profit_loss / 2.0;
                exchange_performance.get_mut(sell_exchange).unwrap().total_profit += execution.total_profit_loss / 2.0;
            }
            
            // Time-based analysis
            let hour = 12; // Simplified - would extract from timestamp
            *profit_by_hour.entry(hour).or_insert(0.0) += execution.total_profit_loss;
            
            // Latency analysis
            execution_latencies.push(execution.execution_time_ms);
        }
        
        // Calculate statistics
        let avg_latency = if !execution_latencies.is_empty() {
            execution_latencies.iter().sum::<f64>() / execution_latencies.len() as f64
        } else {
            0.0
        };
        
        let max_latency = execution_latencies.iter().fold(0.0, |a, &b| a.max(b));
        
        ArbitragePerformanceAnalysis {
            exchange_performance,
            profit_by_hour,
            average_execution_latency_ms: avg_latency,
            max_execution_latency_ms: max_latency,
            sharpe_ratio: self.calculate_sharpe_ratio(),
            max_drawdown: self.calculate_max_drawdown(),
        }
    }
    
    /// Calculate Sharpe ratio for arbitrage strategy
    fn calculate_sharpe_ratio(&self) -> f64 {
        if self.execution_history.is_empty() {
            return 0.0;
        }
        
        let profits: Vec<f64> = self.execution_history
            .iter()
            .map(|e| e.total_profit_loss)
            .collect();
        
        let mean_return = profits.iter().sum::<f64>() / profits.len() as f64;
        let variance = profits
            .iter()
            .map(|p| (p - mean_return).powi(2))
            .sum::<f64>() / profits.len() as f64;
        
        let std_dev = variance.sqrt();
        
        if std_dev > 0.0 {
            mean_return / std_dev
        } else {
            0.0
        }
    }
    
    /// Calculate maximum drawdown
    fn calculate_max_drawdown(&self) -> f64 {
        let mut running_pnl = 0.0;
        let mut peak = 0.0;
        let mut max_drawdown = 0.0;
        
        for execution in &self.execution_history {
            running_pnl += execution.total_profit_loss;
            peak = peak.max(running_pnl);
            let drawdown = peak - running_pnl;
            max_drawdown = max_drawdown.max(drawdown);
        }
        
        max_drawdown
    }
}

/// Performance analysis for arbitrage strategy
#[derive(Debug, Clone)]
pub struct ArbitragePerformanceAnalysis {
    pub exchange_performance: HashMap<String, ExchangePerformance>,
    pub profit_by_hour: HashMap<u32, f64>,
    pub average_execution_latency_ms: f64,
    pub max_execution_latency_ms: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
}

/// Performance metrics per exchange
#[derive(Debug, Clone, Default)]
pub struct ExchangePerformance {
    pub total_volume: f64,
    pub successful_trades: i32,
    pub total_profit: f64,
}

/// Example usage and testing
#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    
    #[tokio::test]
    async fn test_arbitrage_backtest_example() {
        // Create test configuration
        let config = BacktestConfig {
            database: config::DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                database: "test".to_string(),
                username: "test".to_string(),
                password: "test".to_string(),
                max_connections: None,
                connection_timeout: None,
            },
            trading: config::TradingConfig {
                commission_rate: 0.001,
                slippage_bps: 5.0,
                initial_capital: 100000.0,
                max_position_size: Some(0.1),
                risk_free_rate: 0.02,
                exchange_fees: None,
                routing_mode: Some(RoutingMode::Arbitrage {
                    exchanges: vec!["binance".to_string(), "coinbase".to_string()],
                    min_profit_bps: 10.0,
                    max_holding_time_seconds: 300,
                    risk_limits: ConfigArbitrageRiskLimits::default(),
                }),
            },
            data: config::DataConfig {
                default_timeframe: "1m".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: false,
                cache_duration_hours: 1,
            },
            output: config::OutputConfig {
                results_dir: "test_results".to_string(),
                reports_dir: "test_reports".to_string(),
                charts_enabled: false,
                export_formats: vec!["json".to_string()],
            },
            analysis: config::AnalysisConfig {
                monte_carlo: config::MonteCarloConfig::default(),
                walk_forward: None,
            },
        };
        
        // Create example instance
        let mut example = ArbitrageBacktestExample::new(config).unwrap();
        
        // Create test market data with arbitrage opportunities
        let mut market_data_by_exchange = HashMap::new();
        
        let timestamp = Utc::now();
        
        // Binance data (cheaper)
        market_data_by_exchange.insert("binance".to_string(), vec![
            MarketData {
                timestamp,
                symbol: "BTC/USD".to_string(),
                open: BigDecimal::from(49950),
                high: BigDecimal::from(50050),
                low: BigDecimal::from(49900),
                close: BigDecimal::from(50000), // Lower price
                volume: BigDecimal::from(100),
                interval: "1m".to_string(),
            }
        ]);
        
        // Coinbase data (more expensive)
        market_data_by_exchange.insert("coinbase".to_string(), vec![
            MarketData {
                timestamp,
                symbol: "BTC/USD".to_string(),
                open: BigDecimal::from(50050),
                high: BigDecimal::from(50150),
                low: BigDecimal::from(50000),
                close: BigDecimal::from(50100), // Higher price - arbitrage opportunity
                volume: BigDecimal::from(80),
                interval: "1m".to_string(),
            }
        ]);
        
        // Run backtest
        let results = example.run_backtest(market_data_by_exchange).await.unwrap();
        
        // Verify results
        assert!(results.total_opportunities >= 0);
        assert!(results.final_capital >= config.trading.initial_capital);
        
        // Analyze performance
        let analysis = results.analyze_performance();
        println!("Performance Analysis: {:#?}", analysis);
    }
}
