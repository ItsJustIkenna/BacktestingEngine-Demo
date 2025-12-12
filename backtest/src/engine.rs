//! Clean and simplified backtesting engine
//! Combines basic and enhanced functionality in a unified interface

use std::time::Instant;
use anyhow::Result;
use serde::{Serialize, Deserialize};

use crate::{BacktestResult, monte_carlo, simulation::SimulationLoop};
use crate::logging_facade::BACKTEST_LOGGER;
use crate::{log_info, log_debug, log_warn, log_error};
use config::{BacktestConfig, AnalysisMode, AnalysisConfig};
use strategy::{StrategyManager, simple_export::ExportConfig};
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
#[cfg(feature = "postgres")]
use databaseschema::models::historical_order::HistoricalOrder;
use std::sync::Arc;

/// Progress callback for Monte Carlo and Walk-Forward analysis
/// Parameters: (phase_name, current, total)
pub type ProgressCallback = Arc<dyn Fn(&str, usize, usize) + Send + Sync>;

/// Main backtesting engine with intelligent analysis capabilities
pub struct BacktestEngine {
    config: BacktestConfig,
    analysis_config: AnalysisConfig,
    progress_callback: Option<ProgressCallback>,
}

/// Comprehensive backtest results with optional advanced analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    /// Core backtest metrics
    pub core: BacktestResult,
    /// Monte Carlo simulation results (if performed)
    pub monte_carlo: Option<monte_carlo::MonteCarloResult>,
    /// Walk-forward analysis results (if performed)  
    pub walk_forward: Option<WalkForwardSummary>,
    /// Analysis metadata and recommendations
    pub metadata: AnalysisMetadata,
}

/// Walk-forward analysis summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardSummary {
    pub periods_tested: usize,
    pub average_performance: f64,
    pub consistency_score: f64,
    pub out_of_sample_degradation: f64,
}

/// Analysis execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisMetadata {
    pub analysis_mode: AnalysisMode,
    pub mode_escalated: bool,
    pub total_runtime_seconds: f64,
    pub data_quality_score: f64,
    pub triggers_fired: Vec<String>,
    pub recommendations: Vec<String>,
}

impl BacktestEngine {
    /// Create a new backtesting engine with configuration
    pub fn new(config: BacktestConfig) -> Self {
        let analysis_config = config.analysis.clone();
        
        Self {
            config,
            analysis_config,
            progress_callback: None,
        }
    }
    
    /// Get a reference price from market data (first available price)
    /// Used for converting order_size_pct to asset units (works for any trading pair)
    fn get_reference_price(market_data: &[MarketData]) -> Option<f64> {
        use bigdecimal::ToPrimitive;
        
        market_data.first().and_then(|data| {
            match data {
                #[cfg(feature = "postgres")]
                MarketData::Order(o) => o.price_level.to_f64(),
                #[cfg(feature = "postgres")]
                MarketData::Snapshot(s) => s.price_level.to_f64(),
                MarketData::Candle(c) => c.close.to_f64(),
                MarketData::Trade(t) => t.price.to_f64(),
            }
        })
    }
    
    /// Set a progress callback for Monte Carlo and Walk-Forward analysis
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Run backtest with automatic analysis mode selection
    pub async fn run_backtest(
        &self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
    ) -> Result<BacktestResults> {
        self.run_with_export_option(
            strategy_manager,
            orderbook,
            portfolio,
            risk_manager,
            market_data,
            false,
        ).await
    }

    /// Run backtest with export consideration (forces production mode)
    pub async fn run_with_export_option(
        &self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
        export_requested: bool,
    ) -> Result<BacktestResults> {
        let start_time = Instant::now();
        
        log_info!(BACKTEST_LOGGER, "run_with_export_option ENTER | data_points={} export={}", 
            market_data.len(), export_requested);
        
        // Determine analysis mode
        let mut current_mode = self.determine_analysis_mode(export_requested);
        let original_mode = current_mode;
        
        log_info!(BACKTEST_LOGGER, "Starting Backtest Analysis - Mode: {:?}", current_mode);
        
        // PRE-PROCESSING: Analyze and handle data gaps BEFORE optimization/simulation
        #[cfg(feature = "postgres")]
        let data_quality_report = self.generate_data_quality_report(market_data, &[]);
        #[cfg(not(feature = "postgres"))]
        let data_quality_report = self.generate_data_quality_report(market_data);
        
        let severe_gap_count = data_quality_report.gaps_detected.iter()
            .filter(|g| matches!(g.severity, crate::types::GapSeverity::Severe | crate::types::GapSeverity::Critical))
            .count();
        log_info!(BACKTEST_LOGGER, "Data Quality: {:.1}/10, {} points, {} gaps ({} severe)", 
            data_quality_report.overall_score, 
            data_quality_report.total_data_points,
            data_quality_report.gaps_detected.len(),
            severe_gap_count
        );
        
        // Apply gap filling strategy if configured
        let processed_data = if self.config.data.gap_detection.enabled {
            self.apply_gap_filling(market_data)?
        } else {
            market_data.to_vec()
        };
        
        if processed_data.len() != market_data.len() {
            log_debug!(BACKTEST_LOGGER, "Gap filling applied: {} → {} data points", 
                market_data.len(), processed_data.len());
        }
        
        // Use processed_data for all subsequent operations
        let market_data = &processed_data;
        
        // MANDATORY: Run genetic optimization first to find optimal parameters (unless skipped)
        if !self.analysis_config.skip_genetic_optimization {
            log_info!(BACKTEST_LOGGER, "Running genetic optimization");
            let optimized_params = self.run_genetic_optimization(
                strategy_manager,
                orderbook,
                portfolio,
                risk_manager,
                market_data
            ).await?;
            
            // Apply optimized parameters to all strategies
            strategy_manager.update_all_parameters(optimized_params).await?;
            log_info!(BACKTEST_LOGGER, "Genetic optimization complete");
        } else {
            log_info!(BACKTEST_LOGGER, "Skipping GA (pre-optimized) - proceeding to simulation");
        }
        
        // Run core backtest simulation with optimized parameters
        log_info!(BACKTEST_LOGGER, "PHASE_START | backtest_simulation | data_points={}", market_data.len());
        let sim_start = Instant::now();
        let mut core_result = self.run_core_simulation(
            strategy_manager, 
            orderbook, 
            portfolio, 
            risk_manager, 
            market_data
        ).await?;
        log_info!(BACKTEST_LOGGER, "PHASE_COMPLETE | backtest_simulation | duration={:.1}s trades={}", 
            sim_start.elapsed().as_secs_f64(), core_result.num_trades);
        
        // Attach data quality report to results
        core_result.data_quality = Some(data_quality_report.clone());
        
        // Auto-escalation logic based on performance
        let mut escalation_triggers = Vec::with_capacity(2);  // Max 2 escalations
        let mut mode_escalated = false;
        
        if current_mode == AnalysisMode::Development {
            if self.should_escalate_to_validation(&core_result) {
                current_mode = AnalysisMode::Validation;
                mode_escalated = true;
                escalation_triggers.push("Poor performance detected - escalated to Validation".to_string());
                log_info!(BACKTEST_LOGGER, "Auto-escalating to Validation mode");
            }
        }
        
        if current_mode == AnalysisMode::Validation {
            if self.should_escalate_to_production(&core_result) {
                current_mode = AnalysisMode::Production;
                mode_escalated = true;
                escalation_triggers.push("Excellent performance detected - escalated to Production".to_string());
                log_info!(BACKTEST_LOGGER, "Auto-escalating to Production mode");
            }
        }
        
        // Run additional analysis based on final mode
        let monte_carlo_result = match current_mode {
            AnalysisMode::Development => None,
            AnalysisMode::Validation | AnalysisMode::Production => {
                let mc_runs = self.analysis_config.monte_carlo_runs;
                log_info!(BACKTEST_LOGGER, "PHASE_START | monte_carlo | runs={}", mc_runs);
                let mc_start = Instant::now();
                let result = self.run_monte_carlo_analysis(&core_result).await?;
                log_info!(BACKTEST_LOGGER, "PHASE_COMPLETE | monte_carlo | duration={:.1}s", mc_start.elapsed().as_secs_f64());
                Some(result)
            }
        };
        
        let walk_forward_result = match current_mode {
            AnalysisMode::Production if self.analysis_config.auto_walk_forward => {
                log_info!(BACKTEST_LOGGER, "PHASE_START | walk_forward | data_points={}", market_data.len());
                let wf_start = Instant::now();
                // Pass current strategy parameters to test robustness across time windows
                let result = self.run_walk_forward_analysis(
                    strategy_manager, orderbook, portfolio, risk_manager, market_data
                ).await?;
                log_info!(BACKTEST_LOGGER, "PHASE_COMPLETE | walk_forward | duration={:.1}s windows={}", 
                    wf_start.elapsed().as_secs_f64(), result.periods_tested);
                Some(result)
            },
            _ => None,
        };
        
        let total_runtime = start_time.elapsed().as_secs_f64();
        
        // Generate analysis metadata
        let metadata = AnalysisMetadata {
            analysis_mode: current_mode,
            mode_escalated: mode_escalated && original_mode != current_mode,
            total_runtime_seconds: total_runtime,
            data_quality_score: self.calculate_data_quality_score(market_data),
            triggers_fired: escalation_triggers,
            recommendations: self.generate_recommendations(&core_result, &monte_carlo_result),
        };
        
        let results = BacktestResults {
            core: core_result,
            monte_carlo: monte_carlo_result,
            walk_forward: walk_forward_result,
            metadata,
        };
        
        log_info!(BACKTEST_LOGGER, "Backtest completed in {:.1}s", total_runtime);
        
        Ok(results)
    }

    /// Determine initial analysis mode based on configuration and export requirements
    fn determine_analysis_mode(&self, export_requested: bool) -> AnalysisMode {
        if export_requested {
            AnalysisMode::Production
        } else {
            self.analysis_config.mode
        }
    }

    /// Check if performance warrants escalation to validation mode
    fn should_escalate_to_validation(&self, result: &BacktestResult) -> bool {
        if let Some(sharpe) = result.sharpe_ratio {
            sharpe < self.analysis_config.validation_sharpe_threshold
        } else {
            false
        }
    }
    
    /// Check if performance warrants escalation to production mode
    fn should_escalate_to_production(&self, result: &BacktestResult) -> bool {
        if let Some(sharpe) = result.sharpe_ratio {
            sharpe > self.analysis_config.validation_sharpe_threshold * 1.5
        } else {
            false
        }
    }
    
    /// Run the core backtest simulation (internal use for Walk-Forward, etc.)
    async fn run_core_simulation(
        &self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
    ) -> Result<BacktestResult> {
        // Run actual simulation using SimulationLoop
        let mut sim_loop = SimulationLoop::new();
        let result = sim_loop.run(
            strategy_manager,
            orderbook,
            portfolio,
            risk_manager,
            market_data.to_vec(),
            &self.config,
        ).await;
        
        Ok(result)
    }
    
    /// Run the core backtest simulation without analysis (Monte Carlo, Walk-Forward)
    /// This is used by genetic optimization fitness functions for fast evaluation
    pub async fn run_core_simulation_direct(
        &self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
    ) -> Result<BacktestResult> {
        self.run_core_simulation(strategy_manager, orderbook, portfolio, risk_manager, market_data).await
    }
    
    /// Run genetic optimization to find optimal strategy parameters (MANDATORY)
    async fn run_genetic_optimization(
        &self,
        _strategy_manager: &mut StrategyManager,
        _orderbook: &mut OrderBook,
        _portfolio: &mut PortfolioState,
        _risk_manager: &mut RiskManager,
        market_data: &[MarketData],
    ) -> Result<std::collections::HashMap<String, config::ParameterValue>> {
        use genetic::{GeneticOptimizer, chromosome::AvellanedaStoikovChromosome};
        use config::GeneticConfig;
        use std::sync::Arc;
        use std::pin::Pin;
        use std::future::Future;
        
        // Configure genetic algorithm based on analysis mode
        let (population_size, generations) = match self.analysis_config.mode {
            config::AnalysisMode::Development => (5, 3),      // Minimal for testing (15 backtests)
            config::AnalysisMode::Validation => (20, 10),     // Moderate
            config::AnalysisMode::Production => (50, 25),     // Comprehensive
        };
        
        let config = GeneticConfig {
            population_size,
            generations,
            crossover_rate: 0.7,
            mutation_rate: 0.2,
            use_monte_carlo_fitness: false,
            mc_sample_size: 50,
            top_percentile_threshold: 0.10,
        };
        
        // Create fitness function that runs backtest with parameters
        let market_data_clone = market_data.to_vec();
        let backtest_config = self.config.clone();
        
        let fitness_fn = Arc::new(move |chromo: &AvellanedaStoikovChromosome| {
            let market_data = market_data_clone.clone();
            let config = backtest_config.clone();
            let chromo = chromo.clone(); // Clone to satisfy 'static lifetime
            
            Box::pin(async move {
                // Calculate order_size in asset units from percentage of capital
                // order_size = (capital * order_size_pct) / asset_price
                let initial_capital = config.trading.initial_capital;
                let asset_price = Self::get_reference_price(&market_data).unwrap_or(100_000.0);
                let order_size_units = (initial_capital * chromo.order_size_pct) / asset_price;
                
                // Create temporary components for fitness evaluation
                let mut temp_strategy_manager = strategy::StrategyManager::new();
                let mut temp_orderbook = orderbook::OrderBook::new(
                    "BTC/USD".to_string(),
                    "Kraken".to_string(),
                    orderbook::OrderBookConfig::default()
                );
                
                // Initialize portfolio with BOTH quote and base assets for proper market making
                // Split capital: 50% as quote (USD), 50% as base asset (BTC/ETH)
                use bigdecimal::FromPrimitive;
                let inventory_pct = 0.5;
                let quote_balance = initial_capital * (1.0 - inventory_pct);
                let base_value = initial_capital * inventory_pct;
                let base_quantity = if asset_price > 0.0 { base_value / asset_price } else { 0.0 };
                
                // Get first timestamp from market data
                let first_timestamp = market_data.first()
                    .map(|d| match d {
                        dataloader::MarketData::Trade(t) => t.timestamp,
                        dataloader::MarketData::Candle(c) => c.timestamp,
                        #[cfg(feature = "postgres")]
                        dataloader::MarketData::Snapshot(s) => s.timestamp,
                        #[cfg(feature = "postgres")]
                        dataloader::MarketData::Order(o) => o.timestamp,
                    })
                    .unwrap_or_else(chrono::Utc::now);
                
                let mut temp_portfolio = portfoliomanager::PortfolioState::with_inventory(
                    "BTC/USD".to_string(),
                    bigdecimal::BigDecimal::from_f64(quote_balance).unwrap_or_default(),
                    base_quantity,
                    bigdecimal::BigDecimal::from_f64(asset_price).unwrap_or_default(),
                    first_timestamp,
                );
                let mut temp_risk_manager = riskmanager::RiskManager::new();
                
                // Create strategy with chromosome parameters (including Phase 2 dynamic params)
                // Get exchange maker fee from config
                let exchange_fee_config = config::ExchangeFeeConfig::for_exchange("kraken");
                let maker_fee = exchange_fee_config.maker_fee;
                
                let strategy = strategy::strategies::avellaneda_stoikov::AvellanedaStoikovStrategy::new(
                    strategy::strategies::avellaneda_stoikov::AvellanedaStoikovParams {
                        risk_aversion: chromo.risk_aversion,
                        order_size: order_size_units,
                        order_size_pct: chromo.order_size_pct,
                        initial_capital,
                        initial_inventory_pct: 0.5, // 50% of capital as base asset for market making
                        window_ms: chromo.window_ms,
                        min_quote_lifetime_ms: chromo.min_quote_lifetime_ms,
                        fee_rate: maker_fee,  // Use actual exchange maker fee, not chromosome
                        inventory_target: chromo.inventory_target,
                        volatility_cap: chromo.volatility_cap,
                        volatility_floor: 0.20, // 20% annualized floor for EWMA cold-start
                        time_horizon_minutes: chromo.time_horizon_minutes,
                        market_depth_k: chromo.market_depth_k,
                        min_spread_bps: chromo.min_spread_bps,
                        max_trades_per_minute: chromo.max_trades_per_minute,
                        take_profit_pct: chromo.take_profit_pct,
                        stop_loss_pct: chromo.stop_loss_pct,
                        
                        // Phase 2: Dynamic adjustment parameters
                        volatility_adjustment_factor: chromo.volatility_adjustment_factor,
                        inventory_penalty_factor: chromo.inventory_penalty_factor,
                        trend_sensitivity: chromo.trend_sensitivity,
                        flow_sensitivity: chromo.flow_sensitivity,
                        
                        // Phase 3: Dynamic k parameters (k divides liquidity term)
                        k_volatility_sensitivity: 0.3,
                        k_flow_sensitivity: 0.2,
                        k_min: 1.0,   // Increased from 0.5 for tighter spreads
                        k_max: 50.0,  // Increased for liquid markets - GA can explore tight spreads
                        
                        // Phase 4: Dynamic time horizon parameters
                        time_horizon_vol_sensitivity: 0.5,
                        time_horizon_min: 0.25,
                        time_horizon_max: 5.0,
                        
                        // Phase 4: Dynamic min spread parameters
                        min_spread_vol_sensitivity: 0.3,
                        min_spread_flow_sensitivity: 0.2,
                        min_spread_min_bps: 3.0,
                        min_spread_max_bps: 50.0,
                        
                        // Phase 4: Dynamic order size parameters
                        size_vol_sensitivity: 0.3,
                        size_drawdown_sensitivity: 0.5,
                        size_win_boost: 0.1,
                        size_min_pct: 0.005,
                        size_max_pct: 0.05,
                        
                        // Phase 4: Dynamic rate limiting parameters
                        rate_vol_sensitivity: 0.4,
                        rate_min: 5.0,
                        rate_max: 60.0,
                        
                        // Inventory risk limits
                        max_inventory_pct: 0.15,
                        inventory_skew_multiplier: 3.0,
                        
                        // Phase 5: Regime detection parameters
                        regime_vol_threshold_high: 1.5,
                        regime_vol_threshold_low: 0.7,
                        regime_trend_threshold: 0.3,
                        regime_high_vol_spread_mult: 1.5,
                        regime_low_vol_spread_mult: 0.8,
                        regime_trending_inv_mult: 0.5,
                        
                        // Phase 5: Order book microstructure parameters
                        orderbook_imbalance_sensitivity: 0.3,
                        orderbook_depth_levels: 5,
                        imbalance_skew_factor: 0.5,
                        
                        // Phase 5: Adverse selection parameters
                        informed_trader_prob_window_ms: 5000,
                        adverse_selection_multiplier: 2.0,
                        toxic_flow_threshold: 0.6,
                    }
                );
                
                // Register strategy with default config
                let strategy_config = strategy::StrategyConfig {
                    id: "temp_strategy".to_string(),
                    name: "Avellaneda-Stoikov".to_string(),
                    strategy_type: "AvellanedaStoikovMarketMaker".to_string(),
                    symbol: "BTC/USD".to_string(),
                    exchange: "Kraken".to_string(),
                    enabled: true,
                    parameters: std::collections::HashMap::new(),
                    risk_limits: strategy::config::RiskLimits::default(),
                };
                temp_strategy_manager.register_strategy(strategy_config, strategy).ok();
                
                // Run backtest
                let mut sim_loop = crate::simulation::SimulationLoop::new();
                let result = sim_loop.run(
                    &mut temp_strategy_manager,
                    &mut temp_orderbook,
                    &mut temp_portfolio,
                    &mut temp_risk_manager,
                    market_data,
                    &config,
                ).await;
                
                // ========== COMPREHENSIVE FITNESS FUNCTION ==========
                // Designed to find genuinely profitable strategies, not just good-looking metrics
                
                use bigdecimal::ToPrimitive;
                let initial_capital = config.trading.initial_capital;
                
                // === HARD CONSTRAINT: Reject commission-inefficient strategies early ===
                if let (Some(gross_profit), Some(transaction_costs)) = 
                    (&result.gross_profit, &result.transaction_costs) 
                {
                    let gross = *gross_profit;
                    let commission = transaction_costs.total_commission;
                    
                    if gross <= 0.0 {
                        return genetic::FitnessResult::new(0.001, result.equity_curve);
                    }
                    let commission_ratio = commission / gross;
                    
                    // HARD REJECT: Commission ratio >= 80% means strategy is non-viable
                    if commission_ratio >= 0.80 {
                        let tiny_fitness = 0.001 * (1.0 - (commission_ratio - 0.80).min(0.5));
                        return genetic::FitnessResult::new(tiny_fitness, result.equity_curve);
                    }
                    
                    // GRADUATED PENALTY: Penalize commission ratios > 30%
                    // This encourages the GA to find strategies with reasonable commission costs
                    // Ideal market making: commission ratio < 30% of gross profit
                    // Note: Additional penalty applied after base fitness calculation
                }
                
                // Calculate commission penalty multiplier (used later)
                let commission_penalty: f64 = if let (Some(gross_profit), Some(transaction_costs)) = 
                    (&result.gross_profit, &result.transaction_costs) 
                {
                    let gross = *gross_profit;
                    let commission = transaction_costs.total_commission;
                    if gross > 0.0 {
                        let ratio = commission / gross;
                        if ratio > 0.30 {
                            // Graduated penalty: 1.0 at 30%, 0.5 at 55%, 0.2 at 80%
                            (1.0 - (ratio - 0.30) * 1.6).max(0.2)
                        } else {
                            1.0 // No penalty below 30%
                        }
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };
                
                // === Primary Metrics (weighted combination) ===
                let sharpe = result.sharpe_ratio.unwrap_or(0.0);
                let sortino = result.sortino_ratio.unwrap_or(sharpe);
                let pf = result.profit_factor.unwrap_or(1.0);
                let win_rate = result.win_rate.unwrap_or(0.0);
                let net_profit_pct = result.net_profit.unwrap_or(0.0) / initial_capital;
                let calmar = result.calmar_ratio.unwrap_or(0.0);
                
                // === Base Fitness Calculation ===
                // Emphasize NET profitability more heavily
                let mut fitness = 
                    sortino * 0.15 +
                    sharpe * 0.10 +
                    (pf - 1.0).max(0.0) * 0.15 +
                    win_rate * 0.10 +
                    net_profit_pct * 0.35 +    // Increased: Net profit is what matters!
                    calmar.min(3.0) * 0.15;
                
                // === Penalty: Insufficient Trades ===
                let dd_value = result.max_drawdown.to_f64().unwrap_or(0.0);
                let dd_pct = (dd_value / initial_capital).abs();
                
                if result.num_trades < 30 {
                    fitness *= (result.num_trades as f64 / 30.0).max(0.1);
                }
                
                // === Penalty: Excessive Drawdown ===
                if dd_pct > 0.15 {
                    let dd_penalty = if dd_pct > 0.40 { 0.2 } else if dd_pct > 0.25 { 0.5 } else { 0.8 };
                    fitness *= dd_penalty;
                }
                
                // === Apply Commission Penalty (calculated earlier) ===
                // Graduated penalty: 1.0 at 30% ratio, 0.5 at 55%, 0.2 at 80%
                fitness *= commission_penalty;
                
                // === Penalty: Poor Average Trade ===
                if let (Some(avg_trade), Some(transaction_costs)) = 
                    (&result.avg_trade_return, &result.transaction_costs)
                {
                    let avg_commission_per_trade = if result.num_trades > 0 {
                        transaction_costs.total_commission / result.num_trades as f64
                    } else {
                        0.0
                    };
                    
                    if *avg_trade > 0.0 && *avg_trade < avg_commission_per_trade * 3.0 {
                        fitness *= 0.5;
                    }
                }
                
                // === Bonus: Well-Rounded Strategy ===
                if net_profit_pct > 0.05 && pf > 1.5 && win_rate > 0.50 && dd_pct < 0.15 {
                    fitness *= 1.2;
                }
                
                let final_fitness = fitness.max(0.0);
                genetic::FitnessResult::new(final_fitness, result.equity_curve)
            }) as Pin<Box<dyn Future<Output = genetic::FitnessResult> + Send + 'static>>
        });
        
        log_debug!(BACKTEST_LOGGER, "Genetic: pop={}, gen={}", population_size, generations);
        
        let optimizer = GeneticOptimizer {
            config,
            fitness_fn,
            progress_callback: None,
            job_id: None,
        };
        
        let (best_chromo, best_fitness) = optimizer.run().await;
        
        // Calculate order_size in asset units from percentage for logging and params
        let asset_price = Self::get_reference_price(market_data).unwrap_or(100_000.0);
        let order_size_units = (self.config.trading.initial_capital * best_chromo.order_size_pct) / asset_price;
        
        log_info!(BACKTEST_LOGGER, "Best fitness: {:.4}, risk_aversion={:.4}, order_size_pct={:.2}% (={:.6} units)", 
            best_fitness, best_chromo.risk_aversion, best_chromo.order_size_pct * 100.0, order_size_units);
        
        // Convert chromosome to parameter map (Phase 2: all 14 parameters)
        // Note: order_size is stored in asset units, converted from percentage
        // Get exchange maker fee from config for consistent fee handling
        let exchange_fee_config = config::ExchangeFeeConfig::for_exchange("kraken");
        let maker_fee = exchange_fee_config.maker_fee;
        
        let mut params = std::collections::HashMap::new();
        params.insert("risk_aversion".to_string(), config::ParameterValue::Float(best_chromo.risk_aversion));
        params.insert("order_size".to_string(), config::ParameterValue::Float(order_size_units));
        params.insert("order_size_pct".to_string(), config::ParameterValue::Float(best_chromo.order_size_pct));
        params.insert("window_ms".to_string(), config::ParameterValue::Float(best_chromo.window_ms as f64));
        params.insert("min_quote_lifetime_ms".to_string(), config::ParameterValue::Float(best_chromo.min_quote_lifetime_ms as f64));
        params.insert("fee_rate".to_string(), config::ParameterValue::Float(maker_fee));  // Use exchange fee
        params.insert("inventory_target".to_string(), config::ParameterValue::Float(best_chromo.inventory_target));
        params.insert("volatility_cap".to_string(), config::ParameterValue::Float(best_chromo.volatility_cap));
        params.insert("time_horizon_minutes".to_string(), config::ParameterValue::Float(best_chromo.time_horizon_minutes));
        params.insert("market_depth_k".to_string(), config::ParameterValue::Float(best_chromo.market_depth_k));
        
        // Phase 2 dynamic adjustment parameters
        params.insert("volatility_adjustment_factor".to_string(), config::ParameterValue::Float(best_chromo.volatility_adjustment_factor));
        params.insert("inventory_penalty_factor".to_string(), config::ParameterValue::Float(best_chromo.inventory_penalty_factor));
        params.insert("trend_sensitivity".to_string(), config::ParameterValue::Float(best_chromo.trend_sensitivity));
        params.insert("flow_sensitivity".to_string(), config::ParameterValue::Float(best_chromo.flow_sensitivity));
        
        Ok(params)
    }
    
    /// Run Monte Carlo simulation analysis
    async fn run_monte_carlo_analysis(&self, base_result: &BacktestResult) -> Result<monte_carlo::MonteCarloResult> {
        use crate::logging_facade::BACKTEST_LOGGER;
        use crate::log_info;
        
        let num_runs = self.analysis_config.monte_carlo_runs;
        let num_trades = base_result.num_trades;
        let base_result = base_result.clone();
        
        // Log before spawn_blocking (we have Tokio runtime here)
        log_info!(BACKTEST_LOGGER, "Monte Carlo starting | runs={} trades={} shuffle={} bootstrap={}", 
                  num_runs, num_trades, num_runs / 2, num_runs - num_runs / 2);
        
        let start_time = std::time::Instant::now();
        
        // Run Monte Carlo in blocking thread pool (CPU-intensive, now parallel with Rayon)
        // Note: Logging inside spawn_blocking won't work since there's no Tokio runtime
        let mc_result = tokio::task::spawn_blocking(move || {
            monte_carlo::run_monte_carlo_simulation(
                &base_result, 
                num_runs
            )
        }).await??;
        
        let elapsed = start_time.elapsed();
        
        // Log after spawn_blocking completes (we have Tokio runtime here)
        log_info!(BACKTEST_LOGGER, "Monte Carlo complete | runs={} elapsed={:.2}s mean_sharpe={:.3} var_95=${:.2} cvar_95=${:.2}", 
                  num_runs, elapsed.as_secs_f64(), mc_result.mean_metrics.sharpe_ratio, 
                  mc_result.var_95, mc_result.cvar_95);
        
        Ok(mc_result)
    }
    
    /// Run walk-forward analysis
    async fn run_walk_forward_analysis(
        &self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: &[MarketData],
    ) -> Result<WalkForwardSummary> {
        use walkforward::{WalkForwardConfig, WalkForwardAnalyzer};
        
        // Calculate data duration
        let data_duration_days = if market_data.len() >= 2 {
            #[cfg(feature = "postgres")]
            let first_ts = match &market_data[0] {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Order(o) => o.timestamp,
            };
            #[cfg(not(feature = "postgres"))]
            let first_ts = match &market_data[0] {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            
            #[cfg(feature = "postgres")]
            let last_ts = match &market_data[market_data.len() - 1] {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Order(o) => o.timestamp,
            };
            #[cfg(not(feature = "postgres"))]
            let last_ts = match &market_data[market_data.len() - 1] {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            
            (last_ts - first_ts).num_days()
        } else {
            0
        };
        
        // Dynamically adjust window sizes based on available data
        // Use 2/3 for training, 1/3 for testing
        let optimization_days = (data_duration_days as f64 * 0.67) as i64;
        let test_days = data_duration_days - optimization_days;
        
        // Need at least 3 days total for meaningful walk-forward (2 train, 1 test)
        if data_duration_days < 3 {
            return Ok(WalkForwardSummary {
                periods_tested: 0,
                average_performance: 0.0,
                consistency_score: 0.0,
                out_of_sample_degradation: 0.0,
            });
        }
        
        #[cfg(feature = "postgres")]
        let start_date = match &market_data[0] {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Trade(t) => t.timestamp,
            MarketData::Snapshot(s) => s.timestamp,
            MarketData::Order(o) => o.timestamp,
        };
        #[cfg(not(feature = "postgres"))]
        let start_date = match &market_data[0] {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Trade(t) => t.timestamp,
        };
        
        #[cfg(feature = "postgres")]
        let end_date = match &market_data[market_data.len() - 1] {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Trade(t) => t.timestamp,
            MarketData::Snapshot(s) => s.timestamp,
            MarketData::Order(o) => o.timestamp,
        };
        #[cfg(not(feature = "postgres"))]
        let end_date = match &market_data[market_data.len() - 1] {
            MarketData::Candle(c) => c.timestamp,
            MarketData::Trade(t) => t.timestamp,
        };
        
        // Use larger step size to create fewer windows (3 windows max for efficiency)
        // Each window tests parameter robustness in different market conditions
        let step_size = (test_days).max(7); // Non-overlapping windows, minimum 7 days
        
        let wf_config = WalkForwardConfig {
            start_date,
            end_date,
            optimization_window_size: std::time::Duration::from_secs((optimization_days * 24 * 3600) as u64),
            test_window_size: std::time::Duration::from_secs((test_days * 24 * 3600) as u64),
            max_parameters: 5,
            optimization_period_days: optimization_days,
            test_period_days: test_days,
            step_size_days: step_size, // Non-overlapping for efficiency
            min_trades_per_period: 5,
            parameters_to_optimize: vec![
                "risk_aversion".to_string(),
                "order_size".to_string(),
                "window_ms".to_string(),
            ],
            optimization_metric: "sharpe_ratio".to_string(),
        };
        
        log_info!(BACKTEST_LOGGER, "Walk-forward config: {} days data, opt={}d test={}d step={}d", 
            data_duration_days, optimization_days, test_days, step_size);
        
        let analyzer = WalkForwardAnalyzer::new(wf_config.clone(), self.config.clone());
        let windows = analyzer.generate_windows();
        
        if windows.is_empty() {
            return Ok(WalkForwardSummary {
                periods_tested: 0,
                average_performance: 0.0,
                consistency_score: 0.0,
                out_of_sample_degradation: 0.0,
            });
        }
        
        // Use full data per window - no sampling
        // Market making requires contiguous data to properly simulate fills
        let total_windows = windows.len();
        log_info!(BACKTEST_LOGGER, "Walk-forward: {} windows, FULL DATA per window (no sampling)", total_windows);
        
        // Prepare window data in parallel using Rayon
        use rayon::prelude::*;
        
        let window_datasets: Vec<(usize, Vec<MarketData>)> = windows.par_iter()
            .enumerate()
            .filter_map(|(window_idx, window)| {
                // Split market data into test sets only - use ALL data in window
                let test_data: Vec<_> = market_data.iter()
                    .filter(|md| {
                        #[cfg(feature = "postgres")]
                        let ts = match md {
                            MarketData::Candle(c) => c.timestamp,
                            MarketData::Trade(t) => t.timestamp,
                            MarketData::Snapshot(s) => s.timestamp,
                            MarketData::Order(o) => o.timestamp,
                        };
                        #[cfg(not(feature = "postgres"))]
                        let ts = match md {
                            MarketData::Candle(c) => c.timestamp,
                            MarketData::Trade(t) => t.timestamp,
                        };
                        ts >= window.test_window.start_date && ts < window.test_window.end_date
                    })
                    .cloned()
                    .collect();
                
                if test_data.is_empty() {
                    return None;
                }
                
                // NO SAMPLING - use full window data for accurate fill simulation
                Some((window_idx, test_data))
            })
            .collect();
        
        log_info!(BACKTEST_LOGGER, "Walk-forward: prepared {} valid windows", window_datasets.len());
        
        // Log window sizes for visibility
        for (idx, data) in &window_datasets {
            log_info!(BACKTEST_LOGGER, "Walk-forward window {} has {} events", idx + 1, data.len());
        }
        
        // Process windows sequentially but with pre-filtered data
        // (Full async parallelism would require cloning strategy state per window)
        let mut out_of_sample_sharpes = Vec::with_capacity(window_datasets.len());
        
        for (window_idx, window_data) in window_datasets {
            let window_start = Instant::now();
            
            // Report progress
            if let Some(ref callback) = self.progress_callback {
                callback("walk_forward", window_idx + 1, total_windows);
            }
            
            log_info!(BACKTEST_LOGGER, "Walk-forward window {}/{}: {} events - STARTING", 
                window_idx + 1, total_windows, window_data.len());
            
            // Test the ALREADY-OPTIMIZED parameters on this time window
            // This validates if parameters are robust across different market conditions
            let test_result = self.run_core_simulation(
                strategy_manager,
                orderbook,
                portfolio,
                risk_manager,
                &window_data,
            ).await?;
            
            log_info!(BACKTEST_LOGGER, "Walk-forward window {}/{} complete: {:.1}s, sharpe={:.3}, trades={}", 
                window_idx + 1, total_windows, window_start.elapsed().as_secs_f64(),
                test_result.sharpe_ratio.unwrap_or(0.0), test_result.num_trades);
            
            if let Some(test_sharpe) = test_result.sharpe_ratio {
                out_of_sample_sharpes.push(test_sharpe);
            }
        }
        
        let periods_tested = out_of_sample_sharpes.len();
        if periods_tested == 0 {
            return Ok(WalkForwardSummary {
                periods_tested: 0,
                average_performance: 0.0,
                consistency_score: 0.0,
                out_of_sample_degradation: 0.0,
            });
        }
        
        let avg_out_sample = out_of_sample_sharpes.iter().sum::<f64>() / out_of_sample_sharpes.len() as f64;
        
        // Calculate consistency (lower std dev = higher consistency)
        let mean = avg_out_sample;
        let variance = out_of_sample_sharpes.iter()
            .map(|s| (s - mean).powi(2))
            .sum::<f64>() / out_of_sample_sharpes.len() as f64;
        let std_dev = variance.sqrt();
        let consistency_score = (1.0 - (std_dev / (mean.abs() + 1e-6))).max(0.0).min(1.0);
        
        // Measure parameter stability across time windows (lower = more stable)
        // High std dev relative to mean indicates parameters don't generalize well
        let degradation = (std_dev / (mean.abs() + 1e-6)).min(1.0);
        
        Ok(WalkForwardSummary {
            periods_tested,
            average_performance: avg_out_sample,
            consistency_score,
            out_of_sample_degradation: degradation,
        })
    }
    
    /// Generate performance-based recommendations
    fn generate_recommendations(&self, result: &BacktestResult, _mc_result: &Option<monte_carlo::MonteCarloResult>) -> Vec<String> {
        let mut recommendations = Vec::with_capacity(4);  // Typical number of recommendations
        
        if let Some(sharpe) = result.sharpe_ratio {
            if sharpe < 1.0 {
                recommendations.push("Consider adjusting position sizing or risk parameters".to_string());
            } else if sharpe > 2.0 {
                recommendations.push("Strong performance - consider increasing position size".to_string());
            }
        }
        
        recommendations
    }
    
    /// Calculate data quality score for the market data
    fn calculate_data_quality_score(&self, market_data: &[MarketData]) -> f64 {
        if market_data.is_empty() {
            return 0.0;
        }
        
        let mut score: f64 = 10.0;
        
        // Check for sufficient data points
        if market_data.len() < 100 {
            score -= 2.0;
        }
        
        // Analyze gaps in the data
        let gap_config = &self.config.data.gap_detection;
        if gap_config.enabled && market_data.len() >= 2 {
            let gaps = self.detect_data_gaps(market_data);
            
            // Penalize based on number and severity of gaps
            let severe_gaps = gaps.iter().filter(|g| matches!(g.severity, crate::types::GapSeverity::Severe | crate::types::GapSeverity::Critical)).count();
            let moderate_gaps = gaps.iter().filter(|g| g.severity == crate::types::GapSeverity::Moderate).count();
            
            score -= (severe_gaps as f64 * 0.5).min(3.0);
            score -= (moderate_gaps as f64 * 0.2).min(2.0);
            
            // Penalize high gap ratio
            if let Some(first_ts) = self.get_timestamp(&market_data[0]) {
                if let Some(last_ts) = self.get_timestamp(&market_data[market_data.len() - 1]) {
                    let total_duration = (last_ts - first_ts).num_seconds();
                    let gap_duration: i64 = gaps.iter().map(|g| g.duration_seconds).sum();
                    let gap_ratio = gap_duration as f64 / total_duration as f64;
                    
                    if gap_ratio > 0.2 {
                        score -= 2.0;
                    } else if gap_ratio > 0.1 {
                        score -= 1.0;
                    }
                }
            }
        }
        
        score.max(0.0).min(10.0)
    }
    
    /// Detect gaps in market data
    fn detect_data_gaps(&self, market_data: &[MarketData]) -> Vec<crate::types::DataGap> {
        let mut gaps = Vec::with_capacity(50);  // Pre-allocate for typical gap count
        let gap_config = &self.config.data.gap_detection;
        
        if !gap_config.enabled || market_data.len() < 2 {
            return gaps;
        }
        
        let threshold_seconds = (gap_config.expected_frequency_seconds as f64 * gap_config.gap_threshold_multiplier) as i64;
        
        for i in 1..market_data.len() {
            if let (Some(prev_ts), Some(curr_ts)) = (
                self.get_timestamp(&market_data[i - 1]),
                self.get_timestamp(&market_data[i])
            ) {
                let gap_duration = (curr_ts - prev_ts).num_seconds();
                
                if gap_duration > threshold_seconds {
                    let severity = if gap_duration > gap_config.expected_frequency_seconds * 10 {
                        crate::types::GapSeverity::Critical
                    } else if gap_duration > gap_config.expected_frequency_seconds * 5 {
                        crate::types::GapSeverity::Severe
                    } else if gap_duration > gap_config.expected_frequency_seconds * 2 {
                        crate::types::GapSeverity::Moderate
                    } else {
                        crate::types::GapSeverity::Minor
                    };
                    
                    // Log warnings for significant gaps
                    if gap_duration > gap_config.warning_threshold_seconds {
                        log_warn!(BACKTEST_LOGGER, "Data gap detected: {:.1} minutes ({:?})", 
                            gap_duration as f64 / 60.0, &severity);
                    }
                    
                    gaps.push(crate::types::DataGap {
                        start_time: prev_ts,
                        end_time: curr_ts,
                        duration_seconds: gap_duration,
                        severity,
                    });
                }
            }
        }
        
        // Check for critical gaps that should fail the backtest
        if let Some(critical_threshold) = gap_config.critical_threshold_seconds {
            let critical_gaps: Vec<_> = gaps.iter()
                .filter(|g| g.duration_seconds > critical_threshold)
                .collect();
            
            if !critical_gaps.is_empty() {
                log_error!(BACKTEST_LOGGER, "CRITICAL: {} gap(s) exceed {}s threshold", 
                    critical_gaps.len(), critical_threshold);
            }
        }
        
        gaps
    }
    
    /// Generate comprehensive data quality report
    #[cfg(feature = "postgres")]
    fn generate_data_quality_report(&self, market_data: &[MarketData], _historical_orders: &[HistoricalOrder]) -> crate::types::DataQualityReport {
        let gaps = self.detect_data_gaps(market_data);
        let total_data_points = market_data.len();
        
        let (time_span, avg_gap, max_gap, gap_ratio) = if let (Some(first), Some(last)) = (
            self.get_timestamp(&market_data.first().cloned().unwrap_or_else(|| market_data[0].clone())),
            self.get_timestamp(&market_data.last().cloned().unwrap_or_else(|| market_data[0].clone()))
        ) {
            let time_span = (last - first).num_seconds();
            let total_gap_time: i64 = gaps.iter().map(|g| g.duration_seconds).sum();
            let avg_gap = if !gaps.is_empty() {
                total_gap_time as f64 / gaps.len() as f64
            } else {
                0.0
            };
            let max_gap = gaps.iter().map(|g| g.duration_seconds).max().unwrap_or(0);
            let gap_ratio = if time_span > 0 {
                total_gap_time as f64 / time_span as f64
            } else {
                0.0
            };
            (time_span, avg_gap, max_gap, gap_ratio)
        } else {
            (0, 0.0, 0, 0.0)
        };
        
        let overall_score = self.calculate_data_quality_score(market_data);
        let data_completeness_score = ((1.0 - gap_ratio) * 10.0).max(0.0).min(10.0);
        let timestamp_consistency_score = if gaps.is_empty() { 10.0 } else { 
            (10.0 - (gaps.len() as f64 * 0.1)).max(0.0) 
        };
        
        // Analyze order stream quality if configured
        // TODO: Re-enable once feature gate issue is resolved
        let order_quality = None;
        // let order_quality = if let Some(order_config) = &self.config.data.gap_detection.order_detection {
        //     if order_config.enabled && !historical_orders.is_empty() {
        //         Some(self.analyze_order_stream(historical_orders, order_config))
        //     } else {
        //         None
        //     }
        // } else {
        //     None
        // };
        
        crate::types::DataQualityReport {
            overall_score,
            total_data_points,
            time_span_seconds: time_span,
            gaps_detected: gaps.clone(),
            average_gap_seconds: avg_gap,
            max_gap_seconds: max_gap,
            gap_ratio,
            outliers_detected: 0, // TODO: implement outlier detection
            data_completeness_score,
            timestamp_consistency_score,
            order_quality,
        }
    }
    
    /// Generate comprehensive data quality report (non-postgres version)
    #[cfg(not(feature = "postgres"))]
    fn generate_data_quality_report(&self, market_data: &[MarketData]) -> crate::types::DataQualityReport {
        let gaps = self.detect_data_gaps(market_data);
        let total_data_points = market_data.len();
        
        let (time_span, avg_gap, max_gap, gap_ratio) = if let (Some(first), Some(last)) = (
            self.get_timestamp(&market_data.first().cloned().unwrap_or_else(|| market_data[0].clone())),
            self.get_timestamp(&market_data.last().cloned().unwrap_or_else(|| market_data[0].clone()))
        ) {
            let time_span = (last - first).num_seconds();
            let total_gap_time: i64 = gaps.iter().map(|g| g.duration_seconds).sum();
            let avg_gap = if !gaps.is_empty() {
                total_gap_time as f64 / gaps.len() as f64
            } else {
                0.0
            };
            let max_gap = gaps.iter().map(|g| g.duration_seconds).max().unwrap_or(0);
            let gap_ratio = if time_span > 0 {
                total_gap_time as f64 / time_span as f64
            } else {
                0.0
            };
            (time_span, avg_gap, max_gap, gap_ratio)
        } else {
            (0, 0.0, 0, 0.0)
        };
        
        let overall_score = self.calculate_data_quality_score(market_data);
        let data_completeness_score = ((1.0 - gap_ratio) * 10.0).max(0.0).min(10.0);
        let timestamp_consistency_score = if gaps.is_empty() { 10.0 } else { 
            (10.0 - (gaps.len() as f64 * 0.1)).max(0.0) 
        };
        
        crate::types::DataQualityReport {
            overall_score,
            total_data_points,
            time_span_seconds: time_span,
            gaps_detected: gaps.clone(),
            average_gap_seconds: avg_gap,
            max_gap_seconds: max_gap,
            gap_ratio,
            outliers_detected: 0,
            data_completeness_score,
            timestamp_consistency_score,
            order_quality: None,
        }
    }
    
    /// Helper to extract timestamp from MarketData
    #[cfg(feature = "postgres")]
    fn get_timestamp(&self, data: &MarketData) -> Option<chrono::DateTime<chrono::Utc>> {
        match data {
            MarketData::Candle(c) => Some(c.timestamp),
            MarketData::Trade(t) => Some(t.timestamp),
            MarketData::Snapshot(s) => Some(s.timestamp),
            MarketData::Order(o) => Some(o.timestamp),
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    fn get_timestamp(&self, data: &MarketData) -> Option<chrono::DateTime<chrono::Utc>> {
        match data {
            MarketData::Candle(c) => Some(c.timestamp),
            MarketData::Trade(t) => Some(t.timestamp),
        }
    }
    
    /// Apply gap filling strategy to market data
    fn apply_gap_filling(&self, market_data: &[MarketData]) -> Result<Vec<MarketData>> {
        use config::GapFillStrategy;
        use chrono::Duration;
        
        let gap_config = &self.config.data.gap_detection;
        
        match gap_config.fill_strategy {
            GapFillStrategy::None => {
                // No filling - return as-is
                Ok(market_data.to_vec())
            }
            GapFillStrategy::ForwardFill => {
                log_info!(BACKTEST_LOGGER, "Applying forward fill strategy");
                let mut filled_data = Vec::new();
                
                for (i, data) in market_data.iter().enumerate() {
                    filled_data.push(data.clone());
                    
                    // Check if there's a gap to the next data point
                    if i < market_data.len() - 1 {
                        if let (Some(current_ts), Some(next_ts)) = (
                            self.get_timestamp(data),
                            self.get_timestamp(&market_data[i + 1])
                        ) {
                            let gap = next_ts - current_ts;
                            let gap_seconds = gap.num_seconds();
                            let threshold = (gap_config.expected_frequency_seconds as f64 * gap_config.gap_threshold_multiplier) as i64;
                            
                            if gap_seconds > threshold {
                                // Fill the gap with forward-filled data
                                let num_fills = (gap_seconds / gap_config.expected_frequency_seconds) - 1;
                                for j in 1..=num_fills {
                                    let fill_timestamp = current_ts + Duration::seconds(gap_config.expected_frequency_seconds * j);
                                    let filled_point = self.create_filled_data_point(data, fill_timestamp);
                                    filled_data.push(filled_point);
                                }
                            }
                        }
                    }
                }
                
                Ok(filled_data)
            }
            GapFillStrategy::LinearInterpolation => {
                log_info!(BACKTEST_LOGGER, "Applying linear interpolation strategy");
                let mut filled_data = Vec::new();
                
                for (i, data) in market_data.iter().enumerate() {
                    filled_data.push(data.clone());
                    
                    if i < market_data.len() - 1 {
                        if let (Some(current_ts), Some(next_ts)) = (
                            self.get_timestamp(data),
                            self.get_timestamp(&market_data[i + 1])
                        ) {
                            let gap = next_ts - current_ts;
                            let gap_seconds = gap.num_seconds();
                            let threshold = (gap_config.expected_frequency_seconds as f64 * gap_config.gap_threshold_multiplier) as i64;
                            
                            if gap_seconds > threshold {
                                // Interpolate between current and next data points
                                let num_fills = (gap_seconds / gap_config.expected_frequency_seconds) - 1;
                                for j in 1..=num_fills {
                                    let fill_timestamp = current_ts + Duration::seconds(gap_config.expected_frequency_seconds * j);
                                    let ratio = j as f64 / (num_fills + 1) as f64;
                                    let filled_point = self.create_interpolated_data_point(data, &market_data[i + 1], fill_timestamp, ratio);
                                    filled_data.push(filled_point);
                                }
                            }
                        }
                    }
                }
                
                Ok(filled_data)
            }
            GapFillStrategy::ExcludeGaps => {
                log_info!(BACKTEST_LOGGER, "Excluding data around gaps");
                let gaps = self.detect_data_gaps(market_data);
                let mut filtered_data = Vec::new();
                
                for data in market_data {
                    if let Some(ts) = self.get_timestamp(data) {
                        // Check if this data point is within a gap period
                        let in_gap = gaps.iter().any(|gap| ts >= gap.start_time && ts <= gap.end_time);
                        if !in_gap {
                            filtered_data.push(data.clone());
                        }
                    }
                }
                
                Ok(filtered_data)
            }
        }
    }
    
    /// Create a forward-filled data point
    #[cfg(feature = "postgres")]
    fn create_filled_data_point(&self, source: &MarketData, timestamp: chrono::DateTime<chrono::Utc>) -> MarketData {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        match source {
            MarketData::Candle(c) => {
                let mut filled = c.clone();
                filled.timestamp = timestamp;
                // Keep same OHLCV but mark it as a filled candle
                filled.volume = BigDecimal::from_str("0").unwrap(); // Zero volume indicates synthetic data
                MarketData::Candle(filled)
            }
            MarketData::Trade(t) => {
                let mut filled = t.clone();
                filled.timestamp = timestamp;
                filled.quantity = BigDecimal::from_str("0").unwrap(); // Zero quantity indicates synthetic data
                MarketData::Trade(filled)
            }
            MarketData::Snapshot(s) => {
                let mut filled = s.clone();
                filled.timestamp = timestamp;
                MarketData::Snapshot(filled)
            }
            MarketData::Order(o) => {
                let mut filled = o.clone();
                filled.timestamp = timestamp;
                MarketData::Order(filled)
            }
        }
    }
    
    #[cfg(not(feature = "postgres"))]
    fn create_filled_data_point(&self, source: &MarketData, timestamp: chrono::DateTime<chrono::Utc>) -> MarketData {
        use bigdecimal::BigDecimal;
        use std::str::FromStr;
        
        match source {
            MarketData::Candle(c) => {
                let mut filled = c.clone();
                filled.timestamp = timestamp;
                // Keep same OHLCV but mark it as a filled candle
                filled.volume = BigDecimal::from_str("0").unwrap(); // Zero volume indicates synthetic data
                MarketData::Candle(filled)
            }
            MarketData::Trade(t) => {
                let mut filled = t.clone();
                filled.timestamp = timestamp;
                filled.quantity = BigDecimal::from_str("0").unwrap(); // Zero quantity indicates synthetic data
                MarketData::Trade(filled)
            }
        }
    }
    
    /// Create an interpolated data point between two data points
    fn create_interpolated_data_point(&self, start: &MarketData, end: &MarketData, timestamp: chrono::DateTime<chrono::Utc>, ratio: f64) -> MarketData {
        use bigdecimal::{BigDecimal, ToPrimitive};
        use std::str::FromStr;
        
        match (start, end) {
            (MarketData::Candle(c1), MarketData::Candle(c2)) => {
                // Linear interpolation for OHLC prices
                let open_interp = {
                    let v1 = c1.open.to_f64().unwrap_or(0.0);
                    let v2 = c2.open.to_f64().unwrap_or(0.0);
                    BigDecimal::from_str(&(v1 + (v2 - v1) * ratio).to_string()).unwrap()
                };
                let close_interp = {
                    let v1 = c1.close.to_f64().unwrap_or(0.0);
                    let v2 = c2.close.to_f64().unwrap_or(0.0);
                    BigDecimal::from_str(&(v1 + (v2 - v1) * ratio).to_string()).unwrap()
                };
                
                let interpolated = dataloader::Candle {
                    timestamp,
                    symbol: c1.symbol.clone(),
                    exchange: c1.exchange.clone(),
                    open: open_interp,
                    high: if c1.high > c2.high { c1.high.clone() } else { c2.high.clone() }, // Conservative: use max high
                    low: if c1.low < c2.low { c1.low.clone() } else { c2.low.clone() },     // Conservative: use min low
                    close: close_interp,
                    volume: BigDecimal::from_str("0").unwrap(), // Zero volume indicates synthetic data
                    trade_count: 0,
                };
                MarketData::Candle(interpolated)
            }
            (MarketData::Trade(t1), MarketData::Trade(t2)) => {
                let v1 = t1.price_f64;
                let v2 = t2.price_f64;
                let price_f64 = v1 + (v2 - v1) * ratio;
                let price_interp = BigDecimal::from_str(&price_f64.to_string()).unwrap();
                
                let interpolated = dataloader::TradeData {
                    timestamp,
                    symbol: t1.symbol.clone(),
                    exchange: t1.exchange.clone(),
                    price: price_interp,
                    quantity: BigDecimal::from_str("0").unwrap(), // Zero quantity indicates synthetic data
                    side: t1.side.clone(),
                    trade_id: format!("synthetic_{}", timestamp.timestamp()),
                    price_f64,
                    quantity_f64: 0.0,
                    value_f64: 0.0,
                };
                MarketData::Trade(interpolated)
            }
            _ => {
                // For mismatched types or unsupported types, fall back to forward fill
                self.create_filled_data_point(start, timestamp)
            }
        }
    }
}

impl BacktestResults {
    /// Print comprehensive summary of backtest results
    pub fn print_summary(&self) {
        use crate::logging_facade::BACKTEST_LOGGER;
        
        BACKTEST_LOGGER.info_sync("=== BACKTEST RESULTS SUMMARY ===".to_string());
        
        // Core results
        let mut core_msg = String::from("CORE METRICS: ");
        if let Some(sharpe) = self.core.sharpe_ratio {
            core_msg.push_str(&format!("Sharpe: {:.2}, ", sharpe));
        }
        if let Some(pf) = self.core.profit_factor {
            core_msg.push_str(&format!("PF: {:.2}, ", pf));
        }
        if let Some(wr) = self.core.win_rate {
            core_msg.push_str(&format!("Win Rate: {:.1}%", wr * 100.0));
        }
        BACKTEST_LOGGER.info_sync(core_msg);
        
        // Monte Carlo results
        if let Some(mc) = &self.monte_carlo {
            BACKTEST_LOGGER.info_sync(format!("MONTE CARLO ({} simulations): Mean Sharpe: {:.2}", 
                mc.num_runs, mc.mean_metrics.sharpe_ratio));
        }
        
        // Walk-forward results
        if let Some(wf) = &self.walk_forward {
            BACKTEST_LOGGER.info_sync(format!("WALK-FORWARD: Periods: {}, Avg Perf: {:.1}%, Consistency: {:.1}/10",
                wf.periods_tested, wf.average_performance * 100.0, wf.consistency_score * 10.0));
        }
        
        // Metadata
        BACKTEST_LOGGER.info_sync(format!("METADATA: Mode: {:?}, Escalated: {}, Runtime: {:.1}s, Data Quality: {:.1}/10",
            self.metadata.analysis_mode, self.metadata.mode_escalated, 
            self.metadata.total_runtime_seconds, self.metadata.data_quality_score));
        
        // Data Quality Report
        if let Some(dq) = &self.core.data_quality {
            let mut dq_msg = format!("DATA QUALITY: Score: {:.1}/10, Points: {}, Time Span: {:.1}h, Gaps: {}",
                dq.overall_score, dq.total_data_points, 
                dq.time_span_seconds as f64 / 3600.0, dq.gaps_detected.len());
            if dq.max_gap_seconds > 0 {
                dq_msg.push_str(&format!(", Max Gap: {:.1}m, Gap Ratio: {:.1}%", 
                    dq.max_gap_seconds as f64 / 60.0, dq.gap_ratio * 100.0));
            }
            BACKTEST_LOGGER.info_sync(dq_msg);
        }
        
        // Transaction Cost Analysis
        if let Some(tc) = &self.core.transaction_costs {
            BACKTEST_LOGGER.info_sync(format!("TRANSACTION COSTS: Total: ${:.2}, Commission: ${:.2}, Slippage: ${:.2}, Impact: ${:.2}, Costs/PnL: {:.1}%",
                tc.total_transaction_costs, tc.total_commission, tc.total_slippage, 
                tc.total_market_impact, tc.costs_as_percentage_of_pnl));
        }
        
        // Market Impact Analysis
        if let Some(mi) = &self.core.market_impact {
            if mi.trades_with_impact > 0 {
                let mut mi_msg = format!("MARKET IMPACT: Trades: {}, Avg: {:.2}bps, Max: {:.2}bps, Total Cost: ${:.2}",
                    mi.trades_with_impact, mi.avg_impact_bps, mi.max_impact_bps, mi.total_impact_cost);
                if !mi.liquidity_warnings.is_empty() {
                    mi_msg.push_str(&format!(", Liquidity Warnings: {}", mi.liquidity_warnings.len()));
                }
                BACKTEST_LOGGER.info_sync(mi_msg);
            }
        }
        
        if !self.metadata.triggers_fired.is_empty() {
            BACKTEST_LOGGER.info_sync(format!("TRIGGERS FIRED: {}", self.metadata.triggers_fired.join(", ")));
        }
        
        if !self.metadata.recommendations.is_empty() {
            BACKTEST_LOGGER.info_sync(format!("RECOMMENDATIONS: {}", self.metadata.recommendations.join("; ")));
        }
        
        BACKTEST_LOGGER.info_sync("=== END RESULTS SUMMARY ===".to_string());
    }
    
    /// Check if results meet export criteria
    pub fn should_export(&self, export_config: &ExportConfig) -> (bool, Vec<String>) {
        let mut reasons = Vec::new();
        let mut should_export = true;
        
        // Check core criteria
        if let Some(sharpe) = self.core.sharpe_ratio {
            if sharpe < export_config.min_sharpe_ratio {
                should_export = false;
                reasons.push(format!("Sharpe ratio {:.2} below minimum {:.2}", 
                    sharpe, export_config.min_sharpe_ratio));
            }
        }
        
        if let Some(pf) = self.core.profit_factor {
            if pf < export_config.min_profit_factor {
                should_export = false;
                reasons.push(format!("Profit factor {:.2} below minimum {:.2}", 
                    pf, export_config.min_profit_factor));
            }
        }
        
        // Enhanced criteria
        if self.metadata.data_quality_score < 7.0 {
            should_export = false;
            reasons.push("Data quality score too low".to_string());
        }
        
        if should_export {
            reasons.push("All export criteria met".to_string());
        }
        
        (should_export, reasons)
    }
    
    /// Analyze historical order stream for market making strategies
    /// Detects missing orders, unpaired quotes, and inconsistent quote intervals
    #[cfg(feature = "postgres")]
    fn _analyze_order_stream(&self, orders: &[HistoricalOrder], config: &config::OrderDetectionConfig) -> crate::types::OrderStreamQuality {
        use std::collections::HashMap;
        
        if orders.is_empty() {
            return crate::types::OrderStreamQuality {
                total_orders: 0,
                order_gaps: vec![],
                unpaired_quotes: vec![],
                expected_quote_interval_seconds: config.expected_quote_interval_ms,
                avg_quote_interval_seconds: 0.0,
                quote_consistency_score: 0.0,
                missing_quote_cycles: 0,
                bid_ask_pair_ratio: 0.0,
            };
        }
        
        let mut order_gaps = Vec::new();
        let mut intervals = Vec::new();
        let threshold_ms = (config.expected_quote_interval_ms as f64 * 1.5) as i64;
        let mut missing_cycles = 0u64;
        
        // Detect gaps in order stream (using millisecond precision)
        for i in 1..orders.len() {
            let prev_ts = *orders[i - 1].get_timestamp();
            let curr_ts = *orders[i].get_timestamp();
            let gap_duration_ms = (curr_ts - prev_ts).num_milliseconds();
            
            intervals.push(gap_duration_ms);
            
            if gap_duration_ms > threshold_ms {
                let missing = (gap_duration_ms / config.expected_quote_interval_ms as i64) - 1;
                missing_cycles += missing.max(0) as u64;
                
                let severity = if gap_duration_ms > (config.expected_quote_interval_ms as i64 * 10) {
                    crate::types::GapSeverity::Critical
                } else if gap_duration_ms > (config.expected_quote_interval_ms as i64 * 5) {
                    crate::types::GapSeverity::Severe
                } else if gap_duration_ms > (config.expected_quote_interval_ms as i64 * 2) {
                    crate::types::GapSeverity::Moderate
                } else {
                    crate::types::GapSeverity::Minor
                };
                
                if missing > config.warning_missing_cycles as i64 {
                    println!("⚠️  Order gap detected: {} missing quote cycles ({:.1} ms)", 
                        missing, gap_duration_ms as f64);
                }
                
                order_gaps.push(crate::types::OrderGap {
                    start_time: prev_ts,
                    end_time: curr_ts,
                    duration_seconds: (gap_duration_ms / 1000),
                    missing_quote_cycles: missing as u64,
                    severity,
                });
            }
        }
        
        // Detect unpaired quotes (for market making)
        let mut unpaired = Vec::new();
        if config.require_paired_quotes {
            // Group orders by timestamp window (millisecond precision)
            let mut timestamp_groups: HashMap<i64, Vec<&HistoricalOrder>> = HashMap::new();
            let window_ms = config.pairing_window_ms as i64;
            
            for order in orders {
                let window_key = order.get_timestamp().timestamp_millis() / window_ms;
                timestamp_groups.entry(window_key).or_insert_with(Vec::new).push(order);
            }
            
            // Check each window for bid-ask pairs
            for (_window, orders_in_window) in &timestamp_groups {
                let mut bids = Vec::new();
                let mut asks = Vec::new();
                
                for order in orders_in_window.iter() {
                    match order.get_side() {
                        "BUY" | "bid" => bids.push(*order),
                        "SELL" | "ask" => asks.push(*order),
                        _ => {}
                    }
                }
                
                // Report unpaired quotes
                if bids.len() != asks.len() {
                    if bids.len() > asks.len() {
                        for bid in bids.iter().skip(asks.len()) {
                            unpaired.push(crate::types::UnpairedQuote {
                                timestamp: *bid.get_timestamp(),
                                order_id: bid.get_order_id().to_string(),
                                side: "bid".to_string(),
                                price: bid.price_level.clone(),
                            });
                        }
                    } else {
                        for ask in asks.iter().skip(bids.len()) {
                            unpaired.push(crate::types::UnpairedQuote {
                                timestamp: *ask.get_timestamp(),
                                order_id: ask.get_order_id().to_string(),
                                side: "ask".to_string(),
                                price: ask.price_level.clone(),
                            });
                        }
                    }
                }
            }
        }
        
        // Calculate metrics (millisecond precision)
        let avg_interval = if !intervals.is_empty() {
            intervals.iter().sum::<i64>() as f64 / intervals.len() as f64
        } else {
            0.0
        };
        
        let consistency_score = if !intervals.is_empty() {
            let variance: f64 = intervals.iter()
                .map(|&i| {
                    let diff = i as f64 - config.expected_quote_interval_ms as f64;
                    diff * diff
                })
                .sum::<f64>() / intervals.len() as f64;
            let std_dev = variance.sqrt();
            // Score based on coefficient of variation
            let cv = std_dev / config.expected_quote_interval_ms as f64;
            (10.0_f64 * (1.0 - cv.min(1.0))).max(0.0)
        } else {
            0.0
        };
        
        // Count bid/ask ratio
        let (bids, asks) = orders.iter().fold((0, 0), |(b, a), order| {
            match order.get_side() {
                "BUY" | "bid" => (b + 1, a),
                "SELL" | "ask" => (b, a + 1),
                _ => (b, a),
            }
        });
        let bid_ask_ratio = if asks > 0 { bids as f64 / asks as f64 } else { 0.0 };
        
        if !unpaired.is_empty() {
            println!("⚠️  Market making warning: {} unpaired quotes detected", unpaired.len());
        }
        if missing_cycles > 0 {
            println!("⚠️  Order stream gaps: {} missing quote cycles", missing_cycles);
        }
        
        crate::types::OrderStreamQuality {
            total_orders: orders.len(),
            order_gaps,
            unpaired_quotes: unpaired,
            expected_quote_interval_seconds: config.expected_quote_interval_ms,
            avg_quote_interval_seconds: avg_interval,
            quote_consistency_score: consistency_score,
            missing_quote_cycles: missing_cycles,
            bid_ask_pair_ratio: bid_ask_ratio,
        }
    }
}
