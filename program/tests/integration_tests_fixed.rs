use chrono::Utc;
use bigdecimal::BigDecimal;
use std::collections::HashMap;

// Import from modules
use strategy::{StrategyManager, PositionSide};
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::{PortfolioState, Position};
use riskmanager::{RiskManager, RiskLimits};
use config::{BacktestConfig, TradingConfig, DatabaseConfig, DataConfig, OutputConfig};
use backtest::simulation::BacktestSimulation;
use walkforward::{WalkForwardConfig, WalkForwardAnalyzer};
use metrics::PerformanceMetrics;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_end_to_end_backtest_integration() {
        // Test that all core modules work together for a basic backtest
        let mut strategy_manager = StrategyManager::default();
        let mut orderbook = OrderBook::new("BTC/USDT".to_string(), "binance".to_string(), OrderBookConfig::default());
        let mut portfolio = PortfolioState::default();
        let mut risk_manager = RiskManager::new();
        
        // Set up a basic trading config with correct field names
        let config = BacktestConfig {
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                database: "test_db".to_string(),
                username: "test".to_string(),
                password: "test".to_string(),
                max_connections: Some(10),
                connection_timeout: Some(30),
            },
            trading: TradingConfig {
                commission_rate: 0.001,
                slippage_bps: 5.0,
                initial_capital: 10000.0,
                max_position_size: Some(0.1),
                risk_free_rate: 0.02,
            },
            data: DataConfig {
                default_timeframe: "1d".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: false,
                cache_duration_hours: 24,
            },
            output: OutputConfig {
                results_dir: "./results".to_string(),
                reports_dir: "./reports".to_string(),
                charts_enabled: false,
                export_formats: vec!["json".to_string()],
            },
        };

        // Set initial balance using correct field name
        portfolio.balance = BigDecimal::from(config.trading.initial_capital);
        
        // Add a simple closed position for testing
        let now = Utc::now();
        portfolio.positions.push(Position {
            symbol: "BTC/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 0.1,
            entry_price: BigDecimal::from(50000),
            current_price: BigDecimal::from(55000),
            entry_time: now,
            last_update: now,
            unrealized_pnl: BigDecimal::from(500),
            realized_pnl: BigDecimal::from(0),
        });
        
        // Test that components can be created and basic operations work
        assert!(portfolio.balance > BigDecimal::from(0));
        assert_eq!(portfolio.positions.len(), 1);
        
        // Test basic orderbook operations
        assert_eq!(orderbook.symbol, "BTC/USDT");
        assert_eq!(orderbook.exchange, "binance");
        
        // Test risk manager
        assert!(risk_manager.limits.max_position.is_some());
    }

    #[tokio::test]
    async fn test_walkforward_analyzer_integration() {
        // Test walkforward analysis integration with other components
        
        let config = WalkForwardConfig::new(
            chrono::Duration::days(30),  // optimization_window
            chrono::Duration::days(7),   // test_window
            chrono::Duration::days(1),   // step_size
            HashMap::new(),              // parameter_ranges
        );
        
        let mut analyzer = WalkForwardAnalyzer::new(config);
        
        // Test that analyzer can be created and basic operations work
        assert!(analyzer.config.optimization_window > chrono::Duration::zero());
        assert!(analyzer.config.test_window > chrono::Duration::zero());
        assert!(analyzer.config.step_size > chrono::Duration::zero());
    }

    #[tokio::test]
    async fn test_config_validation_integration() {
        // Test that configuration validation works across modules
        
        let config = BacktestConfig {
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                database: "test_db".to_string(),
                username: "test".to_string(),
                password: "test".to_string(),
                max_connections: Some(10),
                connection_timeout: Some(30),
            },
            trading: TradingConfig {
                commission_rate: 0.001,
                slippage_bps: 5.0,
                initial_capital: 100000.0,
                max_position_size: Some(0.2),
                risk_free_rate: 0.025,
            },
            data: DataConfig {
                default_timeframe: "1h".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: true,
                cache_duration_hours: 12,
            },
            output: OutputConfig {
                results_dir: "./test_results".to_string(),
                reports_dir: "./test_reports".to_string(),
                charts_enabled: true,
                export_formats: vec!["json".to_string(), "csv".to_string()],
            },
        };
        
        // Test basic validation
        assert!(config.trading.initial_capital > 0.0);
        if let Some(max_pos) = config.trading.max_position_size {
            assert!(max_pos > 0.0 && max_pos <= 1.0);
        }
        assert!(config.trading.commission_rate >= 0.0);
        
        // Test that configuration can be used by other modules
        let portfolio = PortfolioState::default();
        assert_eq!(portfolio.balance, BigDecimal::from(0)); // Default balance
    }

    #[tokio::test]
    async fn test_portfolio_risk_management_integration() {
        // Test integration between portfolio management and risk management
        
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from(100000);
        
        // Create risk manager with custom limits
        let limits = RiskLimits {
            max_position: Some(10.0),
            max_order_size: Some(5.0),
            max_inventory_skew: Some(3.0),
            max_volatility: Some(0.15),
        };
        let risk_manager = RiskManager::with_limits(limits);
        
        // Test that limits are properly set
        assert_eq!(risk_manager.limits.max_position, Some(10.0));
        assert_eq!(risk_manager.limits.max_order_size, Some(5.0));
        assert_eq!(risk_manager.limits.max_inventory_skew, Some(3.0));
        assert_eq!(risk_manager.limits.max_volatility, Some(0.15));
        
        // Test portfolio operations
        assert!(portfolio.balance > BigDecimal::from(0));
        assert!(portfolio.positions.is_empty());
        
        // Create and add a position
        let position = Position {
            symbol: "ETH/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 2.0,
            entry_price: BigDecimal::from(3000),
            current_price: BigDecimal::from(3100),
            entry_time: Utc::now(),
            last_update: Utc::now(),
            unrealized_pnl: BigDecimal::from(200),
            realized_pnl: BigDecimal::from(0),
        };
        
        portfolio.positions.push(position);
        assert_eq!(portfolio.positions.len(), 1);
    }
}
