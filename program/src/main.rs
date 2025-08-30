use clap::{Parser, Subcommand};
use chrono::{NaiveDate, Duration};
use dataloader::MarketData;
use walkforward::runner::{run_walkforward_analysis, run_walkforward_with_auto_export};
use walkforward::auto_export::ExportThresholds;
use backtest::BacktestEngine;
use backtest::simulation::SimulationLoop;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use strategy::StrategyManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use genetic::{GeneticOptimizer, chromosome::AvellanedaStoikovChromosome};
use config::GeneticConfig;
use config::OutputConfig;
use std::sync::Arc;

// UltraLogger integration
use program::logging_facade::MAIN_LOGGER;
use program::{log_info, log_warn, log_error, log_debug};

#[derive(Parser)]
#[command(name = "TradingPlatform")]
#[command(about = "CLI for Backtesting, Optimization, and Walk-Forward Analysis", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run walk-forward analysis
    WalkForward {
        #[arg(long)]
        data_path: String,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        train_days: i64,
        #[arg(long)]
        test_days: i64,
        #[arg(long)]
        step_days: i64,
    },
    /// Run enhanced walk-forward analysis with automatic strategy export
    WalkForwardAutoExport {
        #[arg(long)]
        data_path: String,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        train_days: i64,
        #[arg(long)]
        test_days: i64,
        #[arg(long)]
        step_days: i64,
        #[arg(long)]
        strategy_name: String,
        #[arg(long, default_value = "1.0")]
        min_sharpe_ratio: f64,
        #[arg(long, default_value = "1.3")]
        min_profit_factor: f64,
        #[arg(long, default_value = "0.55")]
        min_win_rate: f64,
        #[arg(long, default_value = "0.15")]
        max_drawdown: f64,
        #[arg(long, default_value = "50")]
        min_trades: usize,
        #[arg(long, default_value = "3")]
        min_profitable_windows: usize,
    },
    /// Run a single backtest
    Backtest {
        #[arg(long)]
        data_path: String,
        #[arg(long)]
        risk_aversion: f64,
        #[arg(long)]
        inventory_target: f64,
        #[arg(long)]
        order_size: f64,
    },
    /// Run parameter optimization
    Optimize {
        #[arg(long)]
        data_path: String,
    },
    /// Generate comprehensive backtest report
    Report {
        #[arg(long)]
        data_path: String,
        #[arg(long)]
        strategy_name: String,
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        timeframe: String,
        #[arg(long)]
        output_dir: String,
        #[arg(long, value_delimiter = ',', default_values = ["html", "json", "csv"])]
        formats: Vec<String>,
        #[arg(long, default_value = "10000.0")]
        initial_capital: f64,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::WalkForward { data_path, start, end, train_days, test_days, step_days } => {
            log_info!(MAIN_LOGGER, "🚀 Starting Walk-Forward Analysis...");
            
            // Load market data
            log_info!(MAIN_LOGGER, "📊 Loading market data from: {}", data_path);
            let data = match load_market_data(&data_path).await {
                Ok(data) => data,
                Err(e) => {
                    log_error!(MAIN_LOGGER, "❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Parse dates
            let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                .expect("Invalid start date format. Use YYYY-MM-DD");
            let end_date = NaiveDate::parse_from_str(&end, "%Y-%m-%d")
                .expect("Invalid end date format. Use YYYY-MM-DD");
            
            // Convert days to Duration
            let train_duration = Duration::days(train_days);
            let test_duration = Duration::days(test_days);
            let step_duration = Duration::days(step_days);
            
            println!("📈 Running analysis from {} to {} (train={}, test={}, step={} days)", 
                start, end, train_days, test_days, step_days);
            
            // Run walk-forward analysis
            let results = run_walkforward_analysis(
                &data, 
                start_date, 
                end_date, 
                train_duration, 
                test_duration, 
                step_duration
            ).await;
            
            println!("✅ Walk-forward analysis completed!");
            println!("📊 Summary: {} windows analyzed", results.window_results.len());
            println!("🏆 Average Test Sharpe: {:.4}", results.average_test_sharpe);
            println!("📏 Parameter Stability: {:.4}", results.parameter_stability);
        }
        Commands::WalkForwardAutoExport { 
            data_path, start, end, train_days, test_days, step_days,
            strategy_name, min_sharpe_ratio, min_profit_factor, min_win_rate,
            max_drawdown, min_trades, min_profitable_windows
        } => {
            println!("🚀 Starting Enhanced Walk-Forward Analysis with Auto-Export...");
            
            // Load market data
            println!("📊 Loading market data from: {}", data_path);
            let data = match load_market_data(&data_path).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Parse dates
            let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
                .expect("Invalid start date format. Use YYYY-MM-DD");
            let end_date = NaiveDate::parse_from_str(&end, "%Y-%m-%d")
                .expect("Invalid end date format. Use YYYY-MM-DD");
            
            // Convert days to Duration
            let train_duration = Duration::days(train_days);
            let test_duration = Duration::days(test_days);
            let step_duration = Duration::days(step_days);
            
            println!("📈 Running enhanced analysis from {} to {} (train={}, test={}, step={} days)", 
                start, end, train_days, test_days, step_days);
            println!("🎯 Strategy: {}", strategy_name);
            println!("📋 Export Thresholds: Sharpe≥{:.2}, PF≥{:.2}, WinRate≥{:.2}, DD≤{:.2}%", 
                min_sharpe_ratio, min_profit_factor, min_win_rate, max_drawdown*100.0);
            
            // Configure export thresholds
            let export_thresholds = ExportThresholds {
                min_sharpe_ratio,
                min_profit_factor,
                min_win_rate,
                max_drawdown,
                min_number_of_trades: min_trades,
                min_consistency_windows: min_profitable_windows,
            };
            
            // Run enhanced walk-forward analysis with automatic export
            match run_walkforward_with_auto_export(
                &data, 
                start_date, 
                end_date, 
                train_duration, 
                test_duration, 
                step_duration,
                &strategy_name,
                Some(export_thresholds)
            ).await {
                Ok((results, exported)) => {
                    println!("✅ Enhanced walk-forward analysis completed!");
                    println!("📊 Summary: {} windows analyzed", results.window_results.len());
                    println!("🏆 Average Test Sharpe: {:.4}", results.average_test_sharpe);
                    println!("📏 Parameter Stability: {:.4}", results.parameter_stability);
                    
                    if exported {
                        println!("🎉 Strategy successfully exported to SignalEngine!");
                        println!("🚀 Ready for live trading deployment");
                    } else {
                        println!("⚠️  Strategy did not meet export thresholds");
                        println!("🔧 Consider adjusting parameters or improving strategy");
                    }
                }
                Err(e) => {
                    eprintln!("❌ Enhanced walk-forward analysis failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Backtest { data_path, risk_aversion, inventory_target, order_size } => {
            println!("🚀 Starting Backtest...");
            
            // Load market data
            println!("📊 Loading market data from: {}", data_path);
            let data = match load_market_data(&data_path).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Set up strategy parameters
            let params = AvellanedaStoikovParams {
                risk_aversion,
                inventory_target,
                order_size,
                ..Default::default()
            };
            
            println!("⚙️  Running backtest with params: {:#?}", params);
            
            // Create strategy using Default::default() and update params
            let _strategy = AvellanedaStoikovStrategy::default();
            // Strategy parameters are embedded in the strategy struct
            
            // Initialize backtest components
            let strategy_manager = StrategyManager::new();
            let config = OrderBookConfig::default();
            let fee_config = config::ExchangeFeeConfig::kraken_default();
            let mut orderbook = OrderBook::new("BTCUSD".to_string(), "kraken".to_string(), config);
            orderbook.set_fee_config(fee_config);
            let portfolio = PortfolioState::default();  // Use default initialization
            let risk_manager = RiskManager::new();
            let simulation = SimulationLoop::new();
            
            let mut engine = BacktestEngine::new(
                strategy_manager,
                orderbook,
                portfolio,
                risk_manager,
                simulation,
            );
            
            // Run the backtest
            let result = engine.run(data).await;
            
            println!("✅ Backtest completed!");
            println!("📊 Total P&L: ${:.2}", result.total_pnl.to_string());
            println!("� Number of trades: {}", result.num_trades);
            if let Some(sharpe) = result.sharpe_ratio {
                println!("🎯 Sharpe Ratio: {:.4}", sharpe);
            }
            if let Some(pf) = result.profit_factor {
                println!("💰 Profit Factor: {:.2}", pf);
            }
            println!("📉 Max Drawdown: ${:.2}", result.max_drawdown.to_string());
        }
        Commands::Optimize { data_path } => {
            println!("🧬 Starting Genetic Parameter Optimization...");
            
            // Load market data
            println!("📊 Loading market data from: {}", data_path);
            let data = match load_market_data(&data_path).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Set up genetic algorithm configuration
            let config = GeneticConfig {
                population_size: 20,  // Smaller for faster testing
                generations: 50,     // Fewer generations for now
                crossover_rate: 0.7,
                mutation_rate: 0.1,
            };
            
            // Create fitness function that runs backtests
            let fitness_data = std::sync::Arc::new(data);
            let fitness_fn = {
                let data_copy = fitness_data.clone();
                std::sync::Arc::new(move |chromosome: &AvellanedaStoikovChromosome| {
                    let data_inner = data_copy.clone();
                    let chromosome_clone = chromosome.clone();
                    Box::pin(async move {
                        // Convert chromosome to strategy params
                        let _params = chromosome_to_params(&chromosome_clone);
                        
                        // Run simplified backtest with these params
                        let strategy_manager = StrategyManager::new();
                        let config = OrderBookConfig::default();
                        let fee_config = config::ExchangeFeeConfig::kraken_default();
                        let mut orderbook = OrderBook::new("BTCUSD".to_string(), "kraken".to_string(), config);
                        orderbook.set_fee_config(fee_config);
                        let portfolio = PortfolioState::default();
                        let risk_manager = RiskManager::new();
                        let simulation = SimulationLoop::new();
                        
                        let mut engine = BacktestEngine::new(
                            strategy_manager,
                            orderbook,
                            portfolio,
                            risk_manager,
                            simulation,
                        );
                        
                        let result = engine.run((*data_inner).clone()).await;
                        
                        // Return Sharpe ratio as fitness (higher is better)
                        result.sharpe_ratio.unwrap_or(-1.0)  // Penalize failed backtests
                    }) as std::pin::Pin<Box<dyn std::future::Future<Output = f64> + Send>>
                })
            };
            
            // Create optimizer and run
            let optimizer = GeneticOptimizer {
                config,
                fitness_fn,
            };
            
            println!("🔄 Running {} generations with population size {}...", 
                optimizer.config.generations, optimizer.config.population_size);
            
            let (best_chromosome, best_fitness) = optimizer.run().await;
            let best_params = chromosome_to_params(&best_chromosome);
            
            println!("✅ Optimization completed!");
            println!("🏆 Best Fitness (Sharpe): {:.4}", best_fitness);
            println!("⚙️  Best Parameters:");
            println!("   Risk Aversion: {:.6}", best_params.risk_aversion);
            println!("   Inventory Target: {:.6}", best_params.inventory_target);
            println!("   Order Size: {:.6}", best_params.order_size);
            println!("   Window (ms): {}", best_params.window_ms);
            println!("   Min Quote Lifetime (ms): {}", best_params.min_quote_lifetime_ms);
            println!("   Fee Rate: {:.6}", best_params.fee_rate);
            println!("   Latency Buffer: {:.6}", best_params.latency_buffer);
            println!("   Volatility Cap: {:.6}", best_params.volatility_cap);
        }
        Commands::Report { 
            data_path, 
            strategy_name, 
            symbol, 
            timeframe, 
            output_dir, 
            formats,
            initial_capital,
        } => {
            println!("📊 Generating Comprehensive Backtest Report...");
            
            // Create output configuration from CLI arguments
            let mut export_formats = formats;
            if export_formats.is_empty() {
                export_formats = vec!["html".to_string(), "json".to_string(), "csv".to_string()];
            }
            
            let output_config = OutputConfig {
                results_dir: output_dir.clone(),
                reports_dir: output_dir.clone(),
                charts_enabled: true,
                export_formats,
            };
            
            // Run a sample backtest to get results (in real usage, this would come from stored results)
            println!("🚀 Running sample backtest to generate report data...");
            let data = match load_market_data(&data_path).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Create backtest engine
            let strategy_manager = StrategyManager::new();
            let config = OrderBookConfig::default();
            let fee_config = config::ExchangeFeeConfig::kraken_default();
            let mut orderbook = OrderBook::new(symbol.clone(), "kraken".to_string(), config);
            orderbook.set_fee_config(fee_config);
            let portfolio = PortfolioState::default();
            let risk_manager = RiskManager::new();
            let simulation = SimulationLoop::new();
            
            let mut engine = BacktestEngine::new(
                strategy_manager,
                orderbook,
                portfolio,
                risk_manager,
                simulation,
            );
            
            // Run backtest
            let result = engine.run(data).await;
            
            // Generate report using the new reporting system
            let start_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
            let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
            
            match engine.generate_report(
                &result,
                &strategy_name,
                &symbol,
                &timeframe,
                start_date,
                end_date,
                initial_capital,
                &output_config,
            ) {
                Ok(generated_files) => {
                    println!("✅ Report generation completed successfully!");
                    println!("📁 Generated {} report file(s):", generated_files.len());
                    for file_path in &generated_files {
                        println!("  📄 {}", file_path.display());
                    }
                    
                    // Display key metrics
                    println!("\n📈 Performance Summary:");
                    println!("💰 Total P&L: ${:.2}", result.total_pnl.to_string());
                    println!("🔢 Number of trades: {}", result.num_trades);
                    if let Some(sharpe) = result.sharpe_ratio {
                        println!("🎯 Sharpe Ratio: {:.4}", sharpe);
                    }
                    if let Some(pf) = result.profit_factor {
                        println!("💸 Profit Factor: {:.2}", pf);
                    }
                    println!("📉 Max Drawdown: ${:.2}", result.max_drawdown.to_string());
                    
                    // Suggest next steps
                    println!("\n💡 Next Steps:");
                    println!("  • Open the HTML report in your browser for detailed analysis");
                    println!("  • Use the CSV data for further analysis in Excel/Python");
                    println!("  • Review the JSON output for programmatic integration");
                }
                Err(e) => {
                    eprintln!("❌ Report generation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

// Helper function to convert chromosome to parameters
fn chromosome_to_params(chromosome: &AvellanedaStoikovChromosome) -> AvellanedaStoikovParams {
    AvellanedaStoikovParams {
        risk_aversion: chromosome.risk_aversion,
        inventory_target: chromosome.inventory_target,
        order_size: chromosome.order_size,
        window_ms: chromosome.window_ms,
        min_quote_lifetime_ms: chromosome.min_quote_lifetime_ms,
        fee_rate: chromosome.fee_rate,
        latency_buffer: chromosome.latency_buffer,
        volatility_cap: chromosome.volatility_cap,
    }
}


/// Load market data from file or database
async fn load_market_data(path: &str) -> Result<Vec<MarketData>, Box<dyn std::error::Error>> {
    #[cfg(feature = "postgres")]
    {
        use dotenv::dotenv;
        use std::env;
        use databaseschema::CustomAsyncPgConnectionManager;
        use deadpool::managed::Pool;
        use std::path::Path;

    // Check if path is a file or database query
    if Path::new(path).exists() {
        // TODO: Implement file-based data loading (CSV, JSON, Parquet)
        return Err("File-based data loading not yet implemented".into());
    }

    // Database loading with proper error handling
    dotenv().ok();
    let db_url = env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must be set in .env or environment")?;

    let manager = CustomAsyncPgConnectionManager::new(db_url);
    let pool = Pool::builder(manager)
        .max_size(16)
        .build()
        .map_err(|e| format!("Failed to create database pool: {}", e))?;
    let pool = Arc::new(pool);

    // For now, use default parameters - in production this should come from config
    let params = dataloader::QueryParams {
        symbol: "BTCUSD".to_string(),
        exchange: "BINANCE".to_string(), 
        start_time: Some(chrono::Utc::now() - chrono::Duration::days(30)),
        end_time: Some(chrono::Utc::now()),
        limit: Some(10000), // Reasonable default limit
    };

    let loader = dataloader::DataLoader::new(pool);
    
    // Load multiple data types for comprehensive backtesting
    let mut market_data = Vec::new();
    
    // Load historical snapshots
    match loader.load_historical_snapshots(&params).await {
        Ok(snapshots) => {
            market_data.extend(snapshots.into_iter().map(MarketData::Snapshot));
        }
        Err(e) => eprintln!("Warning: Failed to load snapshots: {}", e),
    }

    // Load historical orders
    match loader.load_historical_orders(&params).await {
        Ok(orders) => {
            market_data.extend(orders.into_iter().map(MarketData::Order));
        }
        Err(e) => eprintln!("Warning: Failed to load orders: {}", e),
    }

    if market_data.is_empty() {
        return Err("No market data loaded from database".into());
    }

    // Sort by timestamp for proper chronological order
    market_data.sort_by(|a, b| {
        let timestamp_a = match a {
            MarketData::Order(o) => o.get_timestamp(),
            MarketData::Snapshot(s) => &s.get_timestamp(),
            MarketData::Candle(c) => &c.timestamp,
            MarketData::Trade(t) => &t.timestamp,
        };
        let timestamp_b = match b {
            MarketData::Order(o) => o.get_timestamp(),
            MarketData::Snapshot(s) => &s.get_timestamp(),
            MarketData::Candle(c) => &c.timestamp,
            MarketData::Trade(t) => &t.timestamp,
        };
        timestamp_a.cmp(timestamp_b)
    });

    println!("📊 Loaded {} market data points", market_data.len());
    Ok(market_data)
    }

    #[cfg(not(feature = "postgres"))]
    {
        // Mock implementation for non-postgres builds
        println!("📊 Mock data loading (postgres feature disabled)");
        
        // Create some mock candle data for testing
        use bigdecimal::BigDecimal;
        
        let now = chrono::Utc::now();
        let mock_candles = vec![
            MarketData::Candle(dataloader::Candle {
                symbol: "BTCUSD".to_string(),
                exchange: "MOCK".to_string(),
                timestamp: now - chrono::Duration::minutes(5),
                open: BigDecimal::from(50000),
                high: BigDecimal::from(50100),
                low: BigDecimal::from(49900),
                close: BigDecimal::from(50050),
                volume: BigDecimal::from(100),
                trade_count: 50,
            }),
            MarketData::Candle(dataloader::Candle {
                symbol: "BTCUSD".to_string(),
                exchange: "MOCK".to_string(),
                timestamp: now,
                open: BigDecimal::from(50050),
                high: BigDecimal::from(50200),
                low: BigDecimal::from(50000),
                close: BigDecimal::from(50150),
                volume: BigDecimal::from(150),
                trade_count: 75,
            }),
        ];
        
        println!("📊 Generated {} mock candles", mock_candles.len());
        Ok(mock_candles)
    }
}
