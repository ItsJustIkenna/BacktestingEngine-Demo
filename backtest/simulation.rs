//! SimulationLoop: Drives the backtest event loop
//!
//! Steps through market data, triggers strategies, matches orders, and updates state.

use crate::strategy::{StrategyManager, StrategyContext, Signal};
use crate::orderbook::OrderBook;
use crate::portfoliomanager::PortfolioState;
use crate::riskmanager::RiskManager;
use crate::dataloader::MarketData;
use crate::backtest::types::{BacktestEvent, BacktestResult};
use chrono::{DateTime, Utc};

pub struct SimulationLoop;

impl SimulationLoop {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(
        &mut self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        risk_manager: &mut RiskManager,
        market_data: Vec<MarketData>,
    ) -> BacktestResult {
        let mut events: Vec<BacktestEvent> = Vec::new();
        let mut result = BacktestResult::default();

        for data in market_data {
            // 1. Update orderbook with new market data if needed
            match &data {
                MarketData::Snapshot(snapshot) => {
                    // Convert snapshot to OrderBookEvent::Snapshot and apply
                    // (Assume conversion function exists)
                    // orderbook.apply_event(OrderBookEvent::Snapshot { ... })
                }
                MarketData::Trade(trade) => {
                    // Convert trade to OrderBookEvent::Trade and apply
                    // orderbook.apply_event(OrderBookEvent::Trade { ... })
                }
                _ => {}
            }

            // 2. Build strategy context (timestamp, portfolio, etc.)
            let timestamp = match &data {
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Order(o) => o.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            let context = StrategyContext {
                timestamp,
                portfolio_state: portfolio.clone(),
                // ...add more context fields as needed
            };

            // 3. Run all enabled strategies, collect signals
            let signals = strategy_manager.process_market_data(&data, &context).await.unwrap_or_default();
            for (strategy_id, sigs) in signals {
                for signal in sigs {
                    // 4. Submit order to orderbook
                    let _ = orderbook.submit_order_from_signal(&signal, timestamp);

                    // 5. Simulate order matching/fills
                    let fills = orderbook.match_all_orders(timestamp);

                    // 6. Update portfolio and risk manager with fills
                    // portfolio.apply_fills(&fills); // Implement this method in PortfolioState
                    // risk_manager.update(&portfolio, &fills); // Implement this method in RiskManager

                    // 7. Record event
                    events.push(BacktestEvent::OrderSubmitted {
                        order_id: signal.id().to_string(),
                        timestamp,
                    });
                    // TODO: Record fills, PnL, etc.
                }
            }
            // TODO: Collect metrics, update result
        }

        // TODO: Finalize result (PnL, drawdown, etc.)
        result
    }
}
