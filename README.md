# BacktestingEngine (demo)

An event-driven backtesting and parameter-optimization engine written in Rust. It simulates strategies against historical trade and order-book data with a matching-engine style order book and queue-position modelling, tunes strategy parameters with a parallel genetic algorithm, checks stability with walk-forward analysis and regime-aware Monte Carlo resampling, and applies portfolio and risk limits (VaR/CVaR style) during the run. Two example strategies are included, Avellaneda–Stoikov market making and a momentum strategy with regime filters. It is research code: results are simulated, and nothing here has been run against real capital.

## Architecture

```
 historical data ──► dataloader / data_prep ──► backtest (simulation loop)
 (TimescaleDB or      (DB chunks → binary        │   ├── orderbook (matching, queue position)
  Tardis CSV files)    files, features)          │   ├── strategy (Avellaneda–Stoikov, momentum)
                                                 │   ├── portfoliomanager (positions)
                                                 │   ├── riskmanager (limits)
                                                 │   └── smartrouter (venue selection model)
                                                 ▼
        genetic (parallel GA, rayon) ──► walkforward ──► metrics (Sharpe, drawdown, ...)
        regime_mc (Monte Carlo)                                   │
                                                                  ▼
                              program (CLI: serve | worker | coordinator | backtest-*)
```

Workspace members: `backtest`, `metrics`, `smartrouter`, `strategy`, `genetic`, `walkforward`, `orderbook`, `portfoliomanager`, `riskmanager`, `signal`, `dataloader`, `data_prep`, `config`, `program`, and `local/` (`ultra-logger`, `protocol`, `publisher`, `databaseschema`). A React UI lives in `gui/`.

## Build and test

Requires a recent stable Rust toolchain (edition 2021).

```bash
cargo build --workspace
cargo test --workspace
```

The CLI is the `program` package; its own name is `backtesting-engine`. Use `cargo run --release -p program -- --help` to list the subcommands (`serve`, `worker`, `distributed-worker`, `coordinator`, `prepare-data`, `backtest-tardis`, `backtest-momentum`, among others). `backtest-tardis` runs from local Tardis CSV files and needs no database for market data. `serve` starts the HTTP API (default `0.0.0.0:8080`) and the database-backed modes expect PostgreSQL/TimescaleDB; the Diesel models live in `local/databaseschema`, which has no SQL migrations in this repo. Docker and Helm files are under `deployment/`.

## Tests

`cargo test --workspace` runs about 780 test functions (static count), unit and integration tests, across `orderbook`, `genetic`, `config`, `strategy`, `riskmanager`, `walkforward`, `metrics`, `backtest` and `program`.
