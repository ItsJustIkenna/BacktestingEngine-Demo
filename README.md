# BacktestingEngine

A high-performance, modular backtesting engine for quantitative trading strategies built in Rust. This engine provides comprehensive tools for strategy development, testing, optimization, and performance analysis with enterprise-grade reliability and speed.

## 🚀 Features

### Core Capabilities
- **High-Performance Backtesting**: Rust-based engine optimized for speed and memory efficiency
- **Modular Architecture**: Loosely coupled components for maximum flexibility
- **Strategy Optimization**: Built-in genetic algorithms and Monte Carlo simulation
- **Risk Management**: Comprehensive risk controls and portfolio management
- **Real-time Order Book**: Advanced order matching and market simulation
- **Performance Analytics**: Detailed reporting and visualization tools
- **Configuration Management**: Centralized, YAML-based configuration system

### Trading Features
- Multiple asset class support
- Advanced order types (Market, Limit, Stop, Stop-Limit)
- Portfolio rebalancing and allocation strategies
- Risk-adjusted performance metrics (Sharpe, Sortino, Calmar ratios)
- Drawdown analysis and risk controls
- Transaction cost modeling

### Technical Features
- Multi-threaded execution
- Memory-efficient data handling
- Real-time metric calculation
- Extensible plugin architecture
- Database integration for historical data
- Export capabilities (CSV, JSON, Excel)

## 📁 Project Structure

```
BacktestingEngine/
├── backtest/           # Core backtesting engine logic
├── config/             # Centralized configuration management
├── dataloader/         # Data ingestion and batch processing
├── genetic/            # Genetic algorithm optimization
├── metrics/            # Performance metrics and reporting
├── orderbook/          # Order book simulation and matching
├── portfoliomanager/   # Portfolio management and allocation
├── program/            # Main application entry point
├── riskmanager/        # Risk management and controls
├── strategy/           # Strategy implementation framework
└── target/             # Build artifacts
```

## 🛠️ Installation

### Prerequisites
- Rust 1.70+ (latest stable recommended)
- PostgreSQL or compatible database
- Git

### Quick Start

1. **Clone the repository**
```bash
git clone https://github.com/Nwagbara-Group-LLC/BacktestingEngine.git
cd BacktestingEngine
```

2. **Install dependencies**
```bash
cargo build --release
```

3. **Configure the engine**
```bash
cp sample_config.yaml config.yaml
# Edit config.yaml with your settings
```

4. **Run a sample backtest**
```bash
cargo run --bin backtest_engine
```

## ⚙️ Configuration

The engine uses a centralized configuration system located in the `config/` module. All configuration options are defined in YAML format.

### Sample Configuration

```yaml
# Backtest Configuration
backtest:
  start_date: "2020-01-01"
  end_date: "2023-12-31"
  initial_capital: 1000000.0
  benchmark: "SPY"

# Strategy Configuration
strategy:
  name: "MeanReversion"
  parameters:
    lookback_period: 20
    entry_threshold: -2.0
    exit_threshold: 0.0

# Risk Management
risk:
  max_position_size: 0.05
  max_drawdown: 0.20
  stop_loss: 0.10

# Data Configuration
data:
  provider: "database"
  batch_size: 10000
  cache_size: 100000

# Reporting
reporting:
  formats: ["json", "csv"]
  output_dir: "./results"
  include_trades: true
```

### Configuration Modules

- **BacktestConfig**: Core backtesting parameters
- **StrategyConfig**: Strategy-specific settings
- **RiskConfig**: Risk management rules
- **DataConfig**: Data loading and processing settings
- **ReportConfig**: Output and reporting options
- **GeneticConfig**: Genetic algorithm optimization
- **MonteCarloConfig**: Monte Carlo simulation parameters

## 🏃‍♂️ Usage

### Basic Backtesting

```rust
use backtest::{BacktestEngine, BacktestConfig};
use strategy::Strategy;
use config::BacktestConfig;

// Load configuration
let config = BacktestConfig::from_file("config.yaml")?;

// Create strategy
let strategy = MyStrategy::new(config.strategy);

// Run backtest
let engine = BacktestEngine::new(config);
let results = engine.run(strategy).await?;

// Generate report
let report = results.generate_report(&config.reporting)?;
println!("Sharpe Ratio: {}", report.sharpe_ratio);
```

### Strategy Development

```rust
use strategy::{Strategy, StrategyContext, Signal};
use orderbook::Order;

pub struct MeanReversionStrategy {
    lookback: usize,
    threshold: f64,
}

impl Strategy for MeanReversionStrategy {
    fn generate_signals(&self, context: &StrategyContext) -> Vec<Signal> {
        let prices = context.get_prices(self.lookback);
        let mean = prices.iter().sum::<f64>() / prices.len() as f64;
        let current = context.current_price();
        
        if current < mean - self.threshold {
            vec![Signal::Buy { symbol: context.symbol.clone(), quantity: 100 }]
        } else if current > mean + self.threshold {
            vec![Signal::Sell { symbol: context.symbol.clone(), quantity: 100 }]
        } else {
            vec![]
        }
    }
}
```

### Optimization

```rust
use genetic::{GeneticOptimizer, GeneticConfig};

// Configure genetic algorithm
let genetic_config = GeneticConfig {
    population_size: 100,
    generations: 50,
    mutation_rate: 0.1,
    crossover_rate: 0.8,
};

// Run optimization
let optimizer = GeneticOptimizer::new(genetic_config);
let best_params = optimizer.optimize(strategy, data).await?;
```

## 📊 Performance Metrics

The engine calculates comprehensive performance metrics:

### Return Metrics
- **Total Return**: Cumulative return over the backtest period
- **Annualized Return**: Geometric mean return annualized
- **Monthly/Daily Returns**: Period-specific performance breakdown

### Risk Metrics
- **Sharpe Ratio**: Risk-adjusted return measure
- **Sortino Ratio**: Downside deviation-adjusted return
- **Calmar Ratio**: Return to maximum drawdown ratio
- **Maximum Drawdown**: Largest peak-to-trough decline
- **Value at Risk (VaR)**: Potential loss at given confidence level

### Trading Metrics
- **Win Rate**: Percentage of profitable trades
- **Average Win/Loss**: Mean profit/loss per trade
- **Profit Factor**: Gross profits / Gross losses
- **Total Trades**: Number of round-trip trades executed

## 🔧 Modules

### Core Components

#### Backtest Engine (`backtest/`)
- Main simulation engine
- Event-driven architecture
- Market data replay
- Strategy execution coordination

#### Configuration (`config/`)
- Centralized configuration management
- YAML-based settings
- Runtime validation
- Parameter optimization support

#### Data Loader (`dataloader/`)
- Market data ingestion
- Batch processing
- Data validation and cleaning
- Multiple data source support

#### Order Book (`orderbook/`)
- Realistic market simulation
- Order matching engine
- Bid/ask spread modeling
- Slippage and transaction costs

#### Portfolio Manager (`portfoliomanager/`)
- Position tracking
- Cash management
- Allocation strategies
- Rebalancing logic

#### Risk Manager (`riskmanager/`)
- Real-time risk monitoring
- Position sizing rules
- Stop-loss execution
- Exposure limits

#### Strategy Framework (`strategy/`)
- Strategy abstraction layer
- Signal generation interface
- Technical indicator library
- Custom strategy development tools

#### Metrics & Reporting (`metrics/`)
- Performance calculation
- Report generation
- Data visualization
- Export functionality

#### Optimization (`genetic/`)
- Genetic algorithm implementation
- Parameter optimization
- Walk-forward analysis
- Overfitting prevention

## 🧪 Testing

Run the test suite:

```bash
# Run all tests
cargo test

# Run specific module tests
cargo test --package config
cargo test --package backtest

# Run with coverage
cargo test --all-features --no-fail-fast
```

## 📈 Benchmarks

Performance benchmarks on modern hardware:

- **Backtest Speed**: 1M+ bars/second
- **Memory Usage**: <100MB for typical strategies
- **Optimization**: 1000+ parameter combinations/minute
- **Concurrent Strategies**: 50+ strategies simultaneously

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

### Development Setup

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes and add tests
4. Ensure all tests pass: `cargo test`
5. Format code: `cargo fmt`
6. Check lints: `cargo clippy`
7. Submit a pull request

### Code Standards

- Follow Rust standard formatting (`cargo fmt`)
- Add comprehensive tests for new features
- Document public APIs with rustdoc
- Use meaningful commit messages
- Maintain backwards compatibility

## 📚 Documentation

- **API Documentation**: Run `cargo doc --open`
- **Examples**: See `examples/` directory
- **Tutorials**: Check the `docs/` folder
- **Performance Guide**: `docs/performance.md`
- **Strategy Development**: `docs/strategy-guide.md`

## 🚀 Roadmap

### Version 2.0 (Planned)
- [ ] Real-time trading integration
- [ ] Machine learning strategy templates
- [ ] Cloud deployment support
- [ ] Advanced portfolio analytics
- [ ] Multi-timeframe backtesting

### Version 1.5 (In Progress)
- [x] Configuration centralization
- [ ] Enhanced reporting dashboard
- [ ] Options and derivatives support
- [ ] Performance improvements

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built with ❤️ by [Nwagbara Group LLC](https://github.com/Nwagbara-Group-LLC)
- Inspired by modern quantitative research methodologies
- Special thanks to the Rust community for excellent libraries

## 📞 Support

- **Issues**: [GitHub Issues](https://github.com/Nwagbara-Group-LLC/BacktestingEngine/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Nwagbara-Group-LLC/BacktestingEngine/discussions)
- **Email**: support@nwagbara-group.com

---

**⚡ Built with Rust for maximum performance and reliability**