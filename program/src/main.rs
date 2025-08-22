use clap::{Parser, Subcommand};
use chrono::{NaiveDate, Duration};
use dataloader::MarketData;
use walkforward::runner::run_walkforward_analysis;
use backtest::BacktestEngine;
use backtest::simulation::SimulationLoop;
use strategy::strategies::avellaneda_stoikov::{AvellanedaStoikovParams, AvellanedaStoikovStrategy};
use strategy::StrategyManager;
use orderbook::OrderBook;
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use config::BacktestConfig;
use metrics::PerformanceMetrics;
use std::io::{self, Write};
use std::sync::Arc;

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
        quote_width: f64,
    },
    /// Run parameter optimization
    Optimize {
        #[arg(long)]
        data_path: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::WalkForward { data_path, start, end, train_days, test_days, step_days } => {
            let data = load_market_data(&data_path).await;
            let start = NaiveDate::parse_from_str(&start, "%Y-%m-%d").expect("Invalid start date");
            let end = NaiveDate::parse_from_str(&end, "%Y-%m-%d").expect("Invalid end date");
            let train_size = Duration::days(train_days);
            let test_size = Duration::days(test_days);
            let step_size = Duration::days(step_days);
            let summary = run_walkforward_analysis(&data, start, end, train_size, test_size, step_size).await;
            println!("Walk-Forward Summary: {:#?}", summary);
        }
        Commands::Backtest { data_path, risk_aversion, inventory_target, quote_width } => {
            let data = load_market_data(&data_path).await;
            let params = AvellanedaStoikovParams {
                risk_aversion,
                inventory_target,
                quote_width,
                ..Default::default()
            };
            let mut strategy = AvellanedaStoikovStrategy::default();
            let mut param_map = std::collections::HashMap::new();
            param_map.insert("risk_aversion".to_string(), strategy::ParameterValue::Float(params.risk_aversion));
            param_map.insert("inventory_target".to_string(), strategy::ParameterValue::Float(params.inventory_target));
            param_map.insert("quote_width".to_string(), strategy::ParameterValue::Float(params.quote_width));
            let _ = strategy.initialize(param_map).await;
            let mut strategy_manager = StrategyManager::default();
            strategy_manager.add_strategy(Box::new(strategy));
            let mut orderbook = OrderBook::default();
            let mut portfolio = PortfolioState::default();
            let mut risk_manager = RiskManager::default();
            let mut sim = SimulationLoop::new();
            let result = sim
                .run(
                    &mut strategy_manager,
                    &mut orderbook,
                    &mut portfolio,
                    &mut risk_manager,
                    data,
                    &BacktestConfig::default(),
                )
                .await;
            println!("Backtest Result: {:#?}", result);
        }
        Commands::Optimize { data_path } => {
            use genetic::chromosome::AvellanedaStoikovChromosome;
            use genetic::{GeneticConfig, GeneticOptimizer};
            use genetic::fitness::avellaneda_stoikov_fitness;
            use std::sync::Arc;
            use strategy::strategies::avellaneda_stoikov::AvellanedaStoikovParams;
            use config::BacktestConfig;

            let data = load_market_data(&data_path).await;
            let config = GeneticConfig {
                population_size: 20,
                generations: 10,
                crossover_rate: 0.7,
                mutation_rate: 0.2,
            };
            let data_vec = data.clone();
            let backtest_config = BacktestConfig::default();
            let fitness_fn = Arc::new(move |chromo: &AvellanedaStoikovChromosome| {
                let data = data_vec.clone();
                let config = backtest_config.clone();
                Box::pin(avellaneda_stoikov_fitness(chromo, data, &config))
            });
            let optimizer = GeneticOptimizer {
                config,
                fitness_fn,
            };
            let (best_chromo, _score) = optimizer.run().await;
            let best_params = AvellanedaStoikovParams {
                risk_aversion: best_chromo.risk_aversion,
                inventory_target: best_chromo.inventory_target,
                quote_width: best_chromo.quote_width,
                ..Default::default()
            };
            println!("Best Parameters: {:#?}", best_params);
        }
    }
}


// Example async loader stub. Replace with your actual DataLoader logic.
async fn load_market_data(_path: &str) -> Vec<MarketData> {
    use dotenv::dotenv;
    use std::env;
    use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod};
    use tokio_postgres::NoTls;

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

    // Set up deadpool_postgres pool
    let mut cfg = Config::new();
    cfg.dbname = None;
    cfg.user = None;
    cfg.password = None;
    cfg.host = None;
    cfg.manager = Some(ManagerConfig { recycling_method: RecyclingMethod::Fast });
    cfg.url = Some(db_url);
    let pool = cfg.create_pool(Some(deadpool_postgres::Runtime::Tokio1), NoTls).expect("Failed to create pool");
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
