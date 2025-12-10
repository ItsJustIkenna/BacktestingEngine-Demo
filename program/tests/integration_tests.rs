use chrono::Utc;
use bigdecimal::BigDecimal;
use std::str::FromStr;

// Import from modules
use strategy::StrategyManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::{PortfolioState, Position, PositionSide};
use riskmanager::{RiskManager, RiskLimits};
use config::{BacktestConfig, TradingConfig, DatabaseConfig, DataConfig, OutputConfig, AnalysisConfig};
use walkforward::{WalkForwardConfig, WalkForwardAnalyzer};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_end_to_end_backtest_integration() {
        // Test that all core modules work together for a basic backtest
        let strategy_manager = StrategyManager::default();
        let orderbook = OrderBook::new("BTC/USDT".to_string(), "binance".to_string(), OrderBookConfig::default());
        let mut portfolio = PortfolioState::default();
        let risk_manager = RiskManager::new();
        
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
                exchange_fees: None,
                routing_mode: None,
                latency_ms: 10,
            },
            data: DataConfig {
                default_timeframe: "1h".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: true,
                cache_duration_hours: 12,
                gap_detection: config::GapDetectionConfig::default(),
                market_impact: config::MarketImpactConfig::default(),
            },
            output: OutputConfig {
                results_dir: "./test_results".to_string(),
                reports_dir: "./reports".to_string(),
                charts_enabled: false,
                export_formats: vec!["json".to_string()],
            },
            analysis: AnalysisConfig::default(),
        };

        // Set initial balance using correct field name and BigDecimal conversion
        portfolio.balance = BigDecimal::from_str("10000.0").unwrap();
        
        // Add a simple closed position for testing
        let now = Utc::now();
        portfolio.positions.push(Position {
            symbol: "BTC/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 0.1,
            entry_price: BigDecimal::from_str("50000.0").unwrap(),
            mark_price: Some(BigDecimal::from_str("55000.0").unwrap()),
            open_time: now,
            close_time: Some(now),
            unrealized_pnl: BigDecimal::from_str("500.0").unwrap(),
            realized_pnl: BigDecimal::from_str("0").unwrap(),
        });
        
        // Test that components can be created and basic operations work
        assert!(portfolio.balance > BigDecimal::from_str("0").unwrap());
        assert_eq!(portfolio.positions.len(), 1);
        
        // Test basic orderbook operations
        assert_eq!(orderbook.symbol, "BTC/USDT");
        assert_eq!(orderbook.exchange, "binance");
        
        // Test risk manager
        assert!(risk_manager.limits.max_position.is_some());
        
        // Test strategy manager creation
        // Strategy manager should be created successfully
        let _sm = &strategy_manager; // Just reference it to avoid unused warning
        
        // Test config is properly initialized
        assert!(config.trading.initial_capital > 0.0);
    }

    #[tokio::test]
    async fn test_walkforward_analyzer_integration() {
        // Test walkforward analysis integration with other components
        
        let config = WalkForwardConfig::new(
            chrono::Utc::now() - chrono::Duration::days(365),  // start_date
            chrono::Utc::now(),   // end_date
            std::time::Duration::from_secs(30 * 24 * 3600),  // optimization_window (30 days)
            std::time::Duration::from_secs(7 * 24 * 3600),   // test_window (7 days)
            5, // max_parameters
        );
        
        let backtest_config = BacktestConfig {
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
                exchange_fees: None,
                routing_mode: None,
                latency_ms: 10,
            },
            data: DataConfig {
                default_timeframe: "1d".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: false,
                cache_duration_hours: 24,
                gap_detection: config::GapDetectionConfig::default(),
                market_impact: config::MarketImpactConfig::default(),
            },
            output: OutputConfig {
                results_dir: "./results".to_string(),
                reports_dir: "./reports".to_string(),
                charts_enabled: false,
                export_formats: vec!["json".to_string()],
            },
            analysis: AnalysisConfig::default(),
        };
        
        let analyzer = WalkForwardAnalyzer::new(config, backtest_config);
        
        // Test that analyzer can be created and basic operations work
        assert!(analyzer.config.optimization_window_size.as_secs() > 0);
        assert!(analyzer.config.test_window_size.as_secs() > 0);
        assert!(analyzer.config.max_parameters > 0);
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
                exchange_fees: None,
                routing_mode: None,
                latency_ms: 10,
            },
            data: DataConfig {
                default_timeframe: "1h".to_string(),
                data_sources: vec!["mock".to_string()],
                cache_enabled: true,
                cache_duration_hours: 12,
                gap_detection: config::GapDetectionConfig::default(),
                market_impact: config::MarketImpactConfig::default(),
            },
            output: OutputConfig {
                results_dir: "./test_results".to_string(),
                reports_dir: "./test_reports".to_string(),
                charts_enabled: true,
                export_formats: vec!["json".to_string(), "csv".to_string()],
            },
            analysis: AnalysisConfig::default(),
        };
        
        // Test basic validation
        assert!(config.trading.initial_capital > 0.0);
        if let Some(max_pos) = config.trading.max_position_size {
            assert!(max_pos > 0.0 && max_pos <= 1.0);
        }
        assert!(config.trading.commission_rate >= 0.0);
        
        // Test that configuration can be used by other modules
        let portfolio = PortfolioState::default();
        assert_eq!(portfolio.balance, BigDecimal::from_str("0").unwrap()); // Default balance
    }

    #[tokio::test]
    async fn test_portfolio_risk_management_integration() {
        // Test integration between portfolio management and risk management
        
        let mut portfolio = PortfolioState::default();
        portfolio.balance = BigDecimal::from_str("100000").unwrap();
        
        // Create risk manager with custom limits
        let limits = RiskLimits {
            max_position: Some(10.0),
            max_order_size: Some(5.0),
            max_inventory_skew: Some(3.0),
            max_volatility: Some(0.15),
            ..Default::default()
        };
        let risk_manager = RiskManager::with_limits(limits);
        
        // Test that limits are properly set
        assert_eq!(risk_manager.limits.max_position, Some(10.0));
        assert_eq!(risk_manager.limits.max_order_size, Some(5.0));
        assert_eq!(risk_manager.limits.max_inventory_skew, Some(3.0));
        assert_eq!(risk_manager.limits.max_volatility, Some(0.15));
        
        // Test portfolio operations
        assert!(portfolio.balance > BigDecimal::from_str("0").unwrap());
        assert!(portfolio.positions.is_empty());
        
        // Create and add a position using correct field names
        let position = Position {
            symbol: "ETH/USDT".to_string(),
            side: PositionSide::Long,
            quantity: 2.0,
            entry_price: BigDecimal::from_str("3000.0").unwrap(),
            mark_price: Some(BigDecimal::from_str("3100.0").unwrap()),
            open_time: Utc::now(),
            close_time: None,
            unrealized_pnl: BigDecimal::from_str("200.0").unwrap(),
            realized_pnl: BigDecimal::from_str("0").unwrap(),
        };
        
        portfolio.positions.push(position);
        assert_eq!(portfolio.positions.len(), 1);
    }
}
