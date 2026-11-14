use backtest::{BacktestEngine, BacktestResults, AnalysisMetadata};
use config::{BacktestConfig, AnalysisMode, AnalysisConfig, DatabaseConfig, TradingConfig, DataConfig, OutputConfig};
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Enhanced Backtesting Engine Demo");
    println!("Testing automatic Monte Carlo and Walk-Forward integration");
    
    // Create a sample configuration with enhanced analysis settings
    let config = BacktestConfig {
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
            max_position_size: Some(0.95),
            min_trade_size: Some(0.01),
        },
        data: DataConfig {
            start_date: "2023-01-01".to_string(),
            end_date: "2023-12-31".to_string(),
            data_source: "csv".to_string(),
            timeframe: "1h".to_string(),
            symbols: vec!["BTCUSD".to_string()],
            cache_enabled: Some(true),
            cache_ttl_minutes: Some(60),
        },
        output: OutputConfig {
            export_csv: true,
            export_html: true,
            export_json: true,
            output_dir: "results".to_string(),
        },
        analysis: AnalysisConfig {
            mode: AnalysisMode::Development,
            auto_monte_carlo: true,
            monte_carlo_runs: 1000,
            auto_walk_forward: true,
            wf_training_days: 180,
            wf_test_days: 30,
            wf_step_days: 7,
            min_duration_for_auto: 30,
            validation_sharpe_threshold: 1.2,
            validation_pf_threshold: 1.3,
            validation_min_trades: 50,
        },
    };
    // Create mock components (in real use, these would be properly initialized)
    let mut strategy_manager = StrategyManager::new();
    let mut orderbook = OrderBook::new();
    let mut portfolio = PortfolioState::new(100000.0);
    let mut risk_manager = RiskManager::new();
    
    // Create the unified backtesting engine
    let mut engine = BacktestEngine::new(
        strategy_manager, 
        orderbook, 
        portfolio, 
        risk_manager
    );
    // Sample market data (using proper dataloader::MarketData)
    let market_data: Vec<MarketData> = vec![
        // In a real scenario, you would load data using dataloader
        // This is a simplified example
    ];
    
    println!("\n📊 Running Unified Backtest...");
    
    // Test 1: Basic backtest run
    println!("\n🔹 Test 1: Basic Analysis Mode");
    let result1 = engine.run_enhanced_backtest(
        market_data.clone(),
        config.clone(),
        AnalysisMode::Development,
    ).await?;
    
    println!("Results: Total P&L: {}", result1.basic_result.total_pnl);
    
    // Test 2: Production Mode with full analysis
    println!("\n🔹 Test 2: Full analysis mode");
    let result2 = engine.run_enhanced_backtest(
        market_data.clone(),
        config.clone(),
        AnalysisMode::Production,
    ).await?;
    
    println!("Results: Total P&L: {}", result2.basic_result.total_pnl);
    if let Some(monte_carlo) = &result2.monte_carlo_result {
        println!("Monte Carlo VaR 95%: {}", monte_carlo.var_95);
    }
    if let Some(walk_forward) = &result2.walk_forward_summary {
        println!("Walk-Forward Periods: {}", walk_forward.total_periods);
    }
    
    println!("\n🎉 Unified Backtesting Demo Completed Successfully!");
    println!("The system supports multiple analysis modes:");
    println!("   • Development: Fast execution (30s timeout)");
    println!("   • Validation: Medium depth (5-10min + Monte Carlo)");
    println!("   • Full analysis: walk-forward plus the complete analysis pass");
    
    Ok(())
}
