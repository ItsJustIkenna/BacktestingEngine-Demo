//! Report generation module for backtesting results
//!
//! Provides comprehensive report generation in multiple formats including
//! HTML, CSV, JSON, and PDF for backtesting analysis and visualization.

use crate::PerformanceMetrics;
// Logging imports available when needed
#[allow(unused_imports)]
use crate::logging_facade::METRICS_LOGGER;
#[allow(unused_imports)]
use crate::{log_info, log_warn, log_error};
use config::{ReportConfig, ReportFormat};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc, NaiveDate};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Complete backtest report containing all analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestReport {
    /// Report metadata
    pub metadata: ReportMetadata,
    /// Strategy performance metrics
    pub performance: PerformanceMetrics,
    /// Additional analysis sections
    pub analysis: BacktestAnalysis,
    /// Trade-level data (optional for detailed reports)
    pub trades: Option<Vec<TradeRecord>>,
    /// Equity curve data points
    pub equity_curve: Vec<EquityPoint>,
    /// Drawdown periods
    pub drawdown_periods: Vec<DrawdownPeriod>,
}

/// Report metadata and configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetadata {
    pub report_id: String,
    pub strategy_name: String,
    pub symbol: String,
    pub timeframe: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub initial_capital: f64,
    pub generated_at: DateTime<Utc>,
    pub backtest_duration_seconds: Option<f64>,
    pub data_points: usize,
}

/// Additional analysis beyond basic metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestAnalysis {
    /// Monthly returns breakdown
    pub monthly_returns: HashMap<String, f64>,
    /// Risk-adjusted metrics
    pub risk_metrics: RiskMetrics,
    /// Trade distribution analysis
    pub trade_distribution: TradeDistribution,
    /// Performance attribution
    pub attribution: PerformanceAttribution,
    /// Benchmark comparison (if available)
    pub benchmark_comparison: Option<BenchmarkComparison>,
    /// Data quality metrics
    pub data_quality: DataQualityMetrics,
    /// Market impact analysis
    pub market_impact: MarketImpactMetrics,
    /// Execution quality metrics
    pub execution_quality: ExecutionQualityMetrics,
}

/// Data quality metrics for the backtest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQualityMetrics {
    pub overall_score: f64,
    pub data_completeness_score: f64,
    pub gaps_detected: usize,
    pub max_gap_seconds: i64,
    pub gap_ratio: f64,
}

/// Market impact analysis metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketImpactMetrics {
    pub trades_with_impact: usize,
    pub avg_impact_bps: f64,
    pub max_impact_bps: f64,
    pub liquidity_warnings: usize,
}

/// Execution quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionQualityMetrics {
    pub avg_slippage_bps: f64,
    pub total_slippage_cost: f64,
    pub fill_rate: f64,
    pub avg_time_to_fill_seconds: f64,
}

/// Risk-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub value_at_risk_95: f64,
    pub conditional_var_95: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    pub max_consecutive_losses: usize,
    pub max_consecutive_wins: usize,
    pub tail_ratio: f64,
}

/// Trade distribution statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeDistribution {
    pub profitable_trades: usize,
    pub losing_trades: usize,
    pub break_even_trades: usize,
    pub best_trade: f64,
    pub worst_trade: f64,
    pub avg_winning_trade: f64,
    pub avg_losing_trade: f64,
    pub largest_winning_streak: usize,
    pub largest_losing_streak: usize,
}

/// Performance attribution analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAttribution {
    pub long_trades_pnl: f64,
    pub short_trades_pnl: f64,
    pub commission_paid: f64,
    pub slippage_cost: f64,
    pub market_impact_cost: f64,
}

/// Benchmark comparison results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub benchmark_name: String,
    pub benchmark_return: f64,
    pub excess_return: f64,
    pub tracking_error: f64,
    pub information_ratio: f64,
    pub beta: f64,
    pub alpha: f64,
    pub correlation: f64,
}

/// Individual trade record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub trade_id: String,
    pub symbol: String,
    pub side: String,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub quantity: f64,
    pub pnl: f64,
    pub pnl_percent: f64,
    pub duration_seconds: i64,
    pub commission: f64,
    pub slippage: f64,
}

/// Equity curve data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: DateTime<Utc>,
    pub equity: f64,
    pub drawdown: f64,
    pub drawdown_percent: f64,
}

/// Drawdown period information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawdownPeriod {
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
    pub recovery_date: Option<DateTime<Utc>>,
    pub duration_days: i64,
    pub magnitude: f64,
    pub magnitude_percent: f64,
}

/// Main report generator
pub struct ReportGenerator {
    config: ReportConfig,
}

impl ReportGenerator {
    /// Create a new report generator
    pub fn new(config: ReportConfig) -> Self {
        Self { config }
    }

    /// Generate a comprehensive backtest report
    pub fn generate_report(&self, report: &BacktestReport) -> Result<Vec<PathBuf>> {
        // Ensure output directory exists
        fs::create_dir_all(&self.config.output_dir)?;

        let mut generated_files = Vec::new();
        let mut file_paths_map = std::collections::HashMap::new();

        for format in &self.config.formats {
            let file_path = match format {
                ReportFormat::Html => self.generate_html_report(report)?,
                ReportFormat::Json => self.generate_json_report(report)?,
                ReportFormat::Csv => self.generate_csv_report(report)?,
                ReportFormat::Pdf => self.generate_pdf_report(report)?,
            };
            
            file_paths_map.insert(format.to_string(), file_path.to_string_lossy().to_string());
            generated_files.push(file_path);
        }

        // Optionally persist to database
        #[cfg(feature = "persistence")]
        if self.config.persist_to_database {
            if let Err(e) = self.persist_report_metadata(report, &file_paths_map) {
                log_warn!(METRICS_LOGGER, "Failed to persist report to database: {}", e);
                // Don't fail the entire operation, just log the warning
            }
        }

        Ok(generated_files)
    }

    /// Persist report metadata to database
    #[cfg(feature = "persistence")]
    fn persist_report_metadata(
        &self,
        report: &BacktestReport,
        file_paths: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        use crate::persistence::NewBacktestReportRecord;
        use uuid::Uuid;
        
        log_info!(METRICS_LOGGER, "Persisting report metadata - Report ID: {}, Strategy: {}", 
            report.metadata.report_id, report.metadata.strategy_name);
        
        // Check if database URL is configured
        if self.config.database_url.is_none() {
            log_warn!(METRICS_LOGGER, "DATABASE_URL not configured, skipping persistence");
            return Ok(());
        }
        
        // Create backtest_result_id from report metadata or generate new
        // In real usage, this should come from the actual backtest run
        let backtest_result_id = Uuid::new_v4();
        
        // Create database record from report
        let new_record = NewBacktestReportRecord::from_backtest_report(
            report,
            backtest_result_id,
            file_paths.clone(),
            self.config.generated_by.clone(),
            self.config.generation_source.clone(),
        );
        
        // Save to database using async runtime
        log_info!(METRICS_LOGGER, "Saving report to backtest_reports table");
        
        let database_url = self.config.database_url.as_ref().unwrap();
        let save_result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Create connection pool
                use diesel_async::AsyncPgConnection;
                use diesel_async::pooled_connection::AsyncDieselConnectionManager;
                let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
                let pool = diesel_async::pooled_connection::deadpool::Pool::builder(config)
                    .max_size(2)
                    .build()
                    .map_err(|e| anyhow::anyhow!("Failed to create pool: {}", e))?;
                
                let pool = std::sync::Arc::new(pool);
                let repo = crate::persistence::BacktestReportRepository::new(pool);
                
                repo.save_report(new_record.clone()).await
                    .map_err(|e| anyhow::anyhow!("Database error: {}", e))
            })
        });
        
        match save_result {
            Ok(()) => {
                log_info!(METRICS_LOGGER, "Report saved successfully - backtest_result_id: {}, report_id: {}, strategy: {}, symbol: {}, period: {} to {}, sharpe: {:?}, total_trades: {}",
                    backtest_result_id, new_record.report_id, new_record.strategy_name, new_record.symbol,
                    new_record.start_date, new_record.end_date, report.performance.sharpe_ratio, report.performance.number_of_trades);
            }
            Err(e) => {
                log_error!(METRICS_LOGGER, "Failed to save report: {} - continuing without database persistence", e);
            }
        }
        // let repo = BacktestReportRepository::new(pool);
        // tokio::runtime::Runtime::new()?.block_on(async {
        //     repo.save_report(new_record).await
        // })?;
        
        Ok(())
    }

    /// Generate HTML report with charts and styling
    fn generate_html_report(&self, report: &BacktestReport) -> Result<PathBuf> {
        let file_path = self.config.output_dir.join(format!("{}.html", report.metadata.report_id));
        
        let html_content = self.create_html_content(report)?;
        fs::write(&file_path, html_content)?;
        
        Ok(file_path)
    }

    /// Generate JSON report
    fn generate_json_report(&self, report: &BacktestReport) -> Result<PathBuf> {
        let file_path = self.config.output_dir.join(format!("{}.json", report.metadata.report_id));
        
        let json_content = serde_json::to_string_pretty(report)?;
        fs::write(&file_path, json_content)?;
        
        Ok(file_path)
    }

    /// Generate CSV report (summary metrics)
    fn generate_csv_report(&self, report: &BacktestReport) -> Result<PathBuf> {
        let file_path = self.config.output_dir.join(format!("{}.csv", report.metadata.report_id));
        
        let csv_content = self.create_csv_content(report)?;
        fs::write(&file_path, csv_content)?;
        
        Ok(file_path)
    }

    /// Generate PDF report (requires additional dependencies)
    fn generate_pdf_report(&self, _report: &BacktestReport) -> Result<PathBuf> {
        // For now, return an error indicating PDF generation needs additional setup
        Err(anyhow!("PDF generation requires additional dependencies (wkhtmltopdf or similar)"))
    }

    /// Create HTML content for the report
    fn create_html_content(&self, report: &BacktestReport) -> Result<String> {
        let mut html = String::new();
        
        // HTML head with styling
        html.push_str(&format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Backtest Report - {}</title>
    <style>
        body {{
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            margin: 0;
            padding: 20px;
            background-color: #f5f5f5;
            color: #333;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
            overflow: hidden;
        }}
        .header {{
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 30px;
            text-align: center;
        }}
        .header h1 {{
            margin: 0;
            font-size: 2.5em;
            font-weight: 300;
        }}
        .header p {{
            margin: 10px 0 0 0;
            opacity: 0.9;
            font-size: 1.1em;
        }}
        .content {{
            padding: 30px;
        }}
        .metrics-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }}
        .metric-card {{
            background: #f8f9fa;
            border: 1px solid #e9ecef;
            border-radius: 6px;
            padding: 20px;
            text-align: center;
        }}
        .metric-value {{
            font-size: 2em;
            font-weight: bold;
            color: #495057;
            margin-bottom: 5px;
        }}
        .metric-label {{
            color: #6c757d;
            font-size: 0.9em;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }}
        .positive {{ color: #28a745; }}
        .negative {{ color: #dc3545; }}
        .section {{
            margin-bottom: 30px;
        }}
        .section h2 {{
            border-bottom: 2px solid #e9ecef;
            padding-bottom: 10px;
            margin-bottom: 20px;
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            margin-bottom: 20px;
        }}
        th, td {{
            text-align: left;
            padding: 12px;
            border-bottom: 1px solid #e9ecef;
        }}
        th {{
            background-color: #f8f9fa;
            font-weight: 600;
        }}
        {}
    </style>
</head>
<body>
"#, report.metadata.strategy_name, self.config.custom_css.as_deref().unwrap_or("")));

        // Header section
        html.push_str(&format!(r#"
    <div class="container">
        <div class="header">
            <h1>{}</h1>
            <p>{} | {} | {} to {}</p>
        </div>
        <div class="content">
"#, 
            report.metadata.strategy_name,
            report.metadata.symbol,
            report.metadata.timeframe,
            report.metadata.start_date,
            report.metadata.end_date
        ));

        // Key metrics section
        html.push_str(r#"
            <div class="section">
                <h2>Key Performance Metrics</h2>
                <div class="metrics-grid">
"#);

        // Add key metrics cards
        let metrics = &report.performance;
        self.add_metric_card(&mut html, "Total Return", &format!("{:.2}%", metrics.net_profit * 100.0), metrics.net_profit >= 0.0);
        self.add_metric_card(&mut html, "Sharpe Ratio", &format!("{:.3}", metrics.sharpe_ratio), metrics.sharpe_ratio >= 1.0);
        self.add_metric_card(&mut html, "Max Drawdown", &format!("{:.2}%", metrics.max_drawdown * 100.0), false);
        self.add_metric_card(&mut html, "Win Rate", &format!("{:.1}%", metrics.win_rate * 100.0), metrics.win_rate >= 0.5);
        self.add_metric_card(&mut html, "Profit Factor", &format!("{:.2}", metrics.profit_factor), metrics.profit_factor >= 1.0);
        self.add_metric_card(&mut html, "Total Trades", &format!("{}", metrics.number_of_trades), true);

        html.push_str("                </div>\n            </div>\n");

        // Risk metrics section
        html.push_str(&format!(r#"
            <div class="section">
                <h2>Risk Analysis</h2>
                <table>
                    <tr><th>Metric</th><th>Value</th></tr>
                    <tr><td>Volatility</td><td>{:.2}%</td></tr>
                    <tr><td>Value at Risk (95%)</td><td>{:.2}%</td></tr>
                    <tr><td>Sortino Ratio</td><td>{:.3}</td></tr>
                    <tr><td>Calmar Ratio</td><td>{:.3}</td></tr>
                </table>
            </div>
"#, 
            metrics.volatility * 100.0,
            report.analysis.risk_metrics.value_at_risk_95 * 100.0,
            report.analysis.risk_metrics.sortino_ratio,
            report.analysis.risk_metrics.calmar_ratio
        ));

        // Trade analysis section
        html.push_str(&format!(r#"
            <div class="section">
                <h2>Trade Analysis</h2>
                <div class="metrics-grid">
                    <div class="metric-card">
                        <div class="metric-value positive">{}</div>
                        <div class="metric-label">Profitable Trades</div>
                    </div>
                    <div class="metric-card">
                        <div class="metric-value negative">{}</div>
                        <div class="metric-label">Losing Trades</div>
                    </div>
                    <div class="metric-card">
                        <div class="metric-value">{:.2}</div>
                        <div class="metric-label">Best Trade</div>
                    </div>
                    <div class="metric-card">
                        <div class="metric-value">{:.2}</div>
                        <div class="metric-label">Worst Trade</div>
                    </div>
                </div>
            </div>
"#,
            report.analysis.trade_distribution.profitable_trades,
            report.analysis.trade_distribution.losing_trades,
            report.analysis.trade_distribution.best_trade,
            report.analysis.trade_distribution.worst_trade
        ));

        // Footer
        html.push_str(&format!(r#"
            <div class="section">
                <p><small>Report generated on {} | Backtest ID: {}</small></p>
            </div>
        </div>
    </div>
</body>
</html>
"#, report.metadata.generated_at.format("%Y-%m-%d %H:%M:%S UTC"), report.metadata.report_id));

        Ok(html)
    }

    /// Add a metric card to HTML content
    fn add_metric_card(&self, html: &mut String, label: &str, value: &str, positive: bool) {
        let class = if positive { "positive" } else { "" };
        html.push_str(&format!(r#"
                    <div class="metric-card">
                        <div class="metric-value {}">{}</div>
                        <div class="metric-label">{}</div>
                    </div>
"#, class, value, label));
    }

    /// Create CSV content for the report
    fn create_csv_content(&self, report: &BacktestReport) -> Result<String> {
        let mut csv = String::new();
        
        // Header
        csv.push_str("Metric,Value\n");
        
        // Basic info
        csv.push_str(&format!("Strategy,{}\n", report.metadata.strategy_name));
        csv.push_str(&format!("Symbol,{}\n", report.metadata.symbol));
        csv.push_str(&format!("Period,{} to {}\n", report.metadata.start_date, report.metadata.end_date));
        csv.push_str(&format!("Initial Capital,{}\n", report.metadata.initial_capital));
        
        // Performance metrics
        let metrics = &report.performance;
        csv.push_str(&format!("Total Return,{:.4}\n", metrics.net_profit));
        csv.push_str(&format!("Sharpe Ratio,{:.4}\n", metrics.sharpe_ratio));
        csv.push_str(&format!("Profit Factor,{:.4}\n", metrics.profit_factor));
        csv.push_str(&format!("Max Drawdown,{:.4}\n", metrics.max_drawdown));
        csv.push_str(&format!("Volatility,{:.4}\n", metrics.volatility));
        csv.push_str(&format!("Win Rate,{:.4}\n", metrics.win_rate));
        csv.push_str(&format!("Total Trades,{}\n", metrics.number_of_trades));
        csv.push_str(&format!("Average Trade Return,{:.4}\n", metrics.average_trade_return));
        csv.push_str(&format!("Average Trade Duration,{:.2}\n", metrics.average_trade_duration));
        
        // Risk metrics
        csv.push_str(&format!("VaR 95%,{:.4}\n", report.analysis.risk_metrics.value_at_risk_95));
        csv.push_str(&format!("Sortino Ratio,{:.4}\n", report.analysis.risk_metrics.sortino_ratio));
        csv.push_str(&format!("Calmar Ratio,{:.4}\n", report.analysis.risk_metrics.calmar_ratio));
        
        Ok(csv)
    }
}

/// Helper function to create a default report from performance metrics
pub fn create_basic_report(
    strategy_name: String,
    symbol: String,
    timeframe: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
    initial_capital: f64,
    performance: PerformanceMetrics,
    equity_curve: Vec<EquityPoint>,
) -> BacktestReport {
    let report_id = format!("{}_{}_{}", 
        strategy_name.replace(" ", "_").to_lowercase(),
        symbol,
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );

    BacktestReport {
        metadata: ReportMetadata {
            report_id,
            strategy_name,
            symbol,
            timeframe,
            start_date,
            end_date,
            initial_capital,
            generated_at: Utc::now(),
            backtest_duration_seconds: None,
            data_points: equity_curve.len(),
        },
        performance,
        analysis: BacktestAnalysis {
            monthly_returns: HashMap::new(),
            risk_metrics: RiskMetrics {
                value_at_risk_95: 0.0,
                conditional_var_95: 0.0,
                sortino_ratio: 0.0,
                calmar_ratio: 0.0,
                max_consecutive_losses: 0,
                max_consecutive_wins: 0,
                tail_ratio: 0.0,
            },
            trade_distribution: TradeDistribution {
                profitable_trades: 0,
                losing_trades: 0,
                break_even_trades: 0,
                best_trade: 0.0,
                worst_trade: 0.0,
                avg_winning_trade: 0.0,
                avg_losing_trade: 0.0,
                largest_winning_streak: 0,
                largest_losing_streak: 0,
            },
            attribution: PerformanceAttribution {
                long_trades_pnl: 0.0,
                short_trades_pnl: 0.0,
                commission_paid: 0.0,
                slippage_cost: 0.0,
                market_impact_cost: 0.0,
            },
            benchmark_comparison: None,
            data_quality: DataQualityMetrics {
                overall_score: 10.0,
                data_completeness_score: 10.0,
                gaps_detected: 0,
                max_gap_seconds: 0,
                gap_ratio: 0.0,
            },
            market_impact: MarketImpactMetrics {
                trades_with_impact: 0,
                avg_impact_bps: 0.0,
                max_impact_bps: 0.0,
                liquidity_warnings: 0,
            },
            execution_quality: ExecutionQualityMetrics {
                avg_slippage_bps: 0.0,
                total_slippage_cost: 0.0,
                fill_rate: 100.0,
                avg_time_to_fill_seconds: 0.1,
            },
        },
        trades: None,
        equity_curve,
        drawdown_periods: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_report() -> BacktestReport {
        let performance = PerformanceMetrics {
            sharpe_ratio: 1.5,
            profit_factor: 1.8,
            net_profit: 0.25,
            gross_profit: 1000.0,
            gross_loss: -400.0,
            average_trade_return: 0.02,
            median_trade_return: 0.015,
            number_of_trades: 100,
            max_drawdown: 0.08,
            volatility: 0.15,
            win_rate: 0.6,
            average_trade_duration: 300.0,
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            transaction_costs_percentage: 0.0,
        };

        create_basic_report(
            "Test Strategy".to_string(),
            "BTCUSD".to_string(),
            "1h".to_string(),
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
            10000.0,
            performance,
            vec![]
        )
    }

    #[test]
    fn test_report_creation() {
        let report = create_test_report();
        assert_eq!(report.metadata.strategy_name, "Test Strategy");
        assert_eq!(report.metadata.symbol, "BTCUSD");
        assert_eq!(report.performance.sharpe_ratio, 1.5);
    }

    #[test]
    fn test_json_generation() {
        let temp_dir = TempDir::new().unwrap();
        let config = ReportConfig {
            output_dir: temp_dir.path().to_path_buf(),
            formats: vec![ReportFormat::Json],
            include_trades: false,
            include_charts: false,
            template_dir: None,
            custom_css: None,
            persist_to_database: false,
            database_url: None,
            generated_by: Some("test_user".to_string()),
            generation_source: "test".to_string(),
        };

        let generator = ReportGenerator::new(config);
        let report = create_test_report();
        
        let files = generator.generate_report(&report).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].exists());
        assert!(files[0].extension().unwrap() == "json");
    }

    #[test]
    fn test_csv_generation() {
        let temp_dir = TempDir::new().unwrap();
        let config = ReportConfig {
            output_dir: temp_dir.path().to_path_buf(),
            formats: vec![ReportFormat::Csv],
            include_trades: false,
            include_charts: false,
            template_dir: None,
            custom_css: None,
            persist_to_database: false,
            database_url: None,
            generated_by: Some("test_user".to_string()),
            generation_source: "test".to_string(),
        };

        let generator = ReportGenerator::new(config);
        let report = create_test_report();
        
        let files = generator.generate_report(&report).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].exists());

        let content = fs::read_to_string(&files[0]).unwrap();
        assert!(content.contains("Metric,Value"));
        assert!(content.contains("Sharpe Ratio,1.5"));
    }

    #[test]
    fn test_html_generation() {
        let temp_dir = TempDir::new().unwrap();
        let config = ReportConfig {
            output_dir: temp_dir.path().to_path_buf(),
            formats: vec![ReportFormat::Html],
            include_trades: false,
            include_charts: false,
            template_dir: None,
            custom_css: None,
            persist_to_database: false,
            database_url: None,
            generated_by: Some("test_user".to_string()),
            generation_source: "test".to_string(),
        };

        let generator = ReportGenerator::new(config);
        let report = create_test_report();
        
        let files = generator.generate_report(&report).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].exists());

        let content = fs::read_to_string(&files[0]).unwrap();
        assert!(content.contains("<!DOCTYPE html>"));
        assert!(content.contains("Test Strategy"));
        assert!(content.contains("Sharpe Ratio"));
    }
}
