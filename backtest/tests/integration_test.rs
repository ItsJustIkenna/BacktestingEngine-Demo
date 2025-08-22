//! Integration test for the backtesting workflow and metrics

use backtest::simulation::SimulationLoop;
use backtest::types::BacktestResult;
use config::BacktestConfig;
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::{PortfolioState, Position, PositionSide};
use riskmanager::RiskManager;
use dataloader::MarketData;
use chrono::{Utc, Duration};
use bigdecimal::BigDecimal;

#[tokio::test]
async fn test_backtest_metrics_integration() {
    // Setup mock components
    let mut strategy_manager = StrategyManager::default();
    let mut orderbook = OrderBook::default();
    let mut portfolio = PortfolioState::default();
    let mut risk_manager = RiskManager::default();
    let config = BacktestConfig::default();
    let mut sim = SimulationLoop::new();

    // Simulate 3 closed BTC positions with varying PnL and durations
    let now = Utc::now();
    portfolio.positions = vec![
        Position {
            symbol: "BTC/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 0.5,
            entry_price: BigDecimal::from(30000),
            mark_price: Some(BigDecimal::from(31000)),
            realized_pnl: BigDecimal::from(500),
            unrealized_pnl: BigDecimal::from(0),
            open_time: now - Duration::seconds(3600),
            close_time: Some(now - Duration::seconds(1800)),
        },
        Position {
            symbol: "BTC/USDT".to_string(),
            side: PositionSide::Short,
            quantity: 0.3,
            entry_price: BigDecimal::from(32000),
            mark_price: Some(BigDecimal::from(31000)),
            realized_pnl: BigDecimal::from(-300),
            unrealized_pnl: BigDecimal::from(0),
            open_time: now - Duration::seconds(7200),
            close_time: Some(now - Duration::seconds(3600)),
        },
        Position {
            symbol: "BTC/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 0.2,
            entry_price: BigDecimal::from(31000),
            mark_price: Some(BigDecimal::from(31500)),
            realized_pnl: BigDecimal::from(100),
            unrealized_pnl: BigDecimal::from(0),
            open_time: now - Duration::seconds(5400),
            close_time: Some(now - Duration::seconds(100)),
        },
    ];
    portfolio.balance = BigDecimal::from(10000);

    // No market data needed for this test, as positions are mocked
    let market_data = vec![];

    let result: BacktestResult = sim
        .run(
            &mut strategy_manager,
            &mut orderbook,
            &mut portfolio,
            &mut risk_manager,
            market_data,
            &config,
        )
        .await;

    // Assert metrics
    assert_eq!(result.num_trades, 3);
    assert_eq!(result.gross_profit.unwrap(), 600.0);
    assert_eq!(result.gross_loss.unwrap(), -300.0);
    assert_eq!(result.net_profit.unwrap(), 300.0);
    assert!(result.avg_trade_return.unwrap() > 0.0);
    assert!(result.median_trade_return.unwrap() > 0.0);
    assert!(result.win_rate.unwrap() > 0.0 && result.win_rate.unwrap() < 1.0);
    assert!(result.avg_trade_duration.unwrap() > 0.0);
    assert!(result.volatility.unwrap() >= 0.0);
    assert!(result.max_drawdown >= BigDecimal::from(0));
}
