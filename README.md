# BacktestingEngine - Demo Version

> **Note**: This is a public demonstration version with sanitized configurations and example parameters.

## Overview

High-performance backtesting and genetic optimization platform for algorithmic trading strategies, built in Rust.

## Features

- **Genetic Algorithm** optimizer with Monte Carlo validation
- **Walk-Forward Analysis** for parameter stability
- **Market Making** strategies (Avellaneda-Stoikov)
- **Momentum Strategies** with regime filters
- **Risk Management** with VaR/CVaR limits
- **TimescaleDB** integration for time-series data
- **React UI** for visualization

## Quick Start

```bash
# Build
cargo build --workspace --release

# Run migrations
cd databaseschema && diesel migration run

# Start API
cargo run --release -- serve --host 0.0.0.0 --port 8080

# Run backtest
cargo run --release -- backtest --symbol BTCUSD --exchange kraken
```

## Documentation

See `/docs` for detailed documentation.

## License

See LICENSE file.

## Disclaimer

For educational purposes only. Test thoroughly before using with real capital.
