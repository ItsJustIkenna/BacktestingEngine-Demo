# 🚀 BacktestingEngine

A minimal, high-performance backtesting engine for quantitative trading strategies.

## 🏗️ **Architecture**

```
BacktestingEngine/
├── program/          # Main CLI application
├── backtest/         # Core backtesting engine with HTML export
├── strategy/         # Trading strategy implementations  
├── orderbook/        # Order book management
├── portfoliomanager/ # Portfolio state management
├── riskmanager/      # Risk management systems
├── dataloader/       # Market data loading
├── walkforward/      # Walk-forward analysis
├── genetic/          # Genetic algorithm optimization
├── metrics/          # Performance metrics calculation
├── signal/           # Trading signal generation
└── config/           # Configuration management
```

## ⚡ **Quick Start**

### Build
```bash
cargo build --release
```

### Run a Backtest
```bash
cargo run -- backtest --symbol BTC --exchange kraken --initial-capital 100000
```
*Generates both terminal output and interactive HTML visualization (`backtest_results.html`)*

### Walk-Forward Analysis
```bash
cargo run -- walk-forward --symbol BTC --exchange kraken --start 2024-01-01 --end 2024-12-31 --train-days 30 --test-days 7 --step-days 7
```

### Genetic Optimization  
```bash
cargo run -- optimize --symbol BTC --exchange kraken
```

### Demo Mode
```bash
cargo run -- demo
```
*Shows enhanced visualization with sample data*

## 📊 **Visualization Features**

- **Terminal Output**: Real-time performance metrics with color coding
- **HTML Export**: Interactive equity curves with Plotly.js
  - Portfolio value tracking with hover details
  - Drawdown analysis with fill areas
  - Daily returns histogram 
  - Trade-by-trade scatter plots
  - Comprehensive metrics dashboard

## 🎯 **Core Components**

| Component | Purpose |
|-----------|---------|
| `BacktestEngine` | Orchestrates backtesting simulation |
| `AvellanedaStoikovStrategy` | Market making strategy implementation |
| `OrderBook` | Manages order book state and matching |
| `PortfolioState` | Tracks positions, balance, P&L |
| `RiskManager` | Enforces risk limits and controls |
| `WalkForwardAnalysis` | Time-series cross-validation |
| `GeneticOptimizer` | Parameter optimization via GA |
| `HtmlExporter` | Interactive visualization export |

## 🔧 **Configuration**

All configuration is handled through the CLI interface. No external config files required.

## 📈 **Performance**

- **Minimal overhead**: Streamlined codebase focused on core functionality
- **Memory efficient**: Optimized data structures for high-frequency data
- **Concurrent processing**: Async/await for I/O operations
- **Type safety**: Rust's ownership system prevents runtime errors

## 🚦 **Status**

✅ **Production Ready**
- Core backtesting engine
- Interactive HTML visualizations  
- Walk-forward analysis
- Genetic optimization
- Risk management

---

*BacktestingEngine - Minimal. Fast. Reliable.*
