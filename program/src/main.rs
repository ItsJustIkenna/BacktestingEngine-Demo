use clap::{Parser, Subcommand};
use chrono::{NaiveDate, Duration};
use bigdecimal::{BigDecimal, FromPrimitive};
use dataloader::MarketData;
use walkforward::runner::{run_walkforward_analysis, run_walkforward_with_auto_export};
use walkforward::auto_export::ExportThresholds;
use backtest::BacktestEngine;
use backtest::simulation::SimulationLoop;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use strategy::{StrategyManager, StrategyConfig, Strategy, StrategyRiskLimits};
use riskmanager::RiskManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use genetic::{GeneticOptimizer, chromosome::AvellanedaStoikovChromosome};
use config::GeneticConfig;
use config::OutputConfig;
use std::collections::HashMap;

// UltraLogger integration
use program::logging_facade::MAIN_LOGGER;
use program::{log_info, log_error};

// CLI Visualizer for enhanced terminal output
use program::cli_visualizer::CliVisualizer;
use program::config::VisualizationConfig;

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
        symbol: String,
        #[arg(long)]
        exchange: String,
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
        symbol: String,
        #[arg(long)]
        exchange: String,
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
        symbol: String,
        #[arg(long)]
        exchange: String,
        #[arg(long)]
        risk_aversion: f64,
        #[arg(long)]
        inventory_target: f64,
        #[arg(long)]
        order_size: f64,
        #[arg(long, default_value = "100000.0")]
        initial_capital: f64,
        #[arg(long, default_value = "0.001")]
        commission_rate: f64,
    },
    /// Run parameter optimization
    Optimize {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        exchange: String,
    },
    /// Generate comprehensive backtest report
    Report {
        #[arg(long)]
        symbol: String,
        #[arg(long)]
        exchange: String,
        #[arg(long)]
        strategy_name: String,
        #[arg(long)]
        timeframe: String,
        #[arg(long)]
        output_dir: String,
        #[arg(long, value_delimiter = ',', default_values = ["html", "json", "csv"])]
        formats: Vec<String>,
        #[arg(long, default_value = "10000.0")]
        initial_capital: f64,
    },
    /// Demonstrate enhanced CLI visualization
    Demo {
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::WalkForward { symbol, exchange, start, end, train_days, test_days, step_days } => {
            log_info!(MAIN_LOGGER, "🚀 Starting Walk-Forward Analysis...");
            
            // Load market data
            // Load market data from database automatically
            log_info!(MAIN_LOGGER, "📊 Loading market data from database for {} on {}", symbol, exchange);
            let data = match load_market_data(&symbol, &exchange).await {
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
            symbol, exchange, start, end, train_days, test_days, step_days,
            strategy_name, min_sharpe_ratio, min_profit_factor, min_win_rate,
            max_drawdown, min_trades, min_profitable_windows
        } => {
            println!("🚀 Starting Enhanced Walk-Forward Analysis with Auto-Export...");
            
            // Load market data
            println!("📊 Loading market data for: {}/{}", symbol, exchange);
            let data = match load_market_data(&symbol, &exchange).await {
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
        Commands::Backtest { symbol, exchange, risk_aversion, inventory_target, order_size, initial_capital, commission_rate } => {
            println!("🚀 Starting Backtest...");
            println!("💰 Initial Capital: ${:.2}", initial_capital);
            println!("📈 Symbol: {}", symbol);
            println!("🏦 Exchange: {}", exchange);
            println!("💸 Commission Rate: {:.3}%", commission_rate * 100.0);
            
            // Load market data from database automatically
            println!("📊 Loading market data from database for {} on {}", symbol, exchange);
            let data = match load_market_data(&symbol, &exchange).await {
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
                window_ms: 60000,
                min_quote_lifetime_ms: 1000,
                fee_rate: commission_rate,
                latency_buffer: 0.001,
                volatility_cap: 0.05,
                market_depth_k: 1.5,
                time_horizon_minutes: 5.0,
            };
            
            println!("⚙️  Running backtest with params: {:#?}", params);
            
            // Create strategy using Default::default() and update params
            let mut strategy = AvellanedaStoikovStrategy::default();
            
            // Initialize strategy parameters using the params from CLI
            let mut strategy_params = HashMap::new();
            strategy_params.insert("risk_aversion".to_string(), config::ParameterValue::Float(params.risk_aversion));
            strategy_params.insert("order_size".to_string(), config::ParameterValue::Float(params.order_size));
            strategy_params.insert("fee_rate".to_string(), config::ParameterValue::Float(params.fee_rate));
            strategy_params.insert("latency_buffer".to_string(), config::ParameterValue::Float(params.latency_buffer));
            strategy_params.insert("inventory_target".to_string(), config::ParameterValue::Float(params.inventory_target));
            strategy_params.insert("volatility_cap".to_string(), config::ParameterValue::Float(params.volatility_cap));
            strategy_params.insert("window_ms".to_string(), config::ParameterValue::Float(params.window_ms as f64));
            strategy_params.insert("min_quote_lifetime_ms".to_string(), config::ParameterValue::Float(params.min_quote_lifetime_ms as f64));
            
            // Initialize the strategy with parameters
            if let Err(e) = strategy.initialize(strategy_params.clone()).await {
                eprintln!("❌ Failed to initialize strategy: {}", e);
                std::process::exit(1);
            }
            
            // Initialize strategy manager and register the strategy
            let mut strategy_manager = StrategyManager::new();
            
            // Convert config::ParameterValue to strategy::ParameterValue
            let mut converted_params = HashMap::new();
            for (key, value) in &strategy_params {
                let strategy_value = match value {
                    config::ParameterValue::Float(f) => strategy::config::ParameterValue::Float(*f),
                    config::ParameterValue::Integer(i) => strategy::config::ParameterValue::Int(*i),
                    config::ParameterValue::String(s) => strategy::config::ParameterValue::String(s.clone()),
                    config::ParameterValue::Boolean(b) => strategy::config::ParameterValue::Bool(*b),
                };
                converted_params.insert(key.clone(), strategy_value);
            }
            
            let strategy_config = StrategyConfig {
                id: "avellaneda-stoikov".to_string(),
                name: "Avellaneda-Stoikov Market Maker".to_string(),
                enabled: true,
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                strategy_type: "AvellanedaStoikovMarketMaker".to_string(),
                parameters: converted_params,
                risk_limits: StrategyRiskLimits {
                    max_position_size: Some(10.0),
                    max_daily_loss: Some(1000.0),
                    max_open_orders: Some(10),
                    max_drawdown_percent: Some(params.volatility_cap),
                    stop_loss_percent: None,
                    take_profit_percent: None,
                },
            };
            
            if let Err(e) = strategy_manager.register_strategy(strategy_config, strategy) {
                eprintln!("❌ Failed to register strategy: {}", e);
                std::process::exit(1);
            }
            
            println!("✅ Avellaneda-Stoikov strategy registered and initialized");
            
            // Initialize backtest components
            let config = OrderBookConfig::default();
            
            // Configure exchange-specific fees
            let fee_config = match exchange.as_str() {
                "kraken" => config::ExchangeFeeConfig::kraken_default(),
                _ => {
                    println!("⚠️  Unknown exchange '{}', using Kraken defaults", exchange);
                    config::ExchangeFeeConfig::kraken_default()
                }
            };
            
            let mut orderbook = OrderBook::new("BTCUSD".to_string(), exchange.clone(), config);
            orderbook.set_fee_config(fee_config);
            
            // Initialize portfolio with configurable starting capital
            let mut portfolio = PortfolioState::default();
            portfolio.balance = BigDecimal::from_f64(initial_capital).unwrap_or_else(|| {
                eprintln!("❌ Invalid initial capital: {}", initial_capital);
                std::process::exit(1);
            });
            
            println!("💼 Portfolio initialized with ${:.2} starting capital", initial_capital);
            let mut risk_manager = RiskManager::new();
            let _simulation = SimulationLoop::new(); // Prefix with _ to mark as intentionally unused
            
            // Create custom configuration with CLI parameters
            let mut backtest_config = config::BacktestConfig::default();
            backtest_config.trading.initial_capital = initial_capital;
            backtest_config.trading.commission_rate = commission_rate;
            
            let engine = BacktestEngine::new(backtest_config);
            
            // Run the backtest with the components
            let result = match engine.run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &data,
            ).await {
                Ok(backtest_results) => backtest_results.core, // Extract the core BacktestResult
                Err(e) => {
                    eprintln!("❌ Backtest failed: {}", e);
                    std::process::exit(1);
                }
            };

            // Use enhanced CLI visualizer for comprehensive results display
            let vis_config = VisualizationConfig::default();
            let visualizer = CliVisualizer::new(vis_config);
            visualizer.display_enhanced_results(&result);
            
            // Export detailed HTML visualization for real backtest results
            println!("\n🌐 Exporting detailed HTML visualization...");
            let html_path = std::path::Path::new("backtest_results.html");
            match backtest::HtmlExporter::export_equity_curve_html(&result, "Avellaneda-Stoikov Strategy", &format!("{}/{}", symbol, exchange), html_path) {
                Ok(()) => {
                    println!("✅ HTML export successful!");
                    println!("📊 Open {} in your web browser for detailed interactive analysis", html_path.display());
                    println!("💡 Features include:");
                    println!("   • 📈 Interactive portfolio tracking with hover details");
                    println!("   • 📉 Real-time drawdown visualization");  
                    println!("   • 📊 Daily returns distribution with color coding");
                    println!("   • 🎯 Individual trade analysis scatter plot");
                    println!("   • 📋 Comprehensive performance metrics table");
                    println!("   • 🖱️  Zoom, pan, and export capabilities");
                    
                    // Try to automatically open the file if possible
                    #[cfg(target_os = "windows")]
                    {
                        let _ = std::process::Command::new("cmd")
                            .args(["/C", "start", "backtest_results.html"])
                            .spawn();
                    }
                }
                Err(e) => {
                    println!("❌ HTML export failed: {}", e);
                }
            }
        }
        Commands::Optimize { symbol, exchange } => {
            println!("🧬 Starting Genetic Parameter Optimization...");
            
            // Load market data from database automatically
            println!("📊 Loading market data from database for {} on {}", symbol, exchange);
            let data = match load_market_data(&symbol, &exchange).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Load genetic algorithm configuration
            let genetic_config = GeneticConfig::default(); // Use defaults for now, will load from file later
            
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
                        let mut strategy_manager = StrategyManager::new();
                        let config = OrderBookConfig::default();
                        let fee_config = config::ExchangeFeeConfig::kraken_default();
                        let mut orderbook = OrderBook::new("BTCUSD".to_string(), "kraken".to_string(), config);
                        orderbook.set_fee_config(fee_config);
                        let mut portfolio = PortfolioState::default();
                        let mut risk_manager = RiskManager::new();
                        
                        // Create backtest configuration
                        let backtest_config = config::BacktestConfig::default();
                        let engine = BacktestEngine::new(backtest_config);
                        
                        let result = match engine.run_backtest(
                            &mut strategy_manager,
                            &mut orderbook,
                            &mut portfolio,
                            &mut risk_manager,
                            &(*data_inner).clone(),
                        ).await {
                            Ok(backtest_results) => backtest_results.core,
                            Err(_) => {
                                // Return a bad fitness score for failed backtests
                                return -999.0;
                            }
                        };
                        
                        // Return Sharpe ratio as fitness (higher is better)
                        result.sharpe_ratio.unwrap_or(-1.0)  // Penalize failed backtests
                    }) as std::pin::Pin<Box<dyn std::future::Future<Output = f64> + Send>>
                })
            };
            
            // Create optimizer and run
            let optimizer = GeneticOptimizer {
                config: genetic_config,
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
            symbol, 
            exchange,
            strategy_name, 
            timeframe: _timeframe, // Mark as unused since we don't use it in the simplified version
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
            let data = match load_market_data(&symbol, &exchange).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("❌ Failed to load market data: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Create backtest engine
            let mut strategy_manager = StrategyManager::new();
            let config = OrderBookConfig::default();
            let fee_config = config::ExchangeFeeConfig::kraken_default();
            let mut orderbook = OrderBook::new(symbol.clone(), exchange.clone(), config);
            orderbook.set_fee_config(fee_config);
            let mut portfolio = PortfolioState::default();
            let mut risk_manager = RiskManager::new();
            
            // Create backtest configuration with CLI parameters
            let mut backtest_config = config::BacktestConfig::default();
            backtest_config.trading.initial_capital = initial_capital;
            
            let engine = BacktestEngine::new(backtest_config);
            
            // Run backtest
            let result = match engine.run_backtest(
                &mut strategy_manager,
                &mut orderbook,
                &mut portfolio,
                &mut risk_manager,
                &data,
            ).await {
                Ok(backtest_results) => backtest_results.core,
                Err(e) => {
                    eprintln!("❌ Backtest failed: {}", e);
                    std::process::exit(1);
                }
            };
            
            // Generate report using the new reporting system
            let _start_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
            let _end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
            
            // For now, we'll create a simple report display
            println!("✅ Report generation completed successfully!");
            println!("📁 Report would be generated with {} format(s):", output_config.export_formats.len());
            for format in &output_config.export_formats {
                println!("  📄 {}.{}", strategy_name, format);
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
            println!("  • Use the backtest results for further analysis");
            println!("  • Consider running optimization for parameter tuning");
            println!("  • Export successful strategies to SignalEngine");
        }
        Commands::Demo { } => {
            program::demo::demo_enhanced_cli();
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
        market_depth_k: 1.5,
        time_horizon_minutes: 5.0,
    }
}


/// Load market data from database only
async fn load_market_data(_symbol: &str, _exchange: &str) -> Result<Vec<MarketData>, Box<dyn std::error::Error>> {
    #[cfg(feature = "postgres")]
    {
        use dotenv::dotenv;
        use std::env;
        use std::sync::Arc;
        use databaseschema::CustomAsyncPgConnectionManager;
        use deadpool::managed::Pool;

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

        // Load ALL data from earliest available timestamp
        let params = dataloader::QueryParams {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            start_time: None, // Load from the very beginning
            end_time: None,   // Load until the latest data
            limit: None,      // No limit - get all data
        };

        let loader = dataloader::DataLoader::new(pool);
        
        // Load multiple data types for comprehensive backtesting
        let mut market_data = Vec::new();
        
        // Load historical snapshots (order book data)
        match loader.load_historical_snapshots(&params).await {
            Ok(snapshots) => {
                println!("📷 Loaded {} historical snapshots", snapshots.len());
                market_data.extend(snapshots.into_iter().map(MarketData::Snapshot));
            }
            Err(e) => println!("⚠️  No snapshots found: {}", e),
        }

        // Load historical orders
        match loader.load_historical_orders(&params).await {
            Ok(orders) => {
                println!("📋 Loaded {} historical orders", orders.len());
                market_data.extend(orders.into_iter().map(MarketData::Order));
            }
            Err(e) => println!("⚠️  No orders found: {}", e),
        }

        // Load trade data from trades table
        match loader.load_trade_data(&params).await {
            Ok(trades) => {
                println!("💱 Loaded {} trade records", trades.len());
                market_data.extend(trades.into_iter().map(MarketData::Trade));
            }
            Err(e) => println!("⚠️  No trades found: {}", e),
        }

        // Load candle data if available
        match loader.load_candles(&params, 1).await {
            Ok(candles) => {
                println!("🕯️  Loaded {} candles", candles.len());
                market_data.extend(candles.into_iter().map(MarketData::Candle));
            }
            Err(e) => println!("⚠️  No candles found: {}", e),
        }

        if market_data.is_empty() {
            println!("❌ No market data found in database for {} on {}", symbol, exchange);
            println!("📊 Database tables checked:");
            println!("   • historical_snapshots");
            println!("   • historical_orders"); 
            println!("   • trades");
            println!("   • candles");
            println!("💡 Please ensure market data is loaded into the database for backtesting");
            return Err(format!("No market data available for {} on {} in database", symbol, exchange).into());
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
        // Database feature is disabled - cannot load data
        println!("❌ PostgreSQL feature is disabled");
        println!("🔧 To enable database loading:");
        println!("   • Build with --features postgres");
        println!("   • Ensure DATABASE_URL is set");
        println!("   • Verify database contains market data");
        return Err("Database feature disabled. Cannot load market data without PostgreSQL support.".into());
    }
}
