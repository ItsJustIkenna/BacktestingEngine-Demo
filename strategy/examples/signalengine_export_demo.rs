use strategy::simple_export::{StrategyExporter, ExportConfig, BacktestResults, StrategyMetrics};
use strategy::integration::BacktestIntegration;
use config::{StrategyConfig, ParameterValue};
use bigdecimal::BigDecimal;
use chrono::{Utc, Duration};
use std::collections::HashMap;
use anyhow::Result;

/// Demo showing BacktestingEngine export to SignalEngine
#[tokio::main]
async fn main() -> Result<()> {
    println!("🔄 BacktestingEngine → SignalEngine Export Demo");
    println!("===============================================\n");

    // 1. Show current export configuration
    let config = ExportConfig::default();
    println!("📁 Export Configuration:");
    println!("  Target Directory: {}", config.export_path);
    println!("  Min Sharpe Ratio: {}", config.min_sharpe_ratio);
    println!("  Min Profit Factor: {}", config.min_profit_factor);
    println!("  Min Win Rate: {}%\n", config.min_win_rate);

    // 2. Create a successful strategy
    let backtest_results = BacktestResults {
        total_pnl: BigDecimal::from(12500),
        num_trades: 187,
        max_drawdown: BigDecimal::from(850),
        sharpe_ratio: 2.4,
        profit_factor: 1.9,
        win_rate: 72.5,
        avg_trade_return: 0.0067,
        median_trade_return: 0.005,
        avg_trade_duration_hours: 1.8,
        volatility: 0.19,
        gross_profit: 18500.0,
        gross_loss: 6000.0,
        net_profit: 12500.0,
        signals_generated: 210,
        strategy_metrics: StrategyMetrics::default(),
        backtest_start: Utc::now() - Duration::days(60),
        backtest_end: Utc::now(),
    };

    println!("📊 Strategy Performance:");
    println!("  Total PnL: $12,500");
    println!("  Sharpe Ratio: {:.2} ✅", backtest_results.sharpe_ratio);
    println!("  Profit Factor: {:.2} ✅", backtest_results.profit_factor);
    println!("  Win Rate: {}% ✅", backtest_results.win_rate);
    println!("  Trades: {}\n", backtest_results.num_trades);

    // 3. Set up strategy parameters
    let mut parameters = HashMap::new();
    parameters.insert("fast_ma".to_string(), 12.0);
    parameters.insert("slow_ma".to_string(), 26.0);
    parameters.insert("signal_ma".to_string(), 9.0);
    parameters.insert("position_size".to_string(), 0.15);

    // 4. Export using the simple export system
    println!("🚀 Exporting to SignalEngine...");
    let exporter = StrategyExporter::new();
    
    let export_path = exporter.export_if_qualified(
        "MACD Crossover",
        "BTCUSDT",
        "binance",
        parameters,
        backtest_results,
    ).await?;

    match export_path {
        Some(path) => {
            println!("✅ SUCCESS: Strategy exported to SignalEngine!");
            println!("📁 File location: {}", path);
            
            // 5. Verify export was successful
            println!("\n🔍 Verification:");
            let exported_strategies = exporter.list_exported_strategies().await?;
            println!("  Exported strategies: {}", exported_strategies.len());
            for strategy in exported_strategies {
                println!("  • {}", strategy);
            }
        },
        None => {
            println!("❌ Strategy did not meet export criteria (this shouldn't happen with our test data)");
        }
    }

    // 6. Integration test
    println!("\n🔄 Integration Test:");
    let mut integration = BacktestIntegration::new()?;
    integration.enable_auto_export(None);
    
    let strategy_config = create_test_config();
    let export_result = integration.would_export(&create_test_results());
    
    println!("  Would auto-export: {}", if export_result { "✅ YES" } else { "❌ NO" });
    println!("  Export path configured: {}", integration.get_export_config().export_path);

    println!("\n✅ Export demo completed!");
    println!("📍 Check ../SignalEngine/exported_strategies/ directory for exported files");
    
    Ok(())
}

fn create_test_config() -> StrategyConfig {
    let mut config = StrategyConfig::default();
    config.name = "Test Strategy".to_string();
    config.symbol = "ETHUSDT".to_string();
    config.exchange = "binance".to_string();
    config.parameters.insert("test_param".to_string(), ParameterValue::Float(1.0));
    config
}

fn create_test_results() -> BacktestResults {
    BacktestResults {
        total_pnl: BigDecimal::from(8000),
        num_trades: 150,
        max_drawdown: BigDecimal::from(500),
        sharpe_ratio: 2.1,
        profit_factor: 1.7,
        win_rate: 65.0,
        avg_trade_return: 0.0053,
        median_trade_return: 0.004,
        avg_trade_duration_hours: 2.2,
        volatility: 0.21,
        gross_profit: 12000.0,
        gross_loss: 4000.0,
        net_profit: 8000.0,
        signals_generated: 180,
        strategy_metrics: StrategyMetrics::default(),
        backtest_start: Utc::now() - Duration::days(45),
        backtest_end: Utc::now(),
    }
}
