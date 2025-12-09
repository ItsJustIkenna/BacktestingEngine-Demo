use crate::cli_visualizer::CliVisualizer;
use crate::config::VisualizationConfig;
use backtest::{BacktestResult, HtmlExporter};
use bigdecimal::BigDecimal;
use std::str::FromStr;
use std::path::Path;

pub fn demo_enhanced_cli() {
    println!("🎯 Enhanced CLI Visualizer Demo");
    println!("================================");
    
    // Create mock backtest results to demonstrate visualization
    let result = BacktestResult {
        total_pnl: BigDecimal::from_str("15420.50").unwrap(),
        num_trades: 142,
        closed_trades: 142,
        open_positions: 0,
        realized_pnl: Some(15420.5),
        unrealized_pnl: Some(0.0),
        max_drawdown: BigDecimal::from_str("8300.0").unwrap(),
        sharpe_ratio: Some(1.85),
        sortino_ratio: Some(2.35),
        calmar_ratio: Some(1.86),
        profit_factor: Some(2.1),
        win_rate: Some(68.5),
        avg_trade_return: Some(3.2),
        median_trade_return: Some(2.8),
        avg_trade_duration: Some(4.5),
        volatility: Some(18.3),
        gross_profit: Some(42500.0),
        gross_loss: Some(-27079.5),
        net_profit: Some(15420.5),
        // Mock equity curve showing realistic portfolio progression
        equity_curve: vec![
            100000.0, 101250.0, 99800.0, 102100.0, 103500.0, 101900.0, 
            104200.0, 106800.0, 105100.0, 108200.0, 107000.0, 109500.0,
            111200.0, 108900.0, 112500.0, 114200.0, 115420.5
        ],
        initial_capital: 100000.0,
        data_quality: None,
        transaction_costs: None,
        market_impact: None,
        risk_halt_reason: None,
        trade_returns: vec![],
    };
    
    // Create and use the enhanced visualizer with configuration
    let vis_config = VisualizationConfig::default();
    let visualizer = CliVisualizer::new(vis_config);
    visualizer.display_enhanced_results(&result);
    
    // Export detailed HTML visualization
    println!("\n🌐 Exporting detailed HTML visualization...");
    let html_path = Path::new("enhanced_equity_curve.html");
    match HtmlExporter::export_equity_curve_html(&result, "Mean Reversion Strategy", "BTC/USD", html_path) {
        Ok(()) => {
            println!("✅ HTML export successful!");
            println!("📊 Open {} in your web browser for detailed interactive analysis", html_path.display());
            println!("💡 Features include:");
            println!("   • 📈 Interactive portfolio tracking with hover details");
            println!("   • 📉 Real-time drawdown visualization");
            println!("   • 📊 Daily returns distribution with color coding");
            println!("   • 🎯 Individual trade analysis scatter plot");
            println!("   • 📋 Comprehensive performance metrics table");
            println!("   • 🖱️  Zoom, pan, and export capabilities");
        }
        Err(e) => {
            println!("❌ HTML export failed: {}", e);
        }
    }
}


