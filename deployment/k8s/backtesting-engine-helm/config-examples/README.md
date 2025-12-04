# Backtesting Engine Configuration Examples

This directory contains configuration templates for different analysis modes of the Backtesting Engine.

## Configuration Files

### `analysis-modes-development.yaml`
- **Purpose**: Fast iteration during strategy development
- **Characteristics**: 
  - Short timeframes (3 months)
  - Disabled Monte Carlo and Walk-Forward
  - 30-second timeout
  - Minimal reporting
- **Use Case**: Daily development and quick strategy testing

### `analysis-modes-validation.yaml`  
- **Purpose**: Strategy validation with statistical analysis
- **Characteristics**:
  - Medium timeframes (2.5 years)
  - Enabled Monte Carlo (2,500 runs)
  - Disabled Walk-Forward
  - 10-minute timeout
- **Use Case**: Pre-production strategy validation

### `analysis-modes-production.yaml`
- **Purpose**: Full production backtesting with comprehensive analysis
- **Characteristics**:
  - Long timeframes (5 years)
  - Full Monte Carlo (10,000 runs)
  - Full Walk-Forward analysis
  - 1-hour timeout
  - Complete reporting suite
- **Use Case**: Production strategy deployment validation

## Usage with Helm

### Development Environment
```bash
helm install backtesting-dev ./backtesting-engine-helm \
  -f values-dev.yaml \
  -f config-examples/analysis-modes-development.yaml
```

### Validation Environment
```bash
helm install backtesting-validation ./backtesting-engine-helm \
  -f values-staging.yaml \
  -f config-examples/analysis-modes-validation.yaml
```

### Production Environment
```bash
helm install backtesting-prod ./backtesting-engine-helm \
  -f values-prod.yaml \
  -f config-examples/analysis-modes-production.yaml
```

## Configuration Override

You can override specific settings using Helm's `--set` flag:

```bash
# Override analysis mode
helm upgrade backtesting-prod ./backtesting-engine-helm \
  --set analysis.mode=production \
  --set analysis.monte_carlo_runs=20000

# Override database settings
helm upgrade backtesting-prod ./backtesting-engine-helm \
  --set database.max_connections=30 \
  --set trading.initial_capital=2000000.0
```

## Environment Variables

These configurations work with the following environment variables:
- `BACKTEST_CONFIG_PATH`: Path to configuration file
- `BACKTEST_MODE`: Override analysis mode (development|validation|production)
- `MONTE_CARLO_RUNS`: Override Monte Carlo iterations
- `TIMEOUT_SECONDS`: Override analysis timeout

## Customization

To create custom configurations:

1. Copy one of the existing files
2. Modify the parameters as needed  
3. Reference in your Helm deployment
4. Test in development environment first

## Auto-Escalation

The analysis modes support automatic escalation based on:
- **Duration**: Longer backtests auto-escalate to higher modes
- **Performance**: Better strategies (Sharpe > threshold) auto-escalate
- **Trade Count**: More trades trigger deeper analysis

This ensures optimal resource usage while maintaining analysis quality.
