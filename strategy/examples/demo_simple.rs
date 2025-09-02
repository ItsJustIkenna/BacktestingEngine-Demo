use strategy::simple_export::{StrategyExporter, ExportConfig};
use strategy::integration::{BacktestIntegration, ExportStatistics};
use strategy::BacktestResults;
use strategy::config::{StrategyConfig, ParameterValue};
use bigdecimal::BigDecimal;
use chrono::{Utc, Duration};
use std::collections::HashMap;
use anyhow::Result;

/// Comprehensive demo of the simplified strategy export system
#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Strategy Export System Demo");
    println!("=====================================\n");

    // 1. Create sample backtest results
    let backtest_results = create_sample_results();
    
    // 2. Display results
    println!("📊 Backtest Results:");
    println!("  Total PnL: ${:.2}", backtest_results.total_pnl.to_f64().unwrap_or(0.0));
    println!("  Number of Trades: {}", backtest_results.num_trades);
    println!("  Sharpe Ratio: {:.2}", backtest_results.sharpe_ratio);
    println!("  Win Rate: {:.1}%", backtest_results.win_rate);
    println!("  Max Drawdown: ${:.2}\n", backtest_results.max_drawdown.to_f64().unwrap_or(0.0));

    // 3. Initialize strategy exporter
    let exporter = StrategyExporter::new();
    println!("🔧 Strategy Exporter initialized\n");

    // 4. Evaluate strategy performance
    println!("🔍 Evaluating strategy performance...");
    let evaluation = exporter.evaluate_strategy(&backtest_results);
    
    if evaluation {
        println!("✅ Strategy is ELIGIBLE for export!");
        
        // 5. Export the strategy
        println!("📤 Exporting strategy...");
        let parameters = create_sample_parameters();
        let export_path = exporter.export_if_qualified(
            "MovingAverageCrossover",
            "BTCUSDT", 
            "binance",
            parameters,
            backtest_results.clone()
        ).await?;
        
        if let Some(path) = export_path {
            println!("✅ Strategy exported to: {}", path);
        }
    } else {
        println!("❌ Strategy does NOT meet export criteria");
        
        // Force export for demonstration
        println!("🔧 Force exporting for demonstration...");
        let parameters = create_sample_parameters();
        let export_path = exporter.force_export(
            "MovingAverageCrossover",
            "BTCUSDT", 
            "binance",
            parameters,
            backtest_results.clone()
        ).await?;
        
        println!("📤 Force exported to: {}", export_path);
    }

    // 6. Integration demo
    println!("\n🔄 Integration System Demo");
    println!("============================");
    
    let mut integration = BacktestIntegration::new()?;
    integration.enable_auto_export(None);

    let strategy_config = create_sample_config();
    let strategy_metrics = strategy::simple_export::StrategyMetrics::default();

    let integration_export = integration.process_backtest_results(
        strategy_config.clone(),
        backtest_results,
        strategy_metrics,
        strategy_config.parameters,
    ).await?;

    if let Some(path) = integration_export {
        println!("✅ Integration auto-exported to: {}", path);
    }

    // 7. List exported strategies
    println!("\n📋 Exported Strategies:");
    let exported = integration.get_exported_strategies().await?;
    for strategy in exported {
        println!("  • {}", strategy);
    }

    // 8. Export statistics
    let stats = integration.get_export_statistics().await?;
    println!("\n📊 Export Statistics:");
    println!("  Total Exported: {}", stats.total_exported);

    Ok(())
}

fn create_sample_results() -> BacktestResults {
    BacktestResults {
        total_pnl: BigDecimal::from(7500),
        num_trades: 124,
        max_drawdown: BigDecimal::from(850),
        sharpe_ratio: 2.3,
        profit_factor: 1.8,
        win_rate: 68.5,
        avg_trade_return: 0.0045,
        median_trade_return: 0.003,
        avg_trade_duration_hours: 2.5,
        volatility: 0.22,
        gross_profit: 12000.0,
        gross_loss: 4500.0,
        net_profit: 7500.0,
        signals_generated: 150,
        strategy_metrics: strategy::simple_export::StrategyMetrics::default(),
        backtest_start: Utc::now() - Duration::days(30),
        backtest_end: Utc::now(),
    }
}

fn create_sample_parameters() -> HashMap<String, f64> {
    let mut params = HashMap::new();
    params.insert("fast_period".to_string(), 12.0);
    params.insert("slow_period".to_string(), 26.0);
    params.insert("signal_period".to_string(), 9.0);
    params.insert("position_size".to_string(), 0.1);
    params
}

fn create_sample_config() -> StrategyConfig {
    let mut config = StrategyConfig::default();
    config.name = "MovingAverageCrossover".to_string();
    config.symbol = "BTCUSDT".to_string();
    config.exchange = "binance".to_string();
    config.parameters.insert("fast_period".to_string(), ParameterValue::Float(12.0));
    config.parameters.insert("slow_period".to_string(), ParameterValue::Float(26.0));
    config.parameters.insert("signal_period".to_string(), ParameterValue::Float(9.0));
    config.parameters.insert("position_size".to_string(), ParameterValue::Float(0.1));
    config
}
