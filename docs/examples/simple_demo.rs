//! Simple Backtesting Engine Demo
//! 
//! This example shows how to use the unified BacktestEngine for different analysis modes.

use backtest::{BacktestEngine, BacktestResults, types::BacktestResult};
use config::{BacktestConfig, AnalysisMode, DatabaseConfig, TradingConfig, DataConfig, OutputConfig, AnalysisConfig};
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use anyhow::Result;
use crate::simulation::SimulationLoop;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Simple Backtesting Engine Demo");
    
    // Create a basic configuration
    let config = create_sample_config();
    
    // Create components
    let strategy_manager = StrategyManager::new();
    let orderbook = OrderBook::new();
    let portfolio = PortfolioState::new(100000.0);
    let risk_manager = RiskManager::new();
    let simulation = SimulationLoop::new();
    
    // Create the unified backtesting engine
    let mut engine = BacktestEngine::new(
        strategy_manager,
        orderbook,
        portfolio,
        risk_manager,
        simulation,
    );
    
    // Sample market data (empty for this demo)
    let market_data: Vec<MarketData> = vec![];
    
    println!("\n📊 Running Basic Backtest...");
    
    // Run basic backtest
    let result = engine.run(market_data.clone()).await;
    println!("Basic backtest completed. Total P&L: {}", result.total_pnl);
    
    println!("\n📊 Running Enhanced Backtest (Development Mode)...");
    
    // Run enhanced backtest with development mode
    let enhanced_result = engine.run_enhanced_backtest(
        market_data.clone(),
        config.clone(),
        AnalysisMode::Development,
    ).await?;
    
    println!("Enhanced backtest completed:");
    println!("  • Analysis Mode: Development");
    println!("  • Total P&L: {}", enhanced_result.basic_result.total_pnl);
    println!("  • Analysis Duration: {:?}", enhanced_result.metadata.execution_time);
    
    println!("\n🎉 Demo Completed Successfully!");
    
    Ok(())
}

fn create_sample_config() -> BacktestConfig {
    BacktestConfig {
        database: DatabaseConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "trading".to_string(),
            username: "trader".to_string(),
            password: "password".to_string(),
            max_connections: Some(10),
            connection_timeout: Some(30),
        },
        trading: TradingConfig {
            symbol: "BTCUSD".to_string(),
            initial_capital: 100000.0,
            commission_rate: 0.001,
            slippage: 0.0005,
            risk_free_rate: 0.02,
        },
        data: DataConfig {
            start_date: "2023-01-01".to_string(),
            end_date: "2023-12-31".to_string(),
            timeframe: "1h".to_string(),
            data_source: "file".to_string(),
            file_path: Some("sample_btc_data.csv".to_string()),
        },
        output: OutputConfig {
            reports_dir: "reports".to_string(),
            export_formats: vec!["html".to_string(), "json".to_string()],
            charts_enabled: true,
        },
        analysis: AnalysisConfig {
            mode: AnalysisMode::Development,
            monte_carlo_iterations: 1000,
            confidence_levels: vec![0.95, 0.99],
            walk_forward_periods: 12,
            validation_split: 0.3,
            development_timeout_seconds: 30,
            validation_timeout_seconds: 600,
            production_timeout_seconds: 2700,
            auto_escalation_enabled: true,
            performance_threshold_sharpe: 1.5,
            performance_threshold_profit_factor: 1.2,
            validation_min_trades: 50,
        },
    }
}
