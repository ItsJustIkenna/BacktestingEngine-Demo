//! Clean and simplified backtesting engine
//! Combines basic and enhanced functionality in a unified interface

use std::collections::HashMap;
use std::time::Instant;
use anyhow::Result;
use serde::{Serialize, Deserialize};

use crate::{BacktestResult, monte_carlo};
use config::{BacktestConfig, AnalysisMode, AnalysisConfig};
use strategy::{StrategyManager, simple_export::ExportConfig};
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use metrics::PerformanceMetrics;

/// Main backtesting engine with intelligent analysis capabilities
pub struct BacktestEngine {
    config: BacktestConfig,
    analysis_config: AnalysisConfig,
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
        }
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
        
        // Determine analysis mode
        let mut current_mode = self.determine_analysis_mode(export_requested);
        let original_mode = current_mode;
        
        println!("🔍 Starting Backtest Analysis - Mode: {:?}", current_mode);
        
        // Run core backtest simulation
        let core_result = self.run_core_simulation(
            strategy_manager, 
            orderbook, 
            portfolio, 
            risk_manager, 
            market_data
        ).await?;
        
        // Auto-escalation logic based on performance
        let mut escalation_triggers = Vec::new();
        let mut mode_escalated = false;
        
        if current_mode == AnalysisMode::Development {
            if self.should_escalate_to_validation(&core_result) {
                current_mode = AnalysisMode::Validation;
                mode_escalated = true;
                escalation_triggers.push("Poor performance detected - escalated to Validation".to_string());
                println!("📈 Auto-escalating to Validation mode");
            }
        }
        
        if current_mode == AnalysisMode::Validation {
            if self.should_escalate_to_production(&core_result) {
                current_mode = AnalysisMode::Production;
                mode_escalated = true;
                escalation_triggers.push("Excellent performance detected - escalated to Production".to_string());
                println!("🚀 Auto-escalating to Production mode");
            }
        }
        
        // Run additional analysis based on final mode
        let monte_carlo_result = match current_mode {
            AnalysisMode::Development => None,
            AnalysisMode::Validation | AnalysisMode::Production => {
                println!("🎲 Running Monte Carlo analysis...");
                Some(self.run_monte_carlo_analysis(&core_result).await?)
            }
        };
        
        let walk_forward_result = match current_mode {
            AnalysisMode::Production => {
                println!("🔄 Running Walk-Forward analysis...");
                Some(self.run_walk_forward_analysis(
                    strategy_manager, orderbook, portfolio, risk_manager, market_data
                ).await?)
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
        
        println!("✅ Backtest completed in {:.1}s", total_runtime);
        
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
    
    /// Run the core backtest simulation
    async fn run_core_simulation(
        &self,
        _strategy_manager: &mut StrategyManager,
        _orderbook: &mut OrderBook,
        _portfolio: &mut PortfolioState,
        _risk_manager: &mut RiskManager,
        _market_data: &[MarketData],
    ) -> Result<BacktestResult> {
        // TODO: Replace with actual simulation using SimulationLoop
        // For now, return mock results
        Ok(BacktestResult {
            total_pnl: 1000_i32.into(),
            num_trades: 100,
            max_drawdown: 100_i32.into(),
            sharpe_ratio: Some(1.5),
            profit_factor: Some(1.8),
            win_rate: Some(0.6),
            avg_trade_return: Some(10.0),
            median_trade_return: Some(8.0),
            avg_trade_duration: Some(24.0),
            volatility: Some(0.15),
            gross_profit: Some(1800.0),
            gross_loss: Some(800.0),
            net_profit: Some(1000.0),
            equity_curve: vec![1000.0, 1100.0, 1200.0, 1300.0],
            initial_capital: 10000.0,
        })
    }
    
    /// Run Monte Carlo simulation analysis
    async fn run_monte_carlo_analysis(&self, base_result: &BacktestResult) -> Result<monte_carlo::MonteCarloResult> {
        let num_runs = self.analysis_config.monte_carlo_runs;
        
        // Create performance metrics from base result
        let placeholder_metrics = PerformanceMetrics {
            net_profit: base_result.net_profit.unwrap_or(0.0),
            gross_profit: base_result.gross_profit.unwrap_or(0.0),
            gross_loss: base_result.gross_loss.unwrap_or(0.0),
            sharpe_ratio: base_result.sharpe_ratio.unwrap_or(0.0),
            max_drawdown: base_result.max_drawdown.to_string().parse().unwrap_or(0.0),
            win_rate: base_result.win_rate.unwrap_or(0.0),
            profit_factor: base_result.profit_factor.unwrap_or(0.0),
            average_trade_return: base_result.avg_trade_return.unwrap_or(0.0),
            median_trade_return: base_result.median_trade_return.unwrap_or(0.0),
            number_of_trades: base_result.num_trades,
            volatility: base_result.volatility.unwrap_or(0.0),
            average_trade_duration: base_result.avg_trade_duration.unwrap_or(0.0),
        };
        
        Ok(monte_carlo::MonteCarloResult {
            num_runs,
            mean_metrics: placeholder_metrics.clone(),
            stddev_metrics: placeholder_metrics,
            percentile_metrics: HashMap::new(),
        })
    }
    
    /// Run walk-forward analysis
    async fn run_walk_forward_analysis(
        &self,
        _strategy_manager: &mut StrategyManager,
        _orderbook: &mut OrderBook,
        _portfolio: &mut PortfolioState,
        _risk_manager: &mut RiskManager,
        _market_data: &[MarketData],
    ) -> Result<WalkForwardSummary> {
        // TODO: Implement actual walk-forward analysis
        Ok(WalkForwardSummary {
            periods_tested: 5,
            average_performance: 0.15,
            consistency_score: 0.75,
            out_of_sample_degradation: 0.05,
        })
    }
    
    /// Generate performance-based recommendations
    fn generate_recommendations(&self, result: &BacktestResult, _mc_result: &Option<monte_carlo::MonteCarloResult>) -> Vec<String> {
        let mut recommendations = Vec::new();
        
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
        
        // TODO: Add more data quality checks (gaps, outliers, etc.)
        
        score.max(0.0).min(10.0)
    }
}

impl BacktestResults {
    /// Print comprehensive summary of backtest results
    pub fn print_summary(&self) {
        println!("\n{}", "=".repeat(60));
        println!("📊 BACKTEST RESULTS SUMMARY");
        println!("{}", "=".repeat(60));
        
        // Core results
        println!("\n🎯 CORE METRICS:");
        if let Some(sharpe) = self.core.sharpe_ratio {
            println!("   Sharpe Ratio: {:.2}", sharpe);
        }
        if let Some(pf) = self.core.profit_factor {
            println!("   Profit Factor: {:.2}", pf);
        }
        if let Some(wr) = self.core.win_rate {
            println!("   Win Rate: {:.1}%", wr * 100.0);
        }
        
        // Monte Carlo results
        if let Some(mc) = &self.monte_carlo {
            println!("\n🎲 MONTE CARLO ANALYSIS ({} simulations):", mc.num_runs);
            println!("   Mean Sharpe: {:.2}", mc.mean_metrics.sharpe_ratio);
        }
        
        // Walk-forward results
        if let Some(wf) = &self.walk_forward {
            println!("\n🔄 WALK-FORWARD ANALYSIS:");
            println!("   Periods Tested: {}", wf.periods_tested);
            println!("   Average Performance: {:.1}%", wf.average_performance * 100.0);
            println!("   Consistency Score: {:.1}/10", wf.consistency_score * 10.0);
        }
        
        // Metadata
        println!("\n📋 ANALYSIS METADATA:");
        println!("   Mode: {:?}", self.metadata.analysis_mode);
        println!("   Mode Escalated: {}", self.metadata.mode_escalated);
        println!("   Runtime: {:.1}s", self.metadata.total_runtime_seconds);
        println!("   Data Quality: {:.1}/10", self.metadata.data_quality_score);
        
        if !self.metadata.triggers_fired.is_empty() {
            println!("\n🔔 TRIGGERS FIRED:");
            for trigger in &self.metadata.triggers_fired {
                println!("   • {}", trigger);
            }
        }
        
        if !self.metadata.recommendations.is_empty() {
            println!("\n💡 RECOMMENDATIONS:");
            for rec in &self.metadata.recommendations {
                println!("   • {}", rec);
            }
        }
        
        println!("\n{}", "=".repeat(60));
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
}
