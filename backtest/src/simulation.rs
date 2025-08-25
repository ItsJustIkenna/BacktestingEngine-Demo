//! SimulationLoop: Drives the backtest event loop
//!
//! Steps through market data, triggers strategies, matches orders, and updates state.

use strategy::{StrategyManager, StrategyContext};
use strategy::traits::{MarketSession, SessionType};
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use crate::types::{BacktestEvent, BacktestResult};
use metrics::{performance, risk, trade_stats};
use bigdecimal::{BigDecimal, ToPrimitive, FromPrimitive};

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
        config: &config::BacktestConfig,
    ) -> BacktestResult {
    let mut events: Vec<BacktestEvent> = Vec::new();
    let mut result = BacktestResult::default();

    // Collectors for metrics
    let mut trade_returns: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = Vec::new();
    let mut trade_durations: Vec<f64> = Vec::new();

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
                price_history: Vec::new(), // Empty for simplicity, could be filled from historical data
                volume_history: Vec::new(), // Empty for simplicity 
                spread: None,
                market_session: MarketSession {
                    is_open: true,
                    session_type: SessionType::Regular,
                    next_open: None,
                    next_close: None,
                }, // Default session
            };

            // 3. Run all enabled strategies, collect signals
            let signals = strategy_manager.process_market_data(&data, &context).await.unwrap_or_default();
            for (_strategy_id, sigs) in signals {
                for signal in sigs {
                    // 4. Submit order to orderbook
                    let _ = orderbook.submit_order_from_signal(&signal, timestamp);

                    // 5. Simulate order matching/fills with proper fee calculation
                    let fills = orderbook.match_all_orders(timestamp);

                    // 6. Update portfolio and risk manager with fills
                    if let Err(e) = portfolio.apply_fills(&fills) {
                        eprintln!("Error applying fills: {}", e);
                    }
                    // risk_manager.update(&portfolio, &fills); // TODO: Implement this method in RiskManager

                    // 7. Record event
                    events.push(BacktestEvent::OrderSubmitted {
                        order_id: signal.id.clone(),
                        timestamp,
                    });
                }
            }

            // Collect equity value at each step
            equity_curve.push(portfolio.balance.to_f64().unwrap_or(0.0));
        }

        // Collect trade returns and durations from closed positions (do this after market data processing)
        for pos in &portfolio.positions {
            if let Some(close_time) = pos.close_time {
                // Only consider closed positions
                trade_returns.push(pos.realized_pnl.to_f64().unwrap_or(0.0));
                let duration = (close_time - pos.open_time).num_seconds() as f64;
                trade_durations.push(duration);
            }
        }

        // Compute metrics using collected data
        result.num_trades = performance::number_of_trades(&trade_returns);
        result.gross_profit = Some(performance::gross_profit(&trade_returns));
        result.gross_loss = Some(performance::gross_loss(&trade_returns));
        result.net_profit = Some(performance::net_profit(&trade_returns));
        result.avg_trade_return = performance::average_trade_return(&trade_returns);
        result.median_trade_return = performance::median_trade_return(&trade_returns);
        result.sharpe_ratio = performance::sharpe_ratio(&trade_returns, config.trading.risk_free_rate);
        result.profit_factor = performance::profit_factor(
            result.gross_profit.unwrap_or(0.0),
            result.gross_loss.unwrap_or(0.0),
        );
        result.win_rate = trade_stats::win_rate(&trade_returns);
        result.avg_trade_duration = trade_stats::average_trade_duration(&trade_durations);
        result.volatility = risk::volatility(&trade_returns);
        result.max_drawdown = risk::max_drawdown(&equity_curve)
            .map(|v| BigDecimal::from_f64(v).unwrap_or_default())
            .unwrap_or_default();
        // result.total_pnl: set as needed from portfolio or trade_returns

        result
    }
}
