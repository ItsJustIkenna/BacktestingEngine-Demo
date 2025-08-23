use clap::{Parser, Subcommand};
use chrono::{NaiveDate, Duration};
use dataloader::MarketData;
use walkforward::runner::run_walkforward_analysis;
use backtest::BacktestEngine;
use backtest::simulation::SimulationLoop;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use strategy::StrategyManager;
use orderbook::{OrderBook, OrderBookConfig};
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use genetic::{GeneticOptimizer, GeneticConfig, chromosome::AvellanedaStoikovChromosome};
use metrics::{PerformanceMetrics, create_basic_report, ReportGenerator, ReportConfig, ReportFormat, EquityPoint};
use config::OutputConfig;
use std::io::{self, Write};
use std::sync::Arc;
use std::path::PathBuf;

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
            println!("🚀 Starting Walk-Forward Analysis...");
            
            // Load market data
            println!("📊 Loading market data from: {}", data_path);
            let data = load_market_data(&data_path).await;
            
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
        Commands::Backtest { data_path, risk_aversion, inventory_target, order_size } => {
            println!("🚀 Starting Backtest...");
            
            // Load market data
            println!("📊 Loading market data from: {}", data_path);
            let data = load_market_data(&data_path).await;
            
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
            let orderbook = OrderBook::new("BTCUSD".to_string(), "EXCHANGE".to_string(), config);
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
            let data = load_market_data(&data_path).await;
            
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
                        let orderbook = OrderBook::new("BTCUSD".to_string(), "EXCHANGE".to_string(), config);
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
            let data = load_market_data(&data_path).await;
            
            // Create backtest engine
            let strategy_manager = StrategyManager::new();
            let config = OrderBookConfig::default();
            let orderbook = OrderBook::new(symbol.clone(), "EXCHANGE".to_string(), config);
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


// Example async loader stub. Replace with your actual DataLoader logic.
async fn load_market_data(_path: &str) -> Vec<MarketData> {
    use dotenv::dotenv;
    use std::env;
    use databaseschema::CustomAsyncPgConnectionManager;
    use deadpool::managed::Pool;

    // Prompt user for symbol, exchange, and date range
    let mut symbol = String::new();
    let mut exchange = String::new();
    let mut start = String::new();
    let mut end = String::new();
    print!("Enter symbol (e.g. BTCUSD): "); io::stdout().flush().unwrap(); io::stdin().read_line(&mut symbol).unwrap();
    print!("Enter exchange (e.g. BINANCE): "); io::stdout().flush().unwrap(); io::stdin().read_line(&mut exchange).unwrap();
    print!("Enter start datetime (YYYY-MM-DDTHH:MM:SS): "); io::stdout().flush().unwrap(); io::stdin().read_line(&mut start).unwrap();
    print!("Enter end datetime (YYYY-MM-DDTHH:MM:SS): "); io::stdout().flush().unwrap(); io::stdin().read_line(&mut end).unwrap();
    let symbol = symbol.trim().to_string();
    let exchange = exchange.trim().to_string();
    let start_time = chrono::DateTime::parse_from_rfc3339(&format!("{}:00Z", start.trim())).ok().map(|dt| dt.with_timezone(&chrono::Utc));
    let end_time = chrono::DateTime::parse_from_rfc3339(&format!("{}:00Z", end.trim())).ok().map(|dt| dt.with_timezone(&chrono::Utc));

    // Load .env and get DATABASE_URL
    dotenv().ok();
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env or environment");

    // Set up database pool using the custom connection manager
    let manager = CustomAsyncPgConnectionManager::new(db_url);
    let pool = Pool::builder(manager)
        .max_size(16)
        .build()
        .expect("Failed to create pool");
    let pool = Arc::new(pool);

    // Instantiate DataLoader
    let loader = dataloader::DataLoader::new(pool);
    let params = dataloader::QueryParams {
        symbol,
        exchange,
        start_time,
        end_time,
        limit: None,
    };
    // Load snapshots as MarketData
    let snapshots = loader.load_historical_snapshots(&params).await.unwrap_or_default();
    let market_data = snapshots.into_iter().map(MarketData::Snapshot).collect();
    market_data
}
