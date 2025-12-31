//! Job queue system for async backtest processing
//! 
//! Supports PostgreSQL backend
//! Workers poll the queue and execute backtests asynchronously.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Job status states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Custom fitness function weights for GA optimization
/// All values should be normalized to sum to 1.0
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FitnessWeights {
    #[serde(default = "default_sharpe_weight")]
    pub sharpe: f64,
    #[serde(default = "default_sortino_weight")]
    pub sortino: f64,
    #[serde(default = "default_profit_factor_weight")]
    pub profit_factor: f64,
    #[serde(default = "default_win_rate_weight")]
    pub win_rate: f64,
    #[serde(default = "default_net_profit_weight")]
    pub net_profit: f64,
    #[serde(default = "default_calmar_weight")]
    pub calmar: f64,
}

fn default_sharpe_weight() -> f64 { 0.10 }
fn default_sortino_weight() -> f64 { 0.15 }
fn default_profit_factor_weight() -> f64 { 0.15 }
fn default_win_rate_weight() -> f64 { 0.10 }
fn default_net_profit_weight() -> f64 { 0.35 }
fn default_calmar_weight() -> f64 { 0.15 }

impl FitnessWeights {
    /// Balanced preset (default)
    pub fn balanced() -> Self {
        Self { sharpe: 0.10, sortino: 0.15, profit_factor: 0.15, win_rate: 0.10, net_profit: 0.35, calmar: 0.15 }
    }
    
    /// Max Sharpe ratio focus
    pub fn max_sharpe() -> Self {
        Self { sharpe: 0.35, sortino: 0.25, profit_factor: 0.10, win_rate: 0.05, net_profit: 0.15, calmar: 0.10 }
    }
    
    /// Max profit focus
    pub fn max_profit() -> Self {
        Self { sharpe: 0.05, sortino: 0.10, profit_factor: 0.15, win_rate: 0.10, net_profit: 0.50, calmar: 0.10 }
    }
    
    /// Low drawdown focus
    pub fn low_drawdown() -> Self {
        Self { sharpe: 0.15, sortino: 0.15, profit_factor: 0.15, win_rate: 0.10, net_profit: 0.15, calmar: 0.30 }
    }
}

/// A single asset in a portfolio (symbol + exchange pair)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioAsset {
    pub symbol: String,
    pub exchange: String,
    /// Capital allocation weight (0.0 to 1.0)
    #[serde(default)]
    pub weight: Option<f64>,
}

/// Backtest job parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJobParams {
    /// Primary symbol (for single-symbol backtests)
    pub symbol: String,
    pub exchange: String,
    
    /// Portfolio assets for multi-symbol/multi-exchange backtests
    /// Each asset is a symbol+exchange pair with optional weight
    /// Allows same symbol on different exchanges (e.g., BTC on Kraken AND Coinbase)
    #[serde(default)]
    pub portfolio_assets: Option<Vec<PortfolioAsset>>,
    
    pub strategy_type: String,
    pub initial_capital: f64,
    
    // Legacy fields (deprecated, use portfolio_assets instead)
    /// Additional symbols for portfolio/multi-symbol backtests
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Per-symbol exchange mapping for portfolio mode
    #[serde(default)]
    pub symbol_exchanges: Option<Vec<String>>,
    // Portfolio allocation weights (legacy - use portfolio_assets.weight instead)
    #[serde(default)]
    pub allocation_weights: Option<Vec<f64>>,
    
    // Initial inventory configuration
    /// Percentage of initial_capital to hold as base asset (e.g., 0.5 = 50% in BTC, 50% in USD)
    #[serde(default = "default_initial_inventory_pct")]
    pub initial_inventory_pct: f64,
    
    /// Initial 30-day trading volume in USD (for fee tier calculations)
    #[serde(default = "default_initial_30d_volume")]
    pub initial_30d_volume: f64,
    
    // Optimization method
    #[serde(default = "default_optimization_method")]
    pub optimization_method: String, // "genetic" or "manual"
    
    // Genetic algorithm parameters
    #[serde(default = "default_population_size")]
    pub population_size: usize,
    #[serde(default = "default_generations")]
    pub generations: usize,
    
    // Walk-forward analysis parameters (used when optimization_method == "walkforward")
    #[serde(default)]
    pub walkforward_optimization_days: Option<i64>, // Length of optimization window in days (default: 180)
    #[serde(default)]
    pub walkforward_test_days: Option<i64>, // Length of test window in days (default: 30)
    #[serde(default)]
    pub walkforward_step_days: Option<i64>, // Step size between windows in days (default: 30)
    
    // Manual strategy parameters (used when optimization_method == "manual")
    #[serde(default = "default_risk_aversion")]
    pub risk_aversion: f64,
    #[serde(default = "default_inventory_target")]
    pub inventory_target: f64,
    #[serde(default = "default_order_size")]
    pub order_size: f64,
    /// Order size as percentage of capital (e.g., 0.02 = 2% of capital per trade)
    #[serde(default = "default_order_size_pct")]
    pub order_size_pct: f64,
    #[serde(default = "default_window_ms")]
    pub window_ms: u64,
    #[serde(default = "default_min_quote_lifetime")]
    pub min_quote_lifetime_ms: u64,
    #[serde(default)]
    pub fee_rate: f64, // Will be auto-populated from exchange
    #[serde(default = "default_volatility_cap")]
    pub volatility_cap: f64,
    #[serde(default = "default_time_horizon")]
    pub time_horizon_minutes: u64,
    #[serde(default = "default_market_depth_k")]
    pub market_depth_k: f64,
    
    // Phase 2 adjustment factors
    #[serde(default = "default_volatility_adjustment")]
    pub volatility_adjustment_factor: f64,
    #[serde(default = "default_inventory_penalty")]
    pub inventory_penalty_factor: f64,
    #[serde(default = "default_trend_sensitivity")]
    pub trend_sensitivity: f64,
    #[serde(default = "default_flow_sensitivity")]
    pub flow_sensitivity: f64,
    
    // Fee and adverse selection parameters
    /// Multiplier for exchange fee to set minimum spread (e.g., 4.0 = 4x fee as min spread)
    #[serde(default = "default_fee_spread_multiplier")]
    pub fee_spread_multiplier: f64,
    /// Sensitivity to adverse selection (0.0 = ignore, 1.0 = fully adjust for order flow)
    #[serde(default = "default_adverse_selection_sensitivity")]
    pub adverse_selection_sensitivity: f64,
    
    // Exit and trading control parameters
    #[serde(default = "default_take_profit_pct")]
    pub take_profit_pct: f64,
    #[serde(default = "default_stop_loss_pct")]
    pub stop_loss_pct: f64,
    #[serde(default = "default_min_spread_bps")]
    pub min_spread_bps: f64,
    #[serde(default = "default_max_trades_per_minute")]
    pub max_trades_per_minute: f64,
    
    // Monte Carlo configuration
    #[serde(default = "default_monte_carlo_enabled")]
    pub monte_carlo_enabled: bool,
    #[serde(default = "default_monte_carlo_runs")]
    pub monte_carlo_runs: usize,
    
    // Walk-Forward configuration
    #[serde(default = "default_walk_forward_enabled")]
    pub walk_forward_enabled: bool,
    
    // Custom fitness weights for GA optimization (normalized to sum to 1.0)
    // If None, uses default balanced weights
    #[serde(default)]
    pub fitness_weights: Option<FitnessWeights>,
    
    // Data split configuration for train/test/validate
    /// Fraction of data used for GA training (default 0.6 = 60%)
    #[serde(default = "default_train_ratio")]
    pub train_ratio: f64,
    /// Fraction of data used for walk-forward testing (default 0.2 = 20%)
    #[serde(default = "default_test_ratio")]
    pub test_ratio: f64,
    /// Fraction of data used for final validation (default 0.2 = 20%)
    #[serde(default = "default_validate_ratio")]
    pub validate_ratio: f64,
    
    // Profitability criteria for auto-saving strategies to database
    #[serde(default = "default_auto_save_enabled")]
    pub auto_save_profitable: bool,
    #[serde(default = "default_min_sharpe_ratio")]
    pub min_sharpe_ratio: f64,
    #[serde(default = "default_min_profit_factor")]
    pub min_profit_factor: f64,
    #[serde(default = "default_max_drawdown")]
    pub max_drawdown: f64,
    #[serde(default = "default_min_win_rate")]
    pub min_win_rate: f64,
    #[serde(default = "default_min_trades")]
    pub min_trades: usize,
    #[serde(default)]
    pub min_sortino_ratio: Option<f64>,
    #[serde(default)]
    pub min_calmar_ratio: Option<f64>,
    #[serde(default)]
    pub min_total_pnl: Option<f64>,
    
    // Smart Order Routing configuration
    /// Routing mode: "single", "best_venue", "multi_venue", "arbitrage"
    #[serde(default)]
    pub routing_mode: Option<String>,
    /// List of exchanges to route orders to (for multi-venue routing)
    #[serde(default)]
    pub routing_exchanges: Option<Vec<String>>,
    /// Maximum venues per order (for multi-venue mode)
    #[serde(default)]
    pub max_venues_per_order: Option<usize>,
    /// Minimum allocation fraction per venue (e.g., 0.1 = 10%)
    #[serde(default)]
    pub min_venue_allocation: Option<f64>,
    /// Execution style: "aggressive", "passive", "balanced", "twap", "vwap"
    #[serde(default)]
    pub execution_style: Option<String>,
    /// TWAP/VWAP duration in milliseconds
    #[serde(default)]
    pub execution_duration_ms: Option<u64>,
    /// Minimum arbitrage profit in basis points (for arbitrage mode)
    #[serde(default)]
    pub min_arb_profit_bps: Option<f64>,
    
    // Fill model configuration for realistic backtesting
    /// Fill model realism level: "optimistic", "standard", "realistic", "pessimistic"
    /// Default is "realistic" for conservative estimates
    #[serde(default = "default_fill_model")]
    pub fill_model: String,
    
    // Auto-populated fields
    #[serde(default)]
    pub commission_rate: f64, // Auto-populated from exchange
    #[serde(default)]
    pub start_date: Option<String>, // Auto-populated from database
    #[serde(default)]
    pub end_date: Option<String>, // Auto-populated from database
}

// Default functions for serde
fn default_initial_inventory_pct() -> f64 { 0.0 }  // Start with 0% inventory to avoid directional exposure
fn default_initial_30d_volume() -> f64 { 0.0 }  // 0 = use exchange default fee tier
fn default_optimization_method() -> String { "genetic".to_string() }
fn default_population_size() -> usize { 30 }  // Reduced from 50 for faster optimization
fn default_generations() -> usize { 15 }      // Reduced from 20, early stopping handles convergence
fn default_risk_aversion() -> f64 { 0.1 }  // Lower = tighter spreads, more fills
fn default_inventory_target() -> f64 { 0.5 }
fn default_order_size() -> f64 { 0.01 }
fn default_order_size_pct() -> f64 { 0.02 }  // 2% of capital per trade
fn default_window_ms() -> u64 { 5000 }
fn default_min_quote_lifetime() -> u64 { 100 }
fn default_volatility_cap() -> f64 { 0.05 }
fn default_time_horizon() -> u64 { 60 }
fn default_market_depth_k() -> f64 { 1.5 }
fn default_volatility_adjustment() -> f64 { 0.5 }
fn default_inventory_penalty() -> f64 { 0.3 }
fn default_trend_sensitivity() -> f64 { 0.2 }
fn default_flow_sensitivity() -> f64 { -0.1 }
fn default_fee_spread_multiplier() -> f64 { 2.5 }  // 2.5x fee = tighter spreads for more fills (2x is breakeven)
fn default_adverse_selection_sensitivity() -> f64 { 0.5 }  // Moderate adverse selection adjustment
fn default_take_profit_pct() -> f64 { 0.0 }  // Disabled for MM (profit from spread, not direction)
fn default_stop_loss_pct() -> f64 { 0.0 }    // Disabled for MM (use inventory limits instead)
fn default_min_spread_bps() -> f64 { 15.0 }    // 15 basis points minimum spread
fn default_max_trades_per_minute() -> f64 { 30.0 }  // 30 trades per minute max
fn default_monte_carlo_enabled() -> bool { true }
fn default_monte_carlo_runs() -> usize { 1000 }
fn default_walk_forward_enabled() -> bool { true } // Re-enabled - now uses sampled data (200K events/window max)
fn default_train_ratio() -> f64 { 0.6 }  // 60% for GA optimization
fn default_test_ratio() -> f64 { 0.2 }   // 20% for walk-forward analysis
fn default_validate_ratio() -> f64 { 0.2 }  // 20% for final held-out validation
fn default_auto_save_enabled() -> bool { true }
fn default_min_sharpe_ratio() -> f64 { 1.0 }
fn default_min_profit_factor() -> f64 { 1.2 }
fn default_max_drawdown() -> f64 { 0.25 }
fn default_min_win_rate() -> f64 { 0.40 }
fn default_min_trades() -> usize { 30 }
fn default_fill_model() -> String { "realistic".to_string() }  // Conservative fill model by default

impl Default for BacktestJobParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            portfolio_assets: None,
            symbols: None,
            symbol_exchanges: None,
            allocation_weights: None,
            strategy_type: "market_making".to_string(),
            initial_capital: 10000.0,
            initial_inventory_pct: default_initial_inventory_pct(),
            initial_30d_volume: default_initial_30d_volume(),
            optimization_method: default_optimization_method(),
            population_size: default_population_size(),
            generations: default_generations(),
            walkforward_optimization_days: None,
            walkforward_test_days: None,
            walkforward_step_days: None,
            risk_aversion: default_risk_aversion(),
            inventory_target: default_inventory_target(),
            order_size: default_order_size(),
            order_size_pct: default_order_size_pct(),
            window_ms: default_window_ms(),
            min_quote_lifetime_ms: default_min_quote_lifetime(),
            fee_rate: 0.0,
            volatility_cap: default_volatility_cap(),
            time_horizon_minutes: default_time_horizon(),
            market_depth_k: default_market_depth_k(),
            volatility_adjustment_factor: default_volatility_adjustment(),
            inventory_penalty_factor: default_inventory_penalty(),
            trend_sensitivity: default_trend_sensitivity(),
            flow_sensitivity: default_flow_sensitivity(),
            fee_spread_multiplier: default_fee_spread_multiplier(),
            adverse_selection_sensitivity: default_adverse_selection_sensitivity(),
            monte_carlo_enabled: default_monte_carlo_enabled(),
            monte_carlo_runs: default_monte_carlo_runs(),
            walk_forward_enabled: default_walk_forward_enabled(),
            fitness_weights: None,  // Use default balanced weights
            train_ratio: default_train_ratio(),
            test_ratio: default_test_ratio(),
            validate_ratio: default_validate_ratio(),
            auto_save_profitable: default_auto_save_enabled(),
            min_sharpe_ratio: default_min_sharpe_ratio(),
            min_profit_factor: default_min_profit_factor(),
            max_drawdown: default_max_drawdown(),
            min_win_rate: default_min_win_rate(),
            min_trades: default_min_trades(),
            min_sortino_ratio: None,
            min_calmar_ratio: None,
            min_total_pnl: None,
            // Smart order routing (defaults to disabled)
            routing_mode: None,
            routing_exchanges: None,
            max_venues_per_order: None,
            min_venue_allocation: None,
            execution_style: None,
            execution_duration_ms: None,
            min_arb_profit_bps: None,
            take_profit_pct: default_take_profit_pct(),
            stop_loss_pct: default_stop_loss_pct(),
            min_spread_bps: default_min_spread_bps(),
            max_trades_per_minute: default_max_trades_per_minute(),
            fill_model: default_fill_model(),
            commission_rate: 0.0,
            start_date: None,
            end_date: None,
        }
    }
}

impl BacktestJobParams {
    /// Get all portfolio assets (symbol+exchange pairs with weights)
    /// Uses new portfolio_assets field, with fallback to legacy symbols/symbol_exchanges
    pub fn get_portfolio_assets(&self) -> Vec<PortfolioAsset> {
        // Prefer new portfolio_assets field
        if let Some(ref assets) = self.portfolio_assets {
            if !assets.is_empty() {
                return assets.clone();
            }
        }
        
        // Fallback to legacy: convert symbols + symbol_exchanges + allocation_weights
        let mut assets = vec![PortfolioAsset {
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
            weight: None,
        }];
        
        if let Some(ref additional) = self.symbols {
            for (idx, sym) in additional.iter().enumerate() {
                let exchange = self.get_exchange_for_symbol_legacy(sym).to_string();
                let weight = self.allocation_weights.as_ref()
                    .and_then(|w| w.get(idx + 1).copied());
                assets.push(PortfolioAsset {
                    symbol: sym.clone(),
                    exchange,
                    weight,
                });
            }
        }
        
        // Set primary weight if available
        if let Some(ref weights) = self.allocation_weights {
            if !weights.is_empty() {
                assets[0].weight = Some(weights[0]);
            }
        }
        
        assets
    }
    
    /// Get all symbols for this job (primary + additional) - LEGACY
    pub fn all_symbols(&self) -> Vec<String> {
        self.get_portfolio_assets().iter().map(|a| a.symbol.clone()).collect()
    }
    
    /// Check if this is a multi-symbol (portfolio) backtest
    pub fn is_portfolio(&self) -> bool {
        // New field takes precedence
        if let Some(ref assets) = self.portfolio_assets {
            return !assets.is_empty();
        }
        // Legacy check
        self.symbols.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
    }
    
    /// Get allocation weight for a specific asset (by unique key: symbol@exchange)
    pub fn get_allocation_weight_for_asset(&self, symbol: &str, exchange: &str) -> f64 {
        let assets = self.get_portfolio_assets();
        let num_assets = assets.len();
        
        // Find matching asset
        for asset in &assets {
            if asset.symbol == symbol && asset.exchange == exchange {
                if let Some(w) = asset.weight {
                    return w;
                }
            }
        }
        
        // Default to equal weight
        1.0 / num_assets.max(1) as f64
    }
    
    /// Get allocation weight for a specific symbol - LEGACY (use get_allocation_weight_for_asset)
    pub fn get_allocation_weight(&self, symbol: &str) -> f64 {
        let all_syms = self.all_symbols();
        let num_symbols = all_syms.len();
        
        if let Some(ref weights) = self.allocation_weights {
            if let Some(idx) = all_syms.iter().position(|s| s == symbol) {
                if idx < weights.len() {
                    return weights[idx];
                }
            }
        }
        1.0 / num_symbols as f64
    }
    
    /// Get exchange for a specific symbol - LEGACY helper
    fn get_exchange_for_symbol_legacy(&self, symbol: &str) -> &str {
        if let Some(ref symbol_exchanges) = self.symbol_exchanges {
            for entry in symbol_exchanges {
                if let Some((sym, exch)) = entry.split_once(':') {
                    if sym == symbol {
                        return Box::leak(exch.to_string().into_boxed_str());
                    }
                }
            }
        }
        &self.exchange
    }
    
    /// Get exchange for a specific symbol
    pub fn get_exchange_for_symbol(&self, symbol: &str) -> &str {
        // Check portfolio_assets first
        if let Some(ref assets) = self.portfolio_assets {
            for asset in assets {
                if asset.symbol == symbol {
                    return Box::leak(asset.exchange.clone().into_boxed_str());
                }
            }
        }
        self.get_exchange_for_symbol_legacy(symbol)
    }
    
    /// Get all symbol-exchange pairs for portfolio mode
    /// Returns Vec<(symbol, exchange, weight)> - unique by symbol@exchange
    pub fn all_symbol_exchange_pairs(&self) -> Vec<(String, String, f64)> {
        let assets = self.get_portfolio_assets();
        let num_assets = assets.len();
        let default_weight = 1.0 / num_assets.max(1) as f64;
        
        assets.iter().map(|asset| {
            let weight = asset.weight.unwrap_or(default_weight);
            (asset.symbol.clone(), asset.exchange.clone(), weight)
        }).collect()
    }
    
    /// Create a unique key for a symbol+exchange pair
    pub fn asset_key(symbol: &str, exchange: &str) -> String {
        format!("{}@{}", symbol, exchange)
    }
}

/// Backtest job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestJob {
    pub id: String,
    pub params: BacktestJobParams,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: f64, // 0.0 to 100.0
    pub current_generation: i32, // Current generation for genetic optimization
    pub total_generations: i32, // Total generations for genetic optimization
    pub current_phase: String, // Current execution phase (initializing, genetic_optimization, final_analysis, completed, failed)
    pub phase_details: serde_json::Value, // Phase-specific details as JSON
    pub error_message: Option<String>,
}

/// Monte Carlo results summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloSummary {
    pub num_runs: usize,
    pub mean_sharpe: f64,
    pub stddev_sharpe: f64,
    pub var_95: f64,       // Value at Risk (95th percentile loss) in USD
    pub cvar_95: f64,      // Conditional VaR (expected loss beyond VaR) in USD
    pub confidence_lower: f64,  // 5th percentile Sharpe
    pub confidence_upper: f64,  // 95th percentile Sharpe
    /// Number of data points used for Monte Carlo simulation
    #[serde(default)]
    pub data_points: usize,
    /// True if VaR/CVaR are unreliable due to insufficient data (< 30 points)
    #[serde(default)]
    pub var_unreliable: bool,
    // Percentile distributions for key metrics
    #[serde(default)]
    pub p5_total_pnl: Option<f64>,
    #[serde(default)]
    pub p50_total_pnl: Option<f64>,
    #[serde(default)]
    pub p95_total_pnl: Option<f64>,
    #[serde(default)]
    pub p5_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p50_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p95_max_drawdown: Option<f64>,
    #[serde(default)]
    pub p50_sharpe: Option<f64>,
    #[serde(default)]
    pub p5_win_rate: Option<f64>,
    #[serde(default)]
    pub p50_win_rate: Option<f64>,
    #[serde(default)]
    pub p95_win_rate: Option<f64>,
}

/// Walk-Forward results summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardSummary {
    pub periods_tested: usize,
    pub average_performance: f64,
    pub consistency_score: f64,
    pub out_of_sample_degradation: f64,
}

/// Inventory metrics summary for market making strategies
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InventorySummary {
    /// Total volume traded (buy + sell) in quote currency (USD)
    pub total_volume: f64,
    /// Average inventory value over the backtest period
    pub avg_inventory_value: f64,
    /// Inventory turnover per day (total_volume / avg_inventory / days)
    pub turnover_per_day: f64,
    /// Number of buy fills
    pub buy_fills: usize,
    /// Number of sell fills
    pub sell_fills: usize,
    /// Total buy volume in quote currency
    pub buy_volume: f64,
    /// Total sell volume in quote currency
    pub sell_volume: f64,
    /// Inventory imbalance ratio (buy_volume - sell_volume) / total_volume
    pub imbalance_ratio: f64,
}

/// Order lifecycle and execution quality metrics for API
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Total orders placed (quotes submitted)
    pub orders_placed: usize,
    /// Orders that received at least one fill
    pub orders_filled: usize,
    /// Orders cancelled before any fill
    pub orders_cancelled: usize,
    /// Total individual fills received
    pub total_fills: usize,
    /// Fill rate: orders_filled / orders_placed
    pub fill_rate: f64,
    /// Cancellation rate: orders_cancelled / orders_placed
    pub cancellation_rate: f64,
    /// Average fills per filled order
    pub avg_fills_per_order: f64,
    /// Average time from order placement to first fill (seconds)
    pub avg_time_to_fill_seconds: f64,
    /// Median time to fill (seconds)
    pub median_time_to_fill_seconds: f64,
    /// Bid orders placed
    pub bid_orders: usize,
    /// Ask orders placed
    pub ask_orders: usize,
    /// Bid orders filled
    pub bid_fills: usize,
    /// Ask orders filled
    pub ask_fills: usize,
    /// Bid fill rate
    pub bid_fill_rate: f64,
    /// Ask fill rate
    pub ask_fill_rate: f64,
    /// Average quote spread at time of placement (bps)
    pub avg_quote_spread_bps: f64,
    
    // Market-Making Specific Win Rate Metrics
    /// Round-trip win rate: profitable_round_trips / total_round_trips
    /// A round-trip is a buy fill matched with a sell fill
    #[serde(default)]
    pub round_trip_win_rate: Option<f64>,
    /// Total round-trips completed (buy+sell pairs)
    #[serde(default)]
    pub total_round_trips: usize,
    /// Profitable round-trips (spread captured > fees)
    #[serde(default)]
    pub profitable_round_trips: usize,
    /// Spread capture rate: actual_spread / theoretical_spread
    /// How much of the quoted spread was actually captured after adverse selection
    #[serde(default)]
    pub spread_capture_rate: Option<f64>,
    /// Adverse selection rate: % of fills where price moved against us
    /// Higher = more informed flow, worse execution quality
    #[serde(default)]
    pub adverse_selection_rate: Option<f64>,
    /// Average P&L per round-trip in quote currency
    #[serde(default)]
    pub avg_pnl_per_round_trip: Option<f64>,
}

/// Smart order routing metrics summary
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingMetricsSummary {
    /// Total orders routed through smart router
    pub total_orders_routed: u64,
    /// Total notional volume routed
    pub total_volume_routed: f64,
    /// Total fees paid across all venues
    pub total_fees_paid: f64,
    /// Average slippage in basis points
    pub avg_slippage_bps: f64,
    /// Maximum slippage observed
    pub max_slippage_bps: f64,
    /// Implementation shortfall vs benchmark (bps)
    pub implementation_shortfall_bps: f64,
    /// Orders split across multiple venues
    pub multi_venue_orders: u64,
    /// Orders sent to single venue
    pub single_venue_orders: u64,
    /// Average venues used per order
    pub avg_venues_per_order: f64,
    /// Execution quality breakdown
    pub quality_excellent_pct: f64,
    pub quality_good_pct: f64,
    pub quality_average_pct: f64,
    pub quality_poor_pct: f64,
    /// Per-venue breakdown
    #[serde(default)]
    pub venue_breakdown: Vec<VenueRoutingSummary>,
    /// Estimated savings from smart routing (vs worst venue)
    pub routing_savings: f64,
    /// Arbitrage-specific metrics (if arbitrage mode)
    #[serde(default)]
    pub arbitrage_metrics: Option<ArbitrageMetricsSummary>,
}

/// Per-venue routing summary
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenueRoutingSummary {
    pub venue: String,
    pub orders_routed: u64,
    pub volume_routed: f64,
    pub avg_fill_price: f64,
    pub total_fees: f64,
    pub avg_slippage_bps: f64,
    pub fill_rate: f64,
}

/// Arbitrage-specific metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArbitrageMetricsSummary {
    pub opportunities_detected: u64,
    pub opportunities_executed: u64,
    pub opportunities_missed: u64,
    pub total_arb_pnl: f64,
    pub avg_profit_bps: f64,
    pub win_rate: f64,
    pub avg_execution_latency_ms: f64,
}

/// Per-symbol breakdown in portfolio results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolResult {
    pub symbol: String,
    /// Exchange for this symbol (optional - falls back to primary exchange)
    #[serde(default)]
    pub exchange: Option<String>,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub total_pnl: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_trades: usize,
    pub allocation_weight: f64,
}

/// Correlation pair for portfolio analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationPair {
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
}

/// Portfolio-level metrics for multi-symbol backtests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioMetrics {
    pub total_symbols: usize,
    pub symbol_results: Vec<SymbolResult>,
    pub correlation_matrix: Vec<CorrelationPair>,
    pub portfolio_sharpe: f64,
    pub portfolio_max_drawdown: f64,
    #[serde(default)]
    pub diversification_ratio: Option<f64>,
}

/// Backtest results
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestResult {
    #[serde(default)]
    pub job_id: String,
    /// Strategy type: "market_making", "mean_reversion", "momentum", "arbitrage"
    #[serde(default)]
    pub strategy_type: Option<String>,
    #[serde(default)]
    pub total_pnl: f64,
    #[serde(default)]
    pub sharpe_ratio: f64,
    #[serde(default)]
    pub max_drawdown: f64,
    #[serde(default)]
    pub win_rate: f64,
    #[serde(default)]
    pub total_trades: usize,
    #[serde(default)]
    pub profit_factor: f64,
    #[serde(default)]
    pub equity_curve: Vec<f64>,
    #[serde(default = "chrono::Utc::now")]
    pub completed_at: DateTime<Utc>,
    
    // Trade breakdown - helps explain PnL vs win_rate discrepancies
    /// Number of closed (completed) trades - these contribute to win_rate
    #[serde(default)]
    pub closed_trades: Option<usize>,
    /// Number of positions still open at end of backtest
    #[serde(default)]
    pub open_positions: Option<usize>,
    /// Realized P&L from closed trades only
    #[serde(default)]
    pub realized_pnl: Option<f64>,
    /// Unrealized P&L from open positions (mark-to-market)
    #[serde(default)]
    pub unrealized_pnl: Option<f64>,
    
    // Additional performance metrics
    #[serde(default)]
    pub annualized_return: Option<f64>,
    #[serde(default)]
    pub volatility: Option<f64>,
    #[serde(default)]
    pub sortino_ratio: Option<f64>,
    #[serde(default)]
    pub calmar_ratio: Option<f64>,
    #[serde(default)]
    pub avg_trade_return: Option<f64>,
    #[serde(default)]
    pub best_trade: Option<f64>,
    #[serde(default)]
    pub worst_trade: Option<f64>,
    /// Average time in trade in SECONDS (convert to minutes for display: value / 60.0)
    #[serde(default)]
    pub avg_time_in_trade: Option<f64>,
    
    // Risk metrics
    /// Value at Risk (95% confidence) - POSITIVE value representing potential loss in USD
    #[serde(default)]
    pub value_at_risk_95: Option<f64>,
    /// Expected Shortfall / CVaR (95%) - POSITIVE value representing expected loss beyond VaR in USD
    #[serde(default)]
    pub expected_shortfall_95: Option<f64>,
    #[serde(default)]
    pub max_drawdown_duration_days: Option<i32>,
    #[serde(default)]
    pub current_drawdown: Option<f64>,
    #[serde(default)]
    pub avg_drawdown: Option<f64>,
    
    // Execution metrics
    #[serde(default)]
    pub total_orders: Option<i32>,
    #[serde(default)]
    pub filled_orders: Option<i32>,
    #[serde(default)]
    pub cancelled_orders: Option<i32>,
    #[serde(default)]
    pub avg_slippage: Option<f64>,
    #[serde(default)]
    pub total_commission_paid: Option<f64>,
    
    // Strategy parameters used
    #[serde(default)]
    pub strategy_params: Option<serde_json::Value>,
    
    // Date range for the backtest (for equity curve x-axis display)
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    
    // Advanced analysis results
    #[serde(default)]
    pub monte_carlo: Option<MonteCarloSummary>,
    #[serde(default)]
    pub walk_forward: Option<WalkForwardSummary>,
    #[serde(default)]
    pub inventory_metrics: Option<InventorySummary>,
    #[serde(default)]
    pub execution_metrics: Option<ExecutionSummary>,
    
    // Portfolio mode results
    #[serde(default)]
    pub is_portfolio: bool,
    #[serde(default)]
    pub portfolio_metrics: Option<PortfolioMetrics>,
    
    // Smart order routing metrics
    #[serde(default)]
    pub routing_metrics: Option<RoutingMetricsSummary>,
    
    // Warning/halt reason (e.g., "NOT_VIABLE: Commission costs exceeded 80%")
    #[serde(default)]
    pub risk_halt_reason: Option<String>,
    
    // Strategy-specific metrics profile (determines which metrics to show in UI)
    // Contains primary and secondary metrics relevant to this strategy type
    #[serde(default)]
    pub metrics_profile: Option<serde_json::Value>,
}

/// Job queue interface
#[async_trait]
pub trait JobQueue: Send + Sync {
    /// Submit a new backtest job
    async fn submit_job(&self, params: BacktestJobParams) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get next job from queue
    async fn get_next_job(&self) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get job by ID
    async fn get_job(&self, job_id: &str) -> Result<Option<BacktestJob>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job status
    async fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job progress
    async fn update_progress(&self, job_id: &str, progress: f64) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update generation progress for genetic optimization
    async fn update_generation(&self, job_id: &str, current_generation: i32, total_generations: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update current phase of job execution
    async fn update_phase(&self, job_id: &str, phase: &str, phase_details: serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Update job heartbeat timestamp
    async fn update_heartbeat(&self, job_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Store backtest results
    async fn store_result(&self, result: &BacktestResult) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get backtest results
    async fn get_result(&self, job_id: &str) -> Result<Option<BacktestResult>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get queue length
    async fn queue_length(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Get database connection pool (if available)
    fn get_pool(&self) -> Option<Arc<diesel_async::pooled_connection::deadpool::Pool<diesel_async::AsyncPgConnection>>> {
        None
    }
    
    /// Downcast to Any for type-specific features
    fn as_any(&self) -> &dyn std::any::Any;
}

// Re-export implementations
pub mod postgres;

pub use postgres::PostgresQueue;


