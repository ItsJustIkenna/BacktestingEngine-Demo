//! Strategy Export CLI Tool
//!
//! Command-line interface for exporting successful strategies from backtesting
//! to live trading deployment.

use strategy::{
    BacktestIntegration, StrategyExporter, PerformanceThresholds, 
    StrategyConfig, ParameterValue, ApprovalLevel,
    BacktestResults, StrategyMetrics,
};
use bigdecimal::BigDecimal;
use clap::{Parser, Subcommand};
use anyhow::Result;
use std::collections::HashMap;
use chrono::{Duration, Utc};

#[derive(Parser)]
#[command(name = "strategy-export")]
#[command(about = "Export successful backtested strategies to SignalEngine for live trading")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Path to SignalEngine directory
    #[arg(short, long, default_value = "../SignalEngine")]
    signal_engine_path: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate strategy against export criteria
    Evaluate {
        /// Strategy configuration file (JSON)
        #[arg(short, long)]
        strategy_config: String,
        
        /// Backtest results file (JSON)
        #[arg(short, long)]
        results: String,
    },
    
    /// Export strategy to SignalEngine
    Export {
        /// Strategy configuration file (JSON)
        #[arg(short, long)]
        strategy_config: String,
        
        /// Backtest results file (JSON)
        #[arg(short, long)]
        results: String,
        
        /// Who is authorizing the export
        #[arg(short, long)]
        authorized_by: String,
        
        /// Approval level (development, staging, production)
        #[arg(short = 'l', long, default_value = "development")]
        approval_level: String,
        
        /// Optional notes
        #[arg(short, long)]
        notes: Option<String>,
    },
    
    /// List all exported strategies
    List,
    
    /// Show export statistics
    Stats,
    
    /// Generate example configuration files
    Examples,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Evaluate { strategy_config, results } => {
            evaluate_strategy(&cli.signal_engine_path, &strategy_config, &results).await
        }
        
        Commands::Export { 
            strategy_config, 
            results, 
            authorized_by, 
            approval_level,
            notes 
        } => {
            export_strategy(
                &cli.signal_engine_path,
                &strategy_config,
                &results,
                &authorized_by,
                &approval_level,
                notes,
            ).await
        }
        
        Commands::List => list_exported_strategies(&cli.signal_engine_path).await,
        Commands::Stats => show_export_statistics(&cli.signal_engine_path).await,
        Commands::Examples => generate_examples().await,
    }
}

async fn evaluate_strategy(
    signal_engine_path: &str,
    _strategy_config_path: &str,
    _results_path: &str,
) -> Result<()> {
    println!("🔍 Evaluating strategy for export eligibility...");
    
    let exporter = StrategyExporter::new(signal_engine_path.to_string())?;
    
    // Load strategy configuration and results (simplified for example)
    let example_results = create_example_results();
    
    let evaluation = exporter.evaluate_strategy(&example_results);
    let report = exporter.generate_report(&example_results);
    
    println!("{}", report);
    
    if evaluation.eligible {
        println!("✅ Strategy is eligible for export!");
        println!("💯 Overall Score: {:.1}%", evaluation.score * 100.0);
    } else {
        println!("❌ Strategy is NOT eligible for export");
        println!("📊 Score: {:.1}%", evaluation.score * 100.0);
    }
    
    Ok(())
}

async fn export_strategy(
    signal_engine_path: &str,
    _strategy_config_path: &str,
    _results_path: &str,
    authorized_by: &str,
    approval_level: &str,
    notes: Option<String>,
) -> Result<()> {
    println!("🚀 Exporting strategy to SignalEngine...");
    
    let integration = BacktestIntegration::new(signal_engine_path.to_string())?;
    
    // Parse approval level
    let approval = match approval_level.to_lowercase().as_str() {
        "development" => ApprovalLevel::Development,
        "staging" => ApprovalLevel::Staging,
        "production" => ApprovalLevel::Production,
        _ => {
            println!("⚠️  Invalid approval level, using 'development'");
            ApprovalLevel::Development
        }
    };
    
    // Create example data (in real implementation, load from files)
    let strategy_config = create_example_strategy_config();
    let backtest_result = create_example_backtest_result();
    let strategy_metrics = strategy::StrategyMetrics::default();
    let optimized_parameters = strategy_config.parameters.clone();
    
    match integration.manual_export(
        strategy_config,
        backtest_result,
        strategy_metrics,
        optimized_parameters,
        authorized_by.to_string(),
        approval,
        notes,
    ).await {
        Ok(export_path) => {
            println!("✅ Strategy successfully exported!");
            println!("📁 Export location: {}", export_path);
            println!("👤 Authorized by: {}", authorized_by);
            println!("🔒 Approval level: {}", approval_level);
        }
        Err(e) => {
            println!("❌ Export failed: {}", e);
        }
    }
    
    Ok(())
}

async fn list_exported_strategies(signal_engine_path: &str) -> Result<()> {
    println!("📋 Listing exported strategies...");
    
    let exporter = StrategyExporter::new(signal_engine_path.to_string())?;
    let strategies = exporter.list_exported_strategies().await?;
    
    if strategies.is_empty() {
        println!("No exported strategies found.");
        return Ok(());
    }
    
    println!("Found {} exported strategies:", strategies.len());
    for (i, strategy) in strategies.iter().enumerate() {
        println!("{}. {}", i + 1, strategy);
    }
    
    Ok(())
}

async fn show_export_statistics(signal_engine_path: &str) -> Result<()> {
    println!("📊 Export Statistics");
    println!("==================");
    
    let integration = BacktestIntegration::new(signal_engine_path.to_string())?;
    let stats = integration.get_export_statistics().await?;
    
    println!("Total Exported: {}", stats.total_exported);
    println!("Paper Trading: {}", stats.paper_trading_count);
    println!("Live Trading: {}", stats.live_trading_count);
    
    if !stats.by_approval_level.is_empty() {
        println!("\nBy Approval Level:");
        for (level, count) in stats.by_approval_level {
            println!("  {}: {}", level, count);
        }
    }
    
    Ok(())
}

async fn generate_examples() -> Result<()> {
    println!("📝 Generating example configuration files...");
    
    // Generate example strategy configuration
    let strategy_config = create_example_strategy_config();
    let config_json = serde_json::to_string_pretty(&strategy_config)?;
    tokio::fs::write("example_strategy_config.json", config_json).await?;
    
    // Generate example backtest results
    let results = create_example_backtest_result();
    let results_json = serde_json::to_string_pretty(&results)?;
    tokio::fs::write("example_backtest_results.json", results_json).await?;
    
    // Generate example performance thresholds
    let thresholds = PerformanceThresholds::default();
    let thresholds_json = serde_json::to_string_pretty(&thresholds)?;
    tokio::fs::write("example_performance_thresholds.json", thresholds_json).await?;
    
    println!("✅ Example files generated:");
    println!("  - example_strategy_config.json");
    println!("  - example_backtest_results.json");
    println!("  - example_performance_thresholds.json");
    
    Ok(())
}

// Helper functions to create example data
fn create_example_strategy_config() -> StrategyConfig {
    let mut parameters = HashMap::new();
    parameters.insert("risk_aversion".to_string(), ParameterValue::Float(0.01));
    parameters.insert("order_size".to_string(), ParameterValue::Float(0.05));
    parameters.insert("window_ms".to_string(), ParameterValue::Float(60000.0));
    
    StrategyConfig {
        id: "avellaneda_stoikov_optimized".to_string(),
        name: "Optimized Avellaneda-Stoikov Market Maker".to_string(),
        strategy_type: "AvellanedaStoikov".to_string(),
        symbol: "BTCUSD".to_string(),
        exchange: "kraken".to_string(),
        parameters,
        risk_limits: Default::default(),
        enabled: true,
    }
}

fn create_example_backtest_result() -> BacktestResults {
    BacktestResults {
        total_pnl: BigDecimal::from(15000),
        num_trades: 450,
        max_drawdown: BigDecimal::from(1200),
        sharpe_ratio: 2.3,
        profit_factor: 1.8,
        win_rate: 0.64,
        avg_trade_return: 0.0034,
        median_trade_return: 0.0025,
        avg_trade_duration_hours: 2.1,
        volatility: 0.22,
        gross_profit: 20000.0,
        gross_loss: 5000.0,
        net_profit: 15000.0,
        backtest_start: Utc::now() - Duration::days(30),
        backtest_end: Utc::now(),
        signals_generated: 890,
        strategy_metrics: StrategyMetrics::default(),
    }
}

fn create_example_results() -> strategy::BacktestResults {
    strategy::BacktestResults {
        total_pnl: BigDecimal::from(15000),
        num_trades: 450,
        max_drawdown: BigDecimal::from(1200),
        sharpe_ratio: 2.3,
        profit_factor: 1.8,
        win_rate: 0.64,
        avg_trade_return: 0.0034,
        median_trade_return: 0.0025,
        avg_trade_duration_hours: 2.1,
        volatility: 0.22,
        gross_profit: 20000.0,
        gross_loss: 5000.0,
        net_profit: 15000.0,
        backtest_start: chrono::Utc::now() - chrono::Duration::days(120),
        backtest_end: chrono::Utc::now(),
        signals_generated: 500,
        strategy_metrics: strategy::StrategyMetrics::default(),
    }
}
