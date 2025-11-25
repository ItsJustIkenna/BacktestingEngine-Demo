//! Worker system for processing backtest jobs
//! 
//! Workers poll the job queue (PostgreSQL) and execute backtests using the existing
//! CLI logic, then store results back to the database.

use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use bigdecimal::{BigDecimal, FromPrimitive};

use crate::job_queue::{BacktestJob, BacktestResult, JobQueue, JobStatus};
use crate::{log_info, log_error};
use crate::logging_facade::MAIN_LOGGER;

// Import existing backtest infrastructure
use backtest::BacktestEngine;
use strategy::{StrategyManager, StrategyConfig, StrategyRiskLimits};
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use riskmanager::RiskManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use config::BacktestConfig;
use dataloader::MarketData;

/// Backtest worker
pub struct Worker {
    id: usize,
    job_queue: Arc<dyn JobQueue>,
}

impl Worker {
    pub fn new(id: usize, job_queue: Arc<dyn JobQueue>) -> Self {
        Self { id, job_queue }
    }
    
    /// Start the worker loop
    pub async fn run(self) {
        log_info!(MAIN_LOGGER, "Worker {} started", self.id);
        
        loop {
            match self.process_next_job().await {
                Ok(true) => {
                    // Job processed successfully
                }
                Ok(false) => {
                    // No job available, wait a bit
                    sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    log_error!(MAIN_LOGGER, "⚠️ Worker {} error: {}", self.id, e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    
    /// Process the next job in queue
    async fn process_next_job(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Get next job from queue
        let job = match self.job_queue.get_next_job().await? {
            Some(j) => j,
            None => return Ok(false), // No job available
        };
        
        log_info!(MAIN_LOGGER, "🎯 Worker {} claimed job: {} (symbol={}, exchange={})", 
            self.id, job.id, job.params.symbol, job.params.exchange);
        
        // Update status to running
        self.job_queue
            .update_job_status(&job.id, JobStatus::Running, None)
            .await?;
        
        // Spawn heartbeat task to update job every 30 seconds
        let heartbeat_queue = self.job_queue.clone();
        let heartbeat_job_id = job.id.clone();
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Err(e) = heartbeat_queue.update_heartbeat(&heartbeat_job_id).await {
                    log_error!(MAIN_LOGGER, "⚠️ Failed to update heartbeat for job {}: {}", heartbeat_job_id, e);
                }
            }
        });
        
        // Execute backtest
        let result = match self.execute_backtest(&job).await {
            Ok(result) => {
                // Stop heartbeat task
                heartbeat_handle.abort();
                
                log_info!(MAIN_LOGGER, "💾 Storing results for job: {}", job.id);
                
                // Store results
                self.job_queue.store_result(&result).await?;
                
                // Update status to completed
                self.job_queue
                    .update_job_status(&job.id, JobStatus::Completed, None)
                    .await?;
                
                log_info!(
                    MAIN_LOGGER,
                    "✅ Worker {} completed job {} - Sharpe: {:.2}, PnL: ${:.2}, Trades: {}",
                    self.id,
                    job.id,
                    result.sharpe_ratio,
                    result.total_pnl,
                    result.total_trades
                );
                
                Ok(true)
            }
            Err(e) => {
                // Stop heartbeat task
                heartbeat_handle.abort();
                
                let error_msg = format!("Backtest failed: {}", e);
                log_error!(MAIN_LOGGER, "❌ Worker {} job {} failed: {}", self.id, job.id, error_msg);
                
                // Update status to failed
                self.job_queue
                    .update_job_status(&job.id, JobStatus::Failed, Some(error_msg))
                    .await?;
                
                Ok(true)
            }
        };
        
        result
    }
    
    /// Execute backtest using existing CLI logic
    async fn execute_backtest(
        &self,
        job: &BacktestJob,
    ) -> Result<BacktestResult, Box<dyn std::error::Error + Send + Sync>> {
        let params = &job.params;
        
        log_info!(MAIN_LOGGER, "📂 Loading market data for job {}: symbol={}, exchange={}, strategy={}", 
            job.id, params.symbol, params.exchange, params.strategy_type);
        
        // Update progress: loading data
        self.job_queue.update_progress(&job.id, 10.0).await?;
        
        // Load market data intelligently based on strategy type
        let data = self.load_market_data_for_strategy(&params.symbol, &params.exchange, &params.strategy_type).await?;
        
        if data.is_empty() {
            log_error!(MAIN_LOGGER, "❌ No market data available for job {}: symbol={}, exchange={}", 
                job.id, params.symbol, params.exchange);
            return Err("No market data available".into());
        }
        
        log_info!(MAIN_LOGGER, "✅ Loaded {} data points for job {}", data.len(), job.id);
        
        // Update progress: initializing
        self.job_queue.update_progress(&job.id, 20.0).await?;
        
        log_info!(MAIN_LOGGER, "⚙️ Initializing backtest engine for job {}: capital=${:.2}, commission={:.4}", 
            job.id, params.initial_capital, params.commission_rate);
        
        // Create configuration
        let mut backtest_config = BacktestConfig::default();
        backtest_config.trading.initial_capital = params.initial_capital;
        backtest_config.trading.commission_rate = params.commission_rate;
        
        // Create engine
        let engine = BacktestEngine::new(backtest_config);
        
        // Initialize portfolio
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from_f64(params.initial_capital)
            .ok_or("Invalid initial capital")?;
        
        // Create orderbook
        let orderbook_config = OrderBookConfig::default();
        let mut orderbook = OrderBook::new(params.symbol.clone(), params.exchange.clone(), orderbook_config);
        
        // Create risk manager
        let mut risk_manager = RiskManager::new();
        
        // Create strategy
        let strategy_params = AvellanedaStoikovParams {
            risk_aversion: params.risk_aversion,
            inventory_target: params.inventory_target,
            order_size: params.order_size,
            ..Default::default()
        };
        
        let mut strategy_params_map = HashMap::new();
        strategy_params_map.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
        strategy_params_map.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
        strategy_params_map.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
        
        let strategy_config = StrategyConfig {
            id: format!("backtest-{}", job.id),
            name: "Avellaneda-Stoikov".to_string(),
            strategy_type: "AvellanedaStoikov".to_string(),
            symbol: params.symbol.clone(),
            exchange: params.exchange.clone(),
            parameters: strategy_params_map,
            risk_limits: StrategyRiskLimits::default(),
            enabled: true,
        };
        
        let strategy = AvellanedaStoikovStrategy::new(strategy_params);
        
        log_info!(MAIN_LOGGER, "📊 Registering strategy for job {}: gamma={:.4}, k={:.4}, order_size={:.4}", 
            job.id, params.risk_aversion, params.inventory_target, params.order_size);
        
        let mut strategy_manager = StrategyManager::new();
        strategy_manager
            .register_strategy(strategy_config.clone(), strategy)
            .map_err(|e| {
                log_error!(MAIN_LOGGER, "❌ Failed to register strategy for job {}: {}", job.id, e);
                format!("Failed to register strategy: {}", e)
            })?;
        
        // Update progress: running backtest
        self.job_queue.update_progress(&job.id, 30.0).await?;
        
        log_info!(MAIN_LOGGER, "🚀 Starting backtest execution for job {}", job.id);
        
        // Run backtest
        let start_time = std::time::Instant::now();
        let result = engine
            .run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &data,
            )
            .await?;
        let execution_time = start_time.elapsed();
        
        log_info!(MAIN_LOGGER, "⏱️ Backtest execution completed for job {} in {:.2}s", 
            job.id, execution_time.as_secs_f64());
        
        // Update progress: calculating metrics
        self.job_queue.update_progress(&job.id, 90.0).await?;
        
        log_info!(MAIN_LOGGER, "📈 Calculating final metrics for job {}: trades={}, pnl=${:.2}", 
            job.id, result.core.num_trades, result.core.total_pnl);
        
        // Convert to API result format
        let total_pnl = result.core.total_pnl.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse total_pnl: {}", e))?;
        let max_drawdown = result.core.max_drawdown.to_string().parse::<f64>()
            .map_err(|e| format!("Failed to parse max_drawdown: {}", e))?;
        
        let backtest_result = BacktestResult {
            job_id: job.id.clone(),
            total_pnl,
            sharpe_ratio: result.core.sharpe_ratio.unwrap_or(0.0),
            max_drawdown,
            win_rate: result.core.win_rate.unwrap_or(0.0),
            total_trades: result.core.num_trades,
            profit_factor: result.core.profit_factor.unwrap_or(1.0),
            equity_curve: result.core.equity_curve,
            completed_at: Utc::now(),
        };
        
        Ok(backtest_result)
    }
    
    /// Load market data intelligently based on strategy type
    async fn load_market_data_for_strategy(
        &self,
        symbol: &str,
        exchange: &str,
        strategy_type: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        match strategy_type {
            "market_making" => {
                log_info!(MAIN_LOGGER, "📊 Loading order book data for market making strategy");
                // For market making, load order events to reconstruct order book
                self.load_order_events(symbol, exchange).await
            }
            _ => {
                // Default to candles for other strategies
                log_info!(MAIN_LOGGER, "📊 Loading candle data for strategy type: {}", strategy_type);
                self.load_market_data(symbol, exchange).await
            }
        }
    }
    
    /// Load order book events (new, cancel, modify) for market making strategies
    #[cfg(feature = "postgres")]
    async fn load_order_events(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        use diesel::prelude::*;
        use diesel::sql_types::{Timestamptz, Varchar, Numeric, Nullable};
        use databaseschema::models::historical_order::HistoricalOrder;
        use uuid::Uuid;
        
        #[derive(QueryableByName)]
        struct OrderEventRow {
            #[diesel(sql_type = Timestamptz)]
            timestamp: chrono::NaiveDateTime,
            #[diesel(sql_type = Varchar)]
            symbol: String,
            #[diesel(sql_type = Varchar)]
            exchange: String,
            #[diesel(sql_type = Varchar)]
            event_type: String,
            #[diesel(sql_type = Varchar)]
            side: String,
            #[diesel(sql_type = Numeric)]
            price_level: BigDecimal,
            #[diesel(sql_type = Numeric)]
            quantity: BigDecimal,
            #[diesel(sql_type = Varchar)]
            order_id: String,
            #[diesel(sql_type = Nullable<Numeric>)]
            prev_price: Option<BigDecimal>,
            #[diesel(sql_type = Nullable<Numeric>)]
            prev_quantity: Option<BigDecimal>,
            #[diesel(sql_type = Varchar)]
            status: String,
        }
        
        let pool = self.job_queue.get_pool()
            .ok_or("Database pool not available")?;
        
        let mut conn = pool.get()
            .map_err(|e| format!("Failed to get database connection: {}", e))?;
        
        // Query order book events (new, cancel, modify)
        let query = format!(
            "SELECT timestamp, symbol, exchange, event_type, side, price_level, quantity, \
             order_id, prev_price, prev_quantity, status \
             FROM historical_orders \
             WHERE symbol = '{}' AND exchange = '{}' \
             AND event_type IN ('new', 'cancel', 'modify') \
             ORDER BY timestamp ASC LIMIT 50000",
            symbol, exchange
        );
        
        let results: Vec<OrderEventRow> = diesel::sql_query(&query)
            .load(&mut conn)
            .map_err(|e| format!("Failed to load order events: {}", e))?;
        
        let orders: Vec<MarketData> = results
            .into_iter()
            .map(|row| {
                MarketData::Order(HistoricalOrder {
                    event_id: Uuid::new_v4(),
                    timestamp: chrono::DateTime::from_naive_utc_and_offset(row.timestamp, chrono::Utc),
                    order_id: row.order_id,
                    event_type: row.event_type,
                    side: row.side,
                    price_level: row.price_level,
                    quantity: row.quantity,
                    prev_price: row.prev_price,
                    prev_quantity: row.prev_quantity,
                    status: row.status,
                    exchange: row.exchange,
                    symbol: row.symbol,
                    security_id: Uuid::new_v4(),
                    exchange_id: Uuid::new_v4(),
                })
            })
            .collect();
        
        log_info!(MAIN_LOGGER, "📊 Loaded {} order book events for {}/{}", orders.len(), symbol, exchange);
        Ok(orders)
    }
    
    #[cfg(not(feature = "postgres"))]
    async fn load_order_events(
        &self,
        _symbol: &str,
        _exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        Err("PostgreSQL feature not enabled".into())
    }
    
    /// Load market data from database (candles from continuous aggregates)
    async fn load_market_data(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<MarketData>, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "postgres")]
        {
            use diesel::prelude::*;
            use diesel::sql_types::{Timestamptz, Varchar, Numeric, BigInt};
            use dataloader::Candle;
            
            #[derive(QueryableByName)]
            struct CandleRow {
                #[diesel(sql_type = Timestamptz)]
                timestamp: chrono::NaiveDateTime,
                #[diesel(sql_type = Varchar)]
                symbol: String,
                #[diesel(sql_type = Varchar)]
                exchange: String,
                #[diesel(sql_type = Numeric)]
                open_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                high_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                low_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                close_price: BigDecimal,
                #[diesel(sql_type = Numeric)]
                volume: BigDecimal,
                #[diesel(sql_type = BigInt)]
                trade_count: i64,
            }
            
            // Get database pool from job queue
            let pool = self.job_queue.get_pool()
                .ok_or("Database pool not available")?;
            
            let mut conn = pool.get()
                .map_err(|e| format!("Failed to get database connection: {}", e))?;
            
            // Query candles_1m view for the symbol/exchange
            // Note: For now we load 1-minute candles. Could make timeframe configurable.
            let query = format!(
                "SELECT timestamp, symbol, exchange, open_price, high_price, low_price, \
                 close_price, volume, trade_count FROM candles_1m \
                 WHERE symbol = '{}' AND exchange = '{}' \
                 ORDER BY timestamp ASC LIMIT 10000",
                symbol, exchange
            );
            
            let results: Vec<CandleRow> = diesel::sql_query(&query)
                .load(&mut conn)
                .map_err(|e| format!("Failed to load candles: {}", e))?;
            
            let candles: Vec<MarketData> = results
                .into_iter()
                .map(|row| {
                    MarketData::Candle(Candle {
                        timestamp: chrono::DateTime::from_naive_utc_and_offset(row.timestamp, chrono::Utc),
                        symbol: row.symbol,
                        exchange: row.exchange,
                        open: row.open_price,
                        high: row.high_price,
                        low: row.low_price,
                        close: row.close_price,
                        volume: row.volume,
                        trade_count: row.trade_count,
                    })
                })
                .collect();
            
            log_info!(MAIN_LOGGER, "📊 Loaded {} candles for {}/{}", candles.len(), symbol, exchange);
            Ok(candles)
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = (symbol, exchange);
            Err("PostgreSQL feature not enabled".into())
        }
    }
}

/// Spawn multiple workers
pub fn spawn_workers(num_workers: usize, job_queue: Arc<dyn JobQueue>) {
    for i in 0..num_workers {
        let job_queue_clone = Arc::clone(&job_queue);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|e| {
                    log_error!(MAIN_LOGGER, "Failed to create worker runtime: {}", e);
                    panic!("Cannot create worker runtime");
                });
            
            runtime.block_on(async {
                let worker = Worker::new(i, job_queue_clone);
                worker.run().await;
            });
        });
    }
    
    log_info!(MAIN_LOGGER, "Spawned {} workers", num_workers);
}
