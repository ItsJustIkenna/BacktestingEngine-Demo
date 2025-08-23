//! Backtest library entry point
pub mod simulation;
pub mod types;
pub mod monte_carlo;

// Main simulation orchestrator
// Coordinates data loading, strategy execution, order matching, portfolio/risk updates, and reporting.
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use crate::simulation::SimulationLoop;
use crate::types::*;

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
	pub async fn run(&mut self, market_data: Vec<MarketData>) -> BacktestResult {
		let default_config = config::BacktestConfig::default();
		self.simulation.run(
			&mut self.strategy_manager,
			&mut self.orderbook,
			&mut self.portfolio,
			&mut self.risk_manager,
			market_data,
			&default_config,
		).await
	}
}
