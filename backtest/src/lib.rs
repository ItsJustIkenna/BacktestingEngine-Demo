//! Backtest library entry point
pub mod simulation;
pub mod types;
pub mod monte_carlo;
pub mod logging_facade;
pub mod html_export;

// Re-export main types
pub use types::{BacktestResult, BacktestEvent};
pub use html_export::HtmlExporter;

// Main simulation orchestrator
// Coordinates data loading, strategy execution, order matching, portfolio/risk updates, and reporting.
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use crate::simulation::SimulationLoop;
use crate::logging_facade::BACKTEST_LOGGER;

use metrics::{PerformanceMetrics, ReportGenerator, ReportConfig, ReportFormat, create_basic_report, EquityPoint};
use config::OutputConfig;
use chrono::NaiveDate;
use anyhow::Result;
use std::path::PathBuf;
use bigdecimal::ToPrimitive;

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
            _ => log_warn!(BACKTEST_LOGGER, "Unknown export format '{}' ignored", format_str),
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
        persist_to_database: false, // Can be enabled via config later
        database_url: None, // Would come from environment or config
        generated_by: None, // Would be set by calling code
        generation_source: "backtest_engine".to_string(),
    })
}

pub struct BacktestEngine {
	pub strategy_manager: StrategyManager,
	pub orderbook: OrderBook,
	pub portfolio: PortfolioState,
	pub risk_manager: RiskManager,
	pub simulation: SimulationLoop,
}

impl BacktestEngine {
	pub fn new(
		strategy_manager: StrategyManager,
		orderbook: OrderBook,
		portfolio: PortfolioState,
		risk_manager: RiskManager,
		simulation: SimulationLoop,
	) -> Self {
		Self {
			strategy_manager,
			orderbook,
			portfolio,
			risk_manager,
			simulation,
		}
	}

	/// Run the backtest simulation
	pub async fn run(&mut self, market_data: Vec<MarketData>) -> types::BacktestResult {
		self.run_with_config(market_data, &config::BacktestConfig::default()).await
	}

	/// Run the backtest simulation with custom configuration
	pub async fn run_with_config(&mut self, market_data: Vec<MarketData>, config: &config::BacktestConfig) -> types::BacktestResult {
		self.simulation.run(
			&mut self.strategy_manager,
			&mut self.orderbook,
			&mut self.portfolio,
			&mut self.risk_manager,
			market_data,
			config,
		).await
	}

	/// Generate comprehensive report from backtest results
	pub fn generate_report(
		&self, 
		result: &types::BacktestResult,
		strategy_name: &str,
		symbol: &str,
		timeframe: &str,
		start_date: NaiveDate,
		end_date: NaiveDate,
		initial_capital: f64,
		output_config: &OutputConfig,
	) -> Result<Vec<PathBuf>> {
		// Convert BacktestResult to PerformanceMetrics
		let performance_metrics = PerformanceMetrics {
			sharpe_ratio: result.sharpe_ratio.unwrap_or(0.0),
			profit_factor: result.profit_factor.unwrap_or(0.0),
			net_profit: result.net_profit.unwrap_or(0.0),
			gross_profit: result.gross_profit.unwrap_or(0.0),
			gross_loss: result.gross_loss.unwrap_or(0.0),
			average_trade_return: result.avg_trade_return.unwrap_or(0.0),
			median_trade_return: result.median_trade_return.unwrap_or(0.0),
			number_of_trades: result.num_trades,
			max_drawdown: result.max_drawdown.to_f64().unwrap_or(0.0),
			volatility: result.volatility.unwrap_or(0.0),
			win_rate: result.win_rate.unwrap_or(0.0),
			average_trade_duration: result.avg_trade_duration.unwrap_or(0.0),
		};

		// Create basic equity curve (in real implementation, this would come from portfolio tracking)
		let equity_curve = vec![
			EquityPoint {
				timestamp: start_date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
				equity: initial_capital,
				drawdown: 0.0,
				drawdown_percent: 0.0,
			},
			EquityPoint {
				timestamp: end_date.and_hms_opt(23, 59, 59).unwrap().and_utc(),
				equity: initial_capital + result.total_pnl.to_f64().unwrap_or(0.0),
				drawdown: result.max_drawdown.to_f64().unwrap_or(0.0),
				drawdown_percent: result.max_drawdown.to_f64().unwrap_or(0.0) / initial_capital,
			},
		];

		// Create report configuration from output configuration
		let report_config = crate::create_report_config(output_config)?;
		let generator = ReportGenerator::new(report_config);

		// Create comprehensive report
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

		// Generate report files
		generator.generate_report(&report)
	}
}
