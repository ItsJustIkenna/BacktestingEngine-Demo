//! Distributed Worker for processing backtest chunks
//!
//! This worker subscribes to chunk messages and processes them independently,
//! publishing results back to the broker.

use std::sync::Arc;
use std::time::Instant;
use std::collections::HashMap;
use tokio::sync::broadcast;
use chrono::{DateTime, Utc};
use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};

use protocol::broker::messages::{BacktestChunk, BacktestChunkResult};

use super::broker_client::{BacktestBrokerClient, get_broker_client};
use crate::job_queue::BacktestJobParams;
use crate::{log_info_structured, log_warn_structured, log_error_structured};
use crate::logging_facade::{MAIN_LOGGER, BACKTEST_LOGGER};

// Import backtest infrastructure
use backtest::BacktestEngine;
use strategy::{StrategyManager, StrategyConfig, StrategyRiskLimits};
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use riskmanager::RiskManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use config::BacktestConfig;
use dataloader::MarketData;

/// Distributed worker that processes backtest chunks
pub struct DistributedWorker {
    /// Unique worker ID
    worker_id: String,
    /// Broker client for pub/sub
    broker: Arc<BacktestBrokerClient>,
    /// Database URL for loading data
    db_url: String,
    /// Shutdown flag
    shutdown: bool,
}

impl DistributedWorker {
    /// Create a new distributed worker
    pub fn new(worker_id: impl Into<String>, db_url: impl Into<String>) -> Self {
        Self {
            worker_id: worker_id.into(),
            broker: get_broker_client(),
            db_url: db_url.into(),
            shutdown: false,
        }
    }
    
    /// Create with custom broker client
    pub fn with_broker(worker_id: impl Into<String>, db_url: impl Into<String>, broker: Arc<BacktestBrokerClient>) -> Self {
        Self {
            worker_id: worker_id.into(),
            broker,
            db_url: db_url.into(),
            shutdown: false,
        }
    }
    
    /// Run the worker (processes chunks until shutdown)
    pub async fn run(&mut self) {
        log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_WORKER_START",
            "event" => "DISTRIBUTED_WORKER_START",
            "worker_id" => &self.worker_id
        );
        
        let mut chunk_rx = self.broker.subscribe_all_chunks();
        
        loop {
            if self.shutdown {
                log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_WORKER_SHUTDOWN",
                    "event" => "DISTRIBUTED_WORKER_SHUTDOWN",
                    "worker_id" => &self.worker_id
                );
                break;
            }
            
            // Wait for a chunk
            match chunk_rx.recv().await {
                Ok(chunk) => {
                    log_info_structured!(BACKTEST_LOGGER, "CHUNK_RECEIVED",
                        "event" => "CHUNK_RECEIVED",
                        "worker_id" => &self.worker_id,
                        "chunk_id" => chunk.chunk_id,
                        "total_chunks" => chunk.total_chunks,
                        "job_id" => &chunk.job_id
                    );
                    
                    // Process the chunk
                    let result = self.process_chunk(chunk).await;
                    
                    // Publish the result
                    if let Err(e) = self.broker.publish_result(result).await {
                        log_error_structured!(MAIN_LOGGER, "WORKER_PUBLISH_RESULT_FAILED",
                            "event" => "WORKER_PUBLISH_RESULT_FAILED",
                            "worker_id" => &self.worker_id,
                            "error" => e.to_string()
                        );
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log_warn_structured!(MAIN_LOGGER, "WORKER_LAGGED",
                        "event" => "WORKER_LAGGED",
                        "worker_id" => &self.worker_id,
                        "lagged_messages" => n
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    log_info_structured!(MAIN_LOGGER, "DISTRIBUTED_WORKER_CHANNEL_CLOSED",
                        "event" => "DISTRIBUTED_WORKER_CHANNEL_CLOSED",
                        "worker_id" => &self.worker_id
                    );
                    break;
                }
            }
        }
    }
    
    /// Process a single chunk
    async fn process_chunk(&self, chunk: BacktestChunk) -> BacktestChunkResult {
        let start_time = Instant::now();
        
        // Parse timestamps
        let start_dt = match chunk.start_time.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(e) => {
                return self.error_result(&chunk, format!("Invalid start_time: {}", e));
            }
        };
        
        let end_dt = match chunk.end_time.parse::<DateTime<Utc>>() {
            Ok(dt) => dt,
            Err(e) => {
                return self.error_result(&chunk, format!("Invalid end_time: {}", e));
            }
        };
        
        // Parse strategy config
        let params: BacktestJobParams = match serde_json::from_slice(&chunk.strategy_config) {
            Ok(p) => p,
            Err(e) => {
                return self.error_result(&chunk, format!("Invalid strategy_config: {}", e));
            }
        };
        
        log_info_structured!(BACKTEST_LOGGER, "CHUNK_PROCESSING_START",
            "event" => "CHUNK_PROCESSING_START",
            "chunk_id" => chunk.chunk_id,
            "start_time" => start_dt.to_string(),
            "end_time" => end_dt.to_string(),
            "symbol" => &chunk.symbol,
            "exchange" => &chunk.exchange
        );
        
        // Load data for this chunk only (memory efficient!)
        let market_data = match self.load_chunk_data(
            &chunk.symbol, 
            &chunk.exchange, 
            &start_dt, 
            &end_dt
        ).await {
            Ok(data) => data,
            Err(e) => {
                return self.error_result(&chunk, format!("Failed to load data: {}", e));
            }
        };
        
        let events_processed = market_data.len() as i64;
        log_info_structured!(BACKTEST_LOGGER, "CHUNK_DATA_LOADED",
            "event" => "CHUNK_DATA_LOADED",
            "event_count" => events_processed,
            "chunk_id" => chunk.chunk_id
        );
        
        if market_data.is_empty() {
            return self.error_result(&chunk, "No market data for this time range".to_string());
        }
        
        // Create configuration using the same pattern as existing worker
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = chunk.initial_capital;
        backtest_config.trading.commission_rate = params.commission_rate;
        
        // Create engine
        let engine = BacktestEngine::new(backtest_config);
        
        // Get initial price for inventory calculation
        let initial_price = Self::get_reference_price(&market_data);
        
        // Initialize portfolio with starting state from previous chunk or fresh
        let (quote_balance, base_quantity) = if chunk.chunk_id == 0 {
            // First chunk: split capital 50/50
            let inventory_pct = 0.5;
            let quote = chunk.initial_capital * (1.0 - inventory_pct);
            let base_value = chunk.initial_capital * inventory_pct;
            let base = if initial_price > 0.0 { base_value / initial_price } else { 0.0 };
            (quote, base)
        } else {
            // Continuation chunk: use ending state from previous
            (chunk.starting_cash, chunk.starting_position)
        };
        
        let first_timestamp = Self::get_first_timestamp(&market_data);
        let mut portfolio = PortfolioState::with_inventory(
            chunk.symbol.clone(),
            BigDecimal::from_f64(quote_balance).unwrap_or_default(),
            base_quantity,
            BigDecimal::from_f64(initial_price).unwrap_or_default(),
            first_timestamp,
        );
        
        // Create orderbook
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(chunk.symbol.clone(), chunk.exchange.clone(), orderbook_config);
        
        // Create risk manager
        let mut risk_manager = RiskManager::new();
        
        // Create strategy parameters
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: params.risk_aversion,
            inventory_target: params.inventory_target,
            order_size: params.order_size,
            order_size_pct: params.order_size_pct,
            initial_capital: params.initial_capital,
            window_ms: params.window_ms,
            min_quote_lifetime_ms: params.min_quote_lifetime_ms,
            volatility_cap: params.volatility_cap,
            time_horizon_minutes: params.time_horizon_minutes as f64,
            market_depth_k: params.market_depth_k,
            volatility_adjustment_factor: params.volatility_adjustment_factor,
            inventory_penalty_factor: params.inventory_penalty_factor,
            trend_sensitivity: params.trend_sensitivity,
            flow_sensitivity: params.flow_sensitivity,
            take_profit_pct: params.take_profit_pct,
            stop_loss_pct: params.stop_loss_pct,
            min_spread_bps: params.min_spread_bps,
            max_trades_per_minute: params.max_trades_per_minute,
            ..Default::default()
        };
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        
        // Create strategy config
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
        strategy_params_map.insert("order_size_pct".to_string(), config::ParameterValue::Float(params.order_size_pct));
        strategy_params_map.insert("initial_capital".to_string(), config::ParameterValue::Float(params.initial_capital));
        
        let strategy_config = StrategyConfig {
            id: format!("chunk-{}-{}", chunk.job_id, chunk.chunk_id),
            name: "Market Making".to_string(),
            strategy_type: "MarketMaking".to_string(),
            symbol: chunk.symbol.clone(),
            exchange: chunk.exchange.clone(),
            parameters: strategy_params_map,
            risk_limits: StrategyRiskLimits::default(),
            enabled: true,
        };
        
        let mut strategy_manager = StrategyManager::new();
        if let Err(e) = strategy_manager.register_strategy(strategy_config, strategy) {
            return self.error_result(&chunk, format!("Failed to register strategy: {}", e));
        }
        
        // Run backtest
        let result = match engine
            .run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &market_data,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return self.error_result(&chunk, format!("Simulation failed: {}", e));
            }
        };
        
        let processing_duration = start_time.elapsed().as_millis() as i64;
        
        // Extract metrics from result
        let net_pnl = result.core.total_pnl.to_f64().unwrap_or(0.0);
        let max_drawdown = result.core.max_drawdown.to_f64().unwrap_or(0.0);
        let num_trades = result.core.num_trades as i32;
        
        // Use gross_profit/loss directly if available, else estimate from win_rate
        let gross_profit = result.core.gross_profit.unwrap_or_else(|| {
            let win_rate = result.core.win_rate.unwrap_or(0.5);
            net_pnl.abs() * win_rate
        });
        let gross_loss = result.core.gross_loss.unwrap_or_else(|| {
            let win_rate = result.core.win_rate.unwrap_or(0.5);
            net_pnl.abs() * (1.0 - win_rate)
        });
        
        // Calculate winning trades from win_rate
        let win_rate = result.core.win_rate.unwrap_or(0.0);
        let winning_trades = (result.core.num_trades as f64 * win_rate).round() as i32;
        
        // Transaction costs from analysis
        let transaction_costs = result.core.transaction_costs
            .as_ref()
            .map(|tc| tc.total_transaction_costs)
            .unwrap_or(0.0);
        
        // Serialize trade returns for aggregation (use equity_curve as proxy for daily returns)
        let daily_returns_json = serde_json::to_vec(&result.core.equity_curve)
            .unwrap_or_default();
        
        // Get ending state for next chunk - use public fields
        let ending_position = portfolio.net_position;
        let ending_cash = portfolio.balance.to_f64().unwrap_or(chunk.starting_cash);
        
        log_info_structured!(BACKTEST_LOGGER, "CHUNK_COMPLETE",
            "event" => "CHUNK_COMPLETE",
            "chunk_id" => chunk.chunk_id,
            "event_count" => events_processed,
            "trade_count" => num_trades,
            "net_pnl" => format!("{:.2}", net_pnl),
            "processing_ms" => processing_duration
        );
        
        BacktestChunkResult {
            job_id: chunk.job_id,
            chunk_id: chunk.chunk_id,
            worker_id: self.worker_id.clone(),
            success: true,
            error_message: String::new(),
            net_pnl,
            gross_profit,
            gross_loss,
            num_trades,
            winning_trades,
            max_drawdown,
            transaction_costs,
            ending_position,
            ending_cash,
            daily_returns: daily_returns_json,
            events_processed,
            processing_duration_ms: processing_duration,
            peak_memory_bytes: 0, // TODO: implement memory tracking
            monte_carlo_run: chunk.monte_carlo_run,
        }
    }
    
    /// Create error result
    fn error_result(&self, chunk: &BacktestChunk, error: String) -> BacktestChunkResult {
        log_error_structured!(BACKTEST_LOGGER, "CHUNK_FAILED",
            "event" => "CHUNK_FAILED",
            "chunk_id" => chunk.chunk_id,
            "error" => &error
        );
        
        BacktestChunkResult {
            job_id: chunk.job_id.clone(),
            chunk_id: chunk.chunk_id,
            worker_id: self.worker_id.clone(),
            success: false,
            error_message: error,
            net_pnl: 0.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
            num_trades: 0,
            winning_trades: 0,
            max_drawdown: 0.0,
            transaction_costs: 0.0,
            ending_position: chunk.starting_position,
            ending_cash: chunk.starting_cash,
            daily_returns: vec![],
            events_processed: 0,
            processing_duration_ms: 0,
            peak_memory_bytes: 0,
            monte_carlo_run: chunk.monte_carlo_run,
        }
    }
    
    /// Get reference price from market data
    fn get_reference_price(market_data: &[MarketData]) -> f64 {
        market_data.first().map(|data| {
            match data {
                #[cfg(feature = "postgres")]
                MarketData::Order(o) => o.price_level.to_f64().unwrap_or(0.0),
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.price_level.to_f64().unwrap_or(0.0),
                MarketData::Candle(c) => c.close.to_f64().unwrap_or(0.0),
                MarketData::Trade(t) => t.price.to_f64().unwrap_or(0.0),
            }
        }).unwrap_or(0.0)
    }
    
    /// Get first timestamp from market data
    fn get_first_timestamp(market_data: &[MarketData]) -> DateTime<Utc> {
        market_data.first().map(|data| {
            match data {
                #[cfg(feature = "postgres")]
                MarketData::Order(o) => o.timestamp,
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            }
        }).unwrap_or_else(Utc::now)
    }
    
    /// Load market data for a chunk using streaming
    #[cfg(feature = "postgres")]
    async fn load_chunk_data(
        &self,
        symbol: &str,
        exchange: &str,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use crate::streaming::StreamingDataLoader;
        
        let loader = StreamingDataLoader::new(&self.db_url).await?;
        loader.load_events_with_limit(symbol, exchange, start, end, 500_000).await
    }
    
    /// Fallback for non-postgres builds
    #[cfg(not(feature = "postgres"))]
    async fn load_chunk_data(
        &self,
        _symbol: &str,
        _exchange: &str,
        _start: &DateTime<Utc>,
        _end: &DateTime<Utc>,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        Err("Postgres feature not enabled".into())
    }
    
    /// Shutdown the worker
    pub fn shutdown(&mut self) {
        self.shutdown = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    
    #[tokio::test]
    async fn test_worker_creation() {
        let broker = Arc::new(BacktestBrokerClient::in_process());
        let worker = DistributedWorker::with_broker("test-worker-1", "postgres://localhost/test", broker);
        assert_eq!(worker.worker_id, "test-worker-1");
        assert!(!worker.shutdown);
    }
    
    #[tokio::test]
    async fn test_worker_shutdown() {
        let broker = Arc::new(BacktestBrokerClient::in_process());
        let mut worker = DistributedWorker::with_broker("test-worker", "postgres://localhost/test", broker);
        assert!(!worker.shutdown);
        worker.shutdown();
        assert!(worker.shutdown);
    }
    
    #[tokio::test]
    async fn test_error_result_preserves_chunk_state() {
        let broker = Arc::new(BacktestBrokerClient::in_process());
        let worker = DistributedWorker::with_broker("test-worker", "postgres://localhost/test", broker);
        
        let chunk = BacktestChunk {
            job_id: "job-123".to_string(),
            chunk_id: 5,
            total_chunks: 10,
            start_time: "2025-01-01T00:00:00Z".to_string(),
            end_time: "2025-01-02T00:00:00Z".to_string(),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            strategy_config: vec![],
            initial_capital: 10000.0,
            starting_position: 0.5,
            starting_cash: 9500.0,
            is_monte_carlo: false,
            monte_carlo_run: 0,
            priority: 5,
        };
        
        let result = worker.error_result(&chunk, "Test error".to_string());
        
        assert_eq!(result.job_id, "job-123");
        assert_eq!(result.chunk_id, 5);
        assert!(!result.success);
        assert_eq!(result.error_message, "Test error");
        // Should preserve starting state on error
        assert_eq!(result.ending_position, 0.5);
        assert_eq!(result.ending_cash, 9500.0);
    }
    
    #[test]
    fn test_get_reference_price_from_trade() {
        use dataloader::TradeData;
        
        let trade = TradeData::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            "binance".to_string(),
            BigDecimal::from_f64(50000.0).unwrap(),
            BigDecimal::from_f64(0.1).unwrap(),
            "buy".to_string(),
            "t1".to_string(),
        );
        
        let market_data = vec![MarketData::Trade(trade)];
        let price = DistributedWorker::get_reference_price(&market_data);
        
        assert!((price - 50000.0).abs() < 0.01);
    }
    
    #[test]
    fn test_get_reference_price_empty() {
        let market_data: Vec<MarketData> = vec![];
        let price = DistributedWorker::get_reference_price(&market_data);
        assert_eq!(price, 0.0);
    }
    
    #[test]
    fn test_get_first_timestamp() {
        use dataloader::TradeData;
        
        let expected_time = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let trade = TradeData::new(
            expected_time,
            "BTCUSDT".to_string(),
            "binance".to_string(),
            BigDecimal::from_f64(50000.0).unwrap(),
            BigDecimal::from_f64(0.1).unwrap(),
            "buy".to_string(),
            "t1".to_string(),
        );
        
        let market_data = vec![MarketData::Trade(trade)];
        let timestamp = DistributedWorker::get_first_timestamp(&market_data);
        
        assert_eq!(timestamp, expected_time);
    }
    
    #[tokio::test]
    async fn test_worker_with_custom_broker() {
        let broker = Arc::new(BacktestBrokerClient::in_process());
        let worker = DistributedWorker::with_broker(
            "custom-worker", 
            "postgres://localhost/test",
            broker.clone()
        );
        
        assert_eq!(worker.worker_id, "custom-worker");
    }
}
