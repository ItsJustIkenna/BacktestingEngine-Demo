use strategy::simple_export::{StrategyExporter, ExportConfig};
use strategy::BacktestResults;
use bigdecimal::BigDecimal;
use chrono::{Utc, Duration};
use std::collections::HashMap;
use anyhow::Result;

/// Demo of custom export configuration
#[tokio::main]
async fn main() -> Result<()> {
    println!("⚙️  Export Configuration Demo");
    println!("==============================\n");

    // 1. Create custom export configuration
    let export_config = ExportConfig {
        min_sharpe_ratio: 1.8,
        min_profit_factor: 1.5,
        min_win_rate: 65.0,
        export_path: "./custom_exports".to_string(),
    };

    println!("🔧 Custom Export Configuration:");
    println!("  Min Sharpe Ratio: {}", export_config.min_sharpe_ratio);
    println!("  Min Profit Factor: {}", export_config.min_profit_factor);
    println!("  Min Win Rate: {}%", export_config.min_win_rate);
    println!("  Export Path: {}\n", export_config.export_path);

    // 2. Create sample backtest results
    let backtest_results = BacktestResults {
        total_pnl: BigDecimal::from(5200),
        num_trades: 89,
        max_drawdown: BigDecimal::from(650),
        sharpe_ratio: 1.9,
        profit_factor: 1.6,
        win_rate: 67.0,
        avg_trade_return: 0.0035,
        median_trade_return: 0.0028,
        avg_trade_duration_hours: 3.2,
        volatility: 0.25,
        gross_profit: 9800.0,
        gross_loss: 4600.0,
        net_profit: 5200.0,
        signals_generated: 120,
        strategy_metrics: strategy::simple_export::StrategyMetrics::default(),
        backtest_start: Utc::now() - Duration::days(45),
        backtest_end: Utc::now(),
    };

    // 3. Initialize exporter with custom config
    let exporter = StrategyExporter::with_config(export_config);

    // 4. Test evaluation
    println!("🔍 Evaluating strategy with custom thresholds...");
    let evaluation = exporter.should_export(&backtest_results);
    
    if evaluation {
        println!("✅ Strategy PASSED custom criteria!");
    } else {
        println!("❌ Strategy FAILED custom criteria");
    }

    // 5. Export attempt
    let mut parameters = HashMap::new();
    parameters.insert("fast_ma".to_string(), 10.0);
    parameters.insert("slow_ma".to_string(), 30.0);

    let export_result = exporter.export_if_qualified(
        "CustomMAStrategy",
        "ETHUSDT",
        "binance",
        parameters,
        backtest_results,
    ).await?;

    match export_result {
        Some(path) => println!("📁 Exported to: {}", path),
        None => println!("📋 Strategy did not meet export criteria"),
    }

    println!("\n✅ Configuration demo completed!");
    Ok(())
}
