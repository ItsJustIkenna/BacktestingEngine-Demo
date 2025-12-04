//! Clean and unified backtest library

// Include logging macros first
#[macro_use]
pub mod logging_facade;

pub mod engine;
pub mod simulation;
pub mod types;
pub mod monte_carlo;
pub mod html_export;

// Re-export main types for easy access
pub use engine::{BacktestEngine, BacktestResults, AnalysisMetadata, WalkForwardSummary};
pub use types::{BacktestResult, BacktestEvent};
pub use html_export::HtmlExporter;

// Utility functions for report generation
use metrics::{ReportConfig, ReportFormat};
use config::OutputConfig;
use anyhow::Result;
use std::path::PathBuf;

/// Convert OutputConfig to ReportConfig for report generation
pub fn create_report_config(output_config: &OutputConfig) -> Result<ReportConfig> {
    let mut formats = Vec::new();
    
    // Convert export formats from strings to ReportFormat enum
    for format_str in &output_config.export_formats {
        match format_str.to_lowercase().as_str() {
            "html" => formats.push(ReportFormat::Html),
            "json" => formats.push(ReportFormat::Json),
            "csv" => formats.push(ReportFormat::Csv),
            "pdf" => formats.push(ReportFormat::Pdf),
            _ => {
                logging_facade::BACKTEST_LOGGER.warn_sync(
                    format!("Unknown export format '{}' ignored", format_str)
                );
            }
        }
    }
    
    // Default to JSON if no valid formats specified
    if formats.is_empty() {
        formats.push(ReportFormat::Json);
    }

    Ok(ReportConfig {
        output_dir: PathBuf::from(&output_config.reports_dir),
        formats,
        include_trades: true,
        include_charts: output_config.charts_enabled,
        template_dir: None,
        custom_css: None,
        persist_to_database: true,  // Enable by default
        database_url: std::env::var("DATABASE_URL").ok(),  // Read from environment
        generated_by: std::env::var("USER").ok().or_else(|| std::env::var("USERNAME").ok()),
        generation_source: "backtest_engine".to_string(),
    })
}
