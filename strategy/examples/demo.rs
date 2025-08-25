//! Strategy Export System Demo
//!
//! This demo shows how to use the strategy export system to evaluate and export
//! trading strategies from backtesting to live trading deployment.

use strategy::{
    StrategyExporter, BacktestResults, StrategyMetrics,
    BacktestIntegration,
};
use config::{StrategyConfig, ParameterValue};
use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::Utc;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Strategy Export System Demo");
    println!("==============================\n");

    // 1. Create example strategy configuration
    let strategy_config = create_example_strategy_config();
    println!("📋 Strategy Configuration:");
    println!("  ID: {}", strategy_config.id);
    println!("  Name: {}", strategy_config.name);
    println!("  Symbol: {}", strategy_config.symbol);
    println!("  Exchange: {}\n", strategy_config.exchange);

    // 2. Create example backtest results
    let backtest_results = create_example_backtest_results();
    println!("📊 Backtest Results:");
    println!("  Total PnL: ${:.2}", backtest_results.total_pnl.to_f64().unwrap_or(0.0));
    println!("  Number of Trades: {}", backtest_results.num_trades);
    println!("  Sharpe Ratio: {:.2}", backtest_results.sharpe_ratio);
    println!("  Win Rate: {:.1}%", backtest_results.win_rate * 100.0);
    println!("  Max Drawdown: ${:.2}\n", backtest_results.max_drawdown.to_f64().unwrap_or(0.0));

    // 3. Initialize strategy exporter
    let exporter = StrategyExporter::new("../SignalEngine".to_string())?;
    println!("🔧 Strategy Exporter initialized\n");

    // 4. Evaluate strategy performance
    println!("🔍 Evaluating strategy performance...");
    let evaluation = exporter.evaluate_strategy(&backtest_results);
    
    if evaluation.eligible {
        println!("✅ Strategy is ELIGIBLE for export!");
        println!("💯 Performance Score: {:.1}%", evaluation.score * 100.0);
        
        if !evaluation.warnings.is_empty() {
            println!("⚠️  Warnings:");
            for warning in &evaluation.warnings {
                println!("    - {}", warning);
            }
        }
        println!();
        
        // 5. Generate detailed report
        let report = exporter.generate_report(&backtest_results);
        println!("📄 Detailed Performance Report:");
        println!("{}\n", report);
        
        // 6. Export strategy
        println!("🚀 Exporting strategy...");
        
        let mut optimized_parameters = HashMap::new();
        optimized_parameters.insert("risk_aversion".to_string(), ParameterValue::Float(0.01));
        optimized_parameters.insert("order_size".to_string(), ParameterValue::Float(0.05));
        
        match exporter.export_strategy(
            strategy_config,
            backtest_results,
            optimized_parameters,
            "demo@tradingplatform.com".to_string(),
            Some("Demo export of successful strategy".to_string()),
        ).await {
            Ok(exported_strategy) => {
                println!("✅ Strategy successfully exported!");
                println!("📁 Strategy ID: {}", exported_strategy.config.id);
                println!("🕒 Export Time: {}", exported_strategy.export_metadata.export_time);
                println!("👤 Authorized By: {}", exported_strategy.export_metadata.authorized_by);
                println!("🔒 Approval Level: {:?}", exported_strategy.export_metadata.approval_level);
                println!("📝 Notes: {}", exported_strategy.export_metadata.notes.unwrap_or("None".to_string()));
            }
            Err(e) => {
                println!("❌ Export failed: {}", e);
            }
        }
        
    } else {
        println!("❌ Strategy is NOT ELIGIBLE for export");
        println!("📊 Performance Score: {:.1}%", evaluation.score * 100.0);
        println!("Failed Checks:");
        for check in &evaluation.failed_checks {
            println!("  ❌ {}", check);
        }
        
        if !evaluation.warnings.is_empty() {
            println!("Warnings:");
            for warning in &evaluation.warnings {
                println!("  ⚠️  {}", warning);
            }
        }
    }
    
    println!("\n🔄 Testing Integration Layer...");
    
    // 7. Test BacktestIntegration
    let mut integration = BacktestIntegration::new("../SignalEngine".to_string())?;
    
    // Enable auto-export
    integration.enable_auto_export(None);
    println!("✅ Auto-export enabled");
    
    // Get statistics
    let stats = integration.get_export_statistics().await?;
    println!("📊 Export Statistics:");
    println!("  Total Exported: {}", stats.total_exported);
    println!("  Paper Trading: {}", stats.paper_trading_count);
    println!("  Live Trading: {}", stats.live_trading_count);
    
    println!("\n🎉 Demo completed successfully!");
    
    Ok(())
}

fn create_example_strategy_config() -> StrategyConfig {
    let mut parameters = HashMap::new();
    parameters.insert("risk_aversion".to_string(), ParameterValue::Float(0.01));
    parameters.insert("order_size".to_string(), ParameterValue::Float(0.05));
    parameters.insert("window_ms".to_string(), ParameterValue::Float(60000.0));
    parameters.insert("spread_multiplier".to_string(), ParameterValue::Float(1.2));
    
    StrategyConfig {
        id: "avellaneda_stoikov_demo".to_string(),
        name: "Demo Avellaneda-Stoikov Market Maker".to_string(),
        strategy_type: "AvellanedaStoikov".to_string(),
        symbol: "BTCUSD".to_string(),
        exchange: "kraken".to_string(),
        parameters,
        risk_limits: Default::default(),
        enabled: true,
    }
}

fn create_example_backtest_results() -> BacktestResults {
    BacktestResults {
        total_pnl: BigDecimal::from(25000),
        num_trades: 380,
        max_drawdown: BigDecimal::from(1800),
        sharpe_ratio: 2.4,
        profit_factor: 1.9,
        win_rate: 0.68,
        avg_trade_return: 0.0042,
        median_trade_return: 0.0035,
        avg_trade_duration_hours: 1.8,
        volatility: 0.25,
        gross_profit: 35000.0,
        gross_loss: 10000.0,
        net_profit: 25000.0,
        backtest_start: Utc::now() - chrono::Duration::days(120),
        backtest_end: Utc::now(),
        signals_generated: 450,
        strategy_metrics: StrategyMetrics::default(),
    }
}
