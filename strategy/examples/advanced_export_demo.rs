use chrono::{DateTime, Utc};
use bigdecimal::BigDecimal;
use std::collections::HashMap;
use std::str::FromStr;

use strategy::simple_export::{
    StrategyExporter, ExportableStrategy, BacktestResults, StrategyConfig, StrategyMetrics,
    ExportConfig, ParameterValue, PerformanceData, LiveTradingConfig, ExportMetadata
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Advanced Export System Demo\n");

    // Demo 1: Using different export presets
    println!("=== Demo 1: Export Presets ===");
    
    let conservative_exporter = StrategyExporter::with_preset("conservative")?;
    let aggressive_exporter = StrategyExporter::with_preset("aggressive")?;
    let research_exporter = StrategyExporter::with_preset("research")?;
    
    // Create some demo strategies with varying performance
    let demo_strategies = create_demo_strategies();
    
    // Show export statistics for different presets
    println!("Conservative criteria:");
    let conservative_stats = conservative_exporter.get_export_stats(&demo_strategies);
    conservative_stats.print_summary();
    
    println!("\nAggressive criteria:");
    let aggressive_stats = aggressive_exporter.get_export_stats(&demo_strategies);
    aggressive_stats.print_summary();
    
    println!("\nResearch criteria:");
    let research_stats = research_exporter.get_export_stats(&demo_strategies);
    research_stats.print_summary();

    // Demo 2: Batch export with detailed reporting
    println!("\n\n=== Demo 2: Batch Export ===");
    
    let _batch_result = aggressive_exporter.export_batch(demo_strategies).await?;
    
    // Demo 3: Custom export criteria
    println!("\n\n=== Demo 3: Custom Export Criteria ===");
    
    let custom_config = ExportConfig {
        min_sharpe_ratio: 1.8,
        min_profit_factor: 1.5,
        min_win_rate: 0.65,
        max_drawdown_pct: 0.12,
        min_trades: 80,
        export_path: "C:/Users/ikenn/Projects/TradingPlatform/SignalEngine/exported_strategies".to_string(),
    };
    
    let custom_exporter = StrategyExporter::with_config(custom_config);
    
    // Test specific strategy against custom criteria
    let test_strategy = create_test_strategy("Custom Test Strategy", 1.9, 1.6, 0.68, 0.08, 150);
    let (should_export, reason) = custom_exporter.should_export_with_reason(&test_strategy.1);
    
    if should_export {
        println!("✅ Custom strategy PASSES export criteria");
        let export_result = custom_exporter.export_if_qualified(
            &test_strategy.0.config.name,
            &test_strategy.0.config.symbol,
            &test_strategy.0.config.exchange,
            HashMap::new(), // Empty parameters for simplicity
            test_strategy.1
        ).await?;
        if let Some(path) = export_result {
            println!("   Exported to: {}", path);
        }
    } else {
        println!("❌ Custom strategy FAILS export criteria");
        println!("   Reason: {}", reason.unwrap_or("Unknown".to_string()));
    }

    // Demo 4: List exported strategies
    println!("\n\n=== Demo 4: Exported Strategies List ===");
    
    let exported_files = aggressive_exporter.list_exported().await?;
    println!("Found {} exported strategies:", exported_files.len());
    for (i, filename) in exported_files.iter().enumerate() {
        println!("  {}. {}", i + 1, filename);
    }

    println!("\n🎉 Advanced export demo completed!");
    Ok(())
}

fn create_demo_strategies() -> Vec<(ExportableStrategy, BacktestResults)> {
    vec![
        create_test_strategy("Elite MACD", 2.5, 2.1, 0.75, 0.05, 200),
        create_test_strategy("Good RSI", 1.8, 1.6, 0.68, 0.08, 150),
        create_test_strategy("Average Bollinger", 1.2, 1.3, 0.62, 0.12, 80),
        create_test_strategy("Mediocre SMA", 0.8, 1.1, 0.55, 0.18, 60),
        create_test_strategy("Poor EMA", 0.3, 0.9, 0.48, 0.25, 40),
        create_test_strategy("Experimental Stoch", 1.4, 1.4, 0.64, 0.15, 90),
    ]
}

fn create_test_strategy(
    name: &str, 
    sharpe: f64, 
    profit_factor: f64, 
    win_rate: f64, 
    drawdown: f64, 
    trades: u64
) -> (ExportableStrategy, BacktestResults) {
    let mut parameters = HashMap::new();
    parameters.insert("period".to_string(), ParameterValue::Float(14.0));
    parameters.insert("threshold".to_string(), ParameterValue::Float(0.02));
    
    let strategy = ExportableStrategy {
        config: StrategyConfig {
            id: format!("{}-{}", name.to_lowercase().replace(" ", "-"), "001"),
            name: name.to_string(),
            strategy_type: "Technical Analysis".to_string(),
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            enabled: true,
            parameters: parameters.clone(),
        },
        performance: PerformanceData {
            total_pnl: "15000".to_string(),
            sharpe_ratio: sharpe,
            profit_factor,
            win_rate: win_rate,
            max_drawdown: format!("-{:.0}", drawdown * 10000.0),
            num_trades: trades,
            avg_trade_return: 1.5,
            volatility: 0.15,
            avg_trade_duration_hours: 24.0,
            backtest_start: Utc::now() - chrono::Duration::days(90),
            backtest_end: Utc::now(),
        },
        optimized_parameters: parameters,
        live_trading_config: LiveTradingConfig::default(),
        export_metadata: ExportMetadata {
            export_time: Utc::now(),
            strategy_version: "1.0.0".to_string(),
            engine_version: "0.1.0".to_string(),
            authorized_by: "demo".to_string(),
            approval_level: "Testing".to_string(),
            notes: "Demo strategy".to_string(),
        },
    };
    
    let results = BacktestResults {
        total_pnl: BigDecimal::from_str("15000").unwrap(),
        num_trades: trades,
        max_drawdown: BigDecimal::from_str(&format!("{:.0}", drawdown * 100.0)).unwrap(), // Convert to proper percentage
        sharpe_ratio: sharpe,
        profit_factor,
        win_rate: win_rate * 100.0, // Convert to percentage for BacktestResults
        avg_trade_return: 1.5,
        median_trade_return: 1.2,
        volatility: 0.15,
        avg_trade_duration_hours: 24.0,
        backtest_start: Utc::now() - chrono::Duration::days(90),
        backtest_end: Utc::now(),
        gross_profit: 25000.0,
        gross_loss: -10000.0,
        net_profit: 15000.0,
        signals_generated: trades,
        strategy_metrics: StrategyMetrics::default(),
    };
    
    (strategy, results)
}
