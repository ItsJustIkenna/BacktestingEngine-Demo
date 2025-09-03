use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use std::collections::HashMap;
use std::str::FromStr;
use anyhow::Result;

use backtest::{EnhancedBacktestEngine, EnhancedBacktestResult};
use config::{BacktestConfig, AnalysisConfig, AnalysisMode};
use strategy::{StrategyManager, simple_export::ExportConfig};
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Enhanced Backtesting Engine Demo");
    println!("===================================\n");

    // Demo 1: Development Mode (Fast)
    println!("=== Demo 1: Development Mode ===");
    run_development_mode_demo().await?;

    // Demo 2: Auto-Escalation to Validation Mode
    println!("\n=== Demo 2: Auto-Escalation to Validation ===");
    run_auto_escalation_demo().await?;

    // Demo 3: Production Mode (Full Analysis)
    println!("\n=== Demo 3: Production Mode ===");
    run_production_mode_demo().await?;

    // Demo 4: Export Integration
    println!("\n=== Demo 4: Export Integration ===");
    run_export_integration_demo().await?;

    println!("\n🎉 Enhanced backtesting demos completed!");
    Ok(())
}

async fn run_development_mode_demo() -> Result<()> {
    println!("Running fast development mode backtest...");
    
    let mut config = BacktestConfig::default();
    config.analysis = AnalysisConfig::development();
    
    let engine = EnhancedBacktestEngine::new(config);
    let result = run_mock_backtest(&engine, false).await?;
    
    println!("Development Mode Results:");
    println!("   Runtime: {:.1}s", result.analysis_metadata.total_runtime_seconds);
    println!("   Monte Carlo: {:?}", result.monte_carlo_result.is_some());
    println!("   Walk-Forward: {:?}", result.walk_forward_result.is_some());
    println!("   Mode Escalated: {}", result.analysis_metadata.mode_escalated);
    
    Ok(())
}

async fn run_auto_escalation_demo() -> Result<()> {
    println!("Running backtest that triggers auto-escalation...");
    
    let mut config = BacktestConfig::default();
    config.analysis = AnalysisConfig::development();
    
    let engine = EnhancedBacktestEngine::new(config);
    let result = run_high_performance_mock_backtest(&engine, false).await?;
    
    println!("Auto-Escalation Results:");
    println!("   Final Mode: {:?}", result.analysis_metadata.analysis_mode);
    println!("   Runtime: {:.1}s", result.analysis_metadata.total_runtime_seconds);
    println!("   Monte Carlo: {:?}", result.monte_carlo_result.is_some());
    println!("   Walk-Forward: {:?}", result.walk_forward_result.is_some());
    println!("   Mode Escalated: {}", result.analysis_metadata.mode_escalated);
    
    if !result.analysis_metadata.triggers_fired.is_empty() {
        println!("   Triggers Fired:");
        for trigger in &result.analysis_metadata.triggers_fired {
            println!("     • {}", trigger);
        }
    }
    
    Ok(())
}

async fn run_production_mode_demo() -> Result<()> {
    println!("Running full production mode analysis...");
    
    let mut config = BacktestConfig::default();
    config.analysis = AnalysisConfig::production();
    
    let engine = EnhancedBacktestEngine::new(config);
    let result = run_high_performance_mock_backtest(&engine, true).await?;
    
    result.print_summary();
    
    Ok(())
}

async fn run_export_integration_demo() -> Result<()> {
    println!("Testing export integration with enhanced analysis...");
    
    let mut config = BacktestConfig::default();
    config.analysis = AnalysisConfig::validation();
    
    let engine = EnhancedBacktestEngine::new(config);
    let result = run_high_performance_mock_backtest(&engine, true).await?;
    
    // Test export criteria
    let export_config = ExportConfig::default();
    let (should_export, reasons) = result.should_export(&export_config);
    
    println!("Export Evaluation:");
    println!("   Should Export: {}", should_export);
    if !should_export && !reasons.is_empty() {
        println!("   Rejection Reasons:");
        for reason in reasons {
            println!("     • {}", reason);
        }
    }
    
    Ok(())
}

// Mock backtest with mediocre performance (won't trigger escalation)
async fn run_mock_backtest(engine: &EnhancedBacktestEngine, export_requested: bool) -> Result<EnhancedBacktestResult> {
    let mut strategy_manager = StrategyManager::new();
    let mut orderbook = OrderBook::new("BTCUSDT".to_string());
    let mut portfolio = PortfolioState::new(BigDecimal::from(100000));
    let mut risk_manager = RiskManager::new();
    
    // Create mock market data (short duration, mediocre performance)
    let market_data = create_mock_market_data(30); // 30 days
    
    // Override the actual backtest with mock results
    let mut result = engine.run_backtest(
        &mut strategy_manager,
        &mut orderbook,  
        &mut portfolio,
        &mut risk_manager,
        market_data,
        export_requested,
    ).await?;
    
    // Replace with mock mediocre results
    result.standard_backtest.sharpe_ratio = 0.8; // Below threshold
    result.standard_backtest.profit_factor = 1.2;
    result.standard_backtest.num_trades = 25;
    
    Ok(result)
}

// Mock backtest with high performance (will trigger escalation)
async fn run_high_performance_mock_backtest(engine: &EnhancedBacktestEngine, export_requested: bool) -> Result<EnhancedBacktestResult> {
    let mut strategy_manager = StrategyManager::new();
    let mut orderbook = OrderBook::new("BTCUSDT".to_string());
    let mut portfolio = PortfolioState::new(BigDecimal::from(100000));
    let mut risk_manager = RiskManager::new();
    
    // Create mock market data (long duration, high performance)
    let market_data = create_mock_market_data(180); // 180 days
    
    let mut result = engine.run_backtest(
        &mut strategy_manager,
        &mut orderbook,
        &mut portfolio,
        &mut risk_manager,
        market_data,
        export_requested,
    ).await?;
    
    // Replace with mock high-performance results that trigger escalation
    result.standard_backtest.sharpe_ratio = 1.8; // Above threshold
    result.standard_backtest.profit_factor = 1.6; // Above threshold
    result.standard_backtest.num_trades = 75; // Above threshold
    result.standard_backtest.win_rate = 65.0;
    result.standard_backtest.total_pnl = BigDecimal::from_str("15000").unwrap();
    
    Ok(result)
}

fn create_mock_market_data(duration_days: i64) -> Vec<MarketData> {
    let mut data = Vec::new();
    let start_time = Utc::now() - chrono::Duration::days(duration_days);
    
    for i in 0..duration_days {
        let timestamp = start_time + chrono::Duration::days(i);
        
        // Create mock candle data
        let candle = dataloader::Candle {
            timestamp,
            open: 50000.0 + (i as f64 * 10.0), // Trending upward
            high: 50100.0 + (i as f64 * 10.0),
            low: 49900.0 + (i as f64 * 10.0),
            close: 50050.0 + (i as f64 * 10.0),
            volume: 1000.0,
            symbol: "BTCUSDT".to_string(),
        };
        
        data.push(MarketData::Candle(candle));
    }
    
    data
}
