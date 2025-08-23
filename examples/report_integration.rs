//! Example showing how to integrate report generation with the backtesting engine
//!
//! This example demonstrates:
//! 1. Using existing OutputConfig from the config module
//! 2. Converting OutputConfig to ReportConfig
//! 3. Generating comprehensive reports from backtest results

use anyhow::Result;
use chrono::NaiveDate;
use config::OutputConfig;
use metrics::{
    PerformanceMetrics, ReportGenerator, ReportConfig, ReportFormat, 
    create_basic_report, EquityPoint
};
use std::path::PathBuf;

/// Convert OutputConfig to ReportConfig for report generation
pub fn create_report_config_from_output_config(output_config: &OutputConfig) -> Result<ReportConfig> {
    let mut formats = Vec::new();
    
    // Convert export formats from strings to ReportFormat enum
    for format_str in &output_config.export_formats {
        match format_str.to_lowercase().as_str() {
            "html" => formats.push(ReportFormat::Html),
            "json" => formats.push(ReportFormat::Json),
            "csv" => formats.push(ReportFormat::Csv),
            "pdf" => formats.push(ReportFormat::Pdf),
            _ => eprintln!("Warning: Unknown export format '{}' ignored", format_str),
        }
    }
    
    // Default to JSON if no valid formats specified
    if formats.is_empty() {
        formats.push(ReportFormat::Json);
    }

    Ok(ReportConfig {
        output_dir: output_config.reports_dir.clone(),
        formats,
        include_trades: true,
        include_charts: output_config.charts_enabled,
        template_dir: None,
        custom_css: None,
    })
}

/// Generate a comprehensive backtest report
pub fn generate_backtest_report(
    strategy_name: &str,
    symbol: &str,
    timeframe: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    initial_capital: f64,
    performance_metrics: PerformanceMetrics,
    equity_curve: Vec<EquityPoint>,
    output_config: &OutputConfig,
) -> Result<Vec<PathBuf>> {
    // Create report configuration from output configuration
    let report_config = create_report_config_from_output_config(output_config)?;
    
    // Create the report generator
    let generator = ReportGenerator::new(report_config);
    
    // Create a basic report from the performance data
    let report = create_basic_report(
        strategy_name.to_string(),
        symbol.to_string(),
        timeframe.to_string(),
        start_date,
        end_date,
        initial_capital,
        performance_metrics,
        equity_curve,
    );
    
    // Generate the report files
    let generated_files = generator.generate_report(&report)?;
    
    println!("Generated {} report file(s):", generated_files.len());
    for file_path in &generated_files {
        println!("  - {}", file_path.display());
    }
    
    Ok(generated_files)
}

/// Example function showing complete integration
pub fn run_backtest_with_reporting_example() -> Result<()> {
    // Example configuration (would normally come from config file)
    let output_config = OutputConfig {
        results_dir: PathBuf::from("./backtest_results"),
        reports_dir: PathBuf::from("./reports"),
        charts_enabled: true,
        export_formats: vec!["html".to_string(), "json".to_string(), "csv".to_string()],
    };
    
    // Example performance metrics (would come from actual backtest)
    let performance_metrics = PerformanceMetrics {
        sharpe_ratio: 1.85,
        profit_factor: 2.3,
        net_profit: 0.42,  // 42% return
        gross_profit: 15000.0,
        gross_loss: -5000.0,
        average_trade_return: 0.025,
        median_trade_return: 0.018,
        number_of_trades: 150,
        max_drawdown: 0.12,  // 12% max drawdown
        volatility: 0.18,
        win_rate: 0.65,      // 65% win rate
        average_trade_duration: 450.0, // 7.5 minutes average
    };
    
    // Example equity curve data (would come from actual backtest)
    let equity_curve = vec![
        EquityPoint {
            timestamp: chrono::Utc::now(),
            equity: 10000.0,
            drawdown: 0.0,
            drawdown_percent: 0.0,
        },
        EquityPoint {
            timestamp: chrono::Utc::now(),
            equity: 14200.0,
            drawdown: -1200.0,
            drawdown_percent: -0.12,
        },
        // More data points would be added during actual backtesting...
    ];
    
    // Generate comprehensive report
    let generated_files = generate_backtest_report(
        "Mean Reversion Strategy",
        "BTCUSD",
        "1h",
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
        10000.0,
        performance_metrics,
        equity_curve,
        &output_config,
    )?;
    
    println!("✅ Report generation completed successfully!");
    println!("📊 Generated files: {:?}", generated_files);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_conversion() {
        let output_config = OutputConfig {
            results_dir: PathBuf::from("./results"),
            reports_dir: PathBuf::from("./reports"),
            charts_enabled: true,
            export_formats: vec!["html".to_string(), "json".to_string()],
        };
        
        let report_config = create_report_config_from_output_config(&output_config).unwrap();
        
        assert_eq!(report_config.output_dir, PathBuf::from("./reports"));
        assert_eq!(report_config.formats.len(), 2);
        assert!(report_config.include_charts);
    }

    #[test]
    fn test_integration_example() {
        // This test would run the full integration example
        // For now, we'll just test that the function exists and can be called
        let result = run_backtest_with_reporting_example();
        // In a real test environment, we'd set up temporary directories
        // and verify the output files are created correctly
        println!("Integration example result: {:?}", result);
    }
}
