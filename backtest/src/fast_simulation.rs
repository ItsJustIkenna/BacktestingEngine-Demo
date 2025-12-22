//! Fast Vectorized Simulation for Genetic Algorithm Fitness Evaluation
//!
//! This module provides a high-performance simulation path optimized for rapid
//! fitness evaluation during genetic optimization. Key optimizations:
//!
//! 1. **Vectorized Operations**: Process entire arrays at once using SIMD
//! 2. **Pre-computed Features**: Calculate volatility, signals once for all data
//! 3. **Synchronous Execution**: No async/await overhead in hot path
//! 4. **Minimal Allocations**: Reuse buffers, avoid cloning
//! 5. **Simplified Order Matching**: Approximate fills without full orderbook
//!
//! This is ~50-100x faster than event-by-event simulation, suitable for
//! evaluating thousands of chromosomes during genetic optimization.
//!
//! # Usage
//! ```ignore
//! let simulator = FastSimulator::new(market_data, initial_capital);
//! let result = simulator.evaluate(&chromosome);
//! ```

use dataloader::Tick;

/// Pre-computed market features for fast simulation
#[derive(Debug)]
pub struct MarketFeatures {
    /// Raw prices (mid prices from trades)
    pub prices: Vec<f64>,
    /// Timestamps as unix millis
    pub timestamps: Vec<i64>,
    /// Pre-computed log returns
    pub log_returns: Vec<f64>,
    /// EWMA volatility at each point (annualized)
    pub volatility: Vec<f64>,
    /// Trade sides: 1.0 = buy aggressor, -1.0 = sell aggressor
    pub trade_sides: Vec<f64>,
    /// Trade volumes in base currency
    pub volumes: Vec<f64>,
    /// Cumulative buy volume (for order flow imbalance)
    pub cum_buy_volume: Vec<f64>,
    /// Cumulative sell volume
    pub cum_sell_volume: Vec<f64>,
}

impl MarketFeatures {
    /// Extract and pre-compute features from market data
    /// This is done ONCE before evaluating multiple chromosomes
    pub fn from_market_data(data: &[dataloader::MarketData]) -> Self {
        let n = data.len();
        let mut prices = Vec::with_capacity(n);
        let mut timestamps = Vec::with_capacity(n);
        let mut trade_sides = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);
        
        // Extract raw data - optimized for TradeData which has pre-computed f64 values
        // Progress: log every 1M records
        let mut processed = 0usize;
        for md in data {
            match md {
                dataloader::MarketData::Trade(t) => {
                    prices.push(t.price_f64);
                    timestamps.push(t.timestamp.timestamp_millis());
                    volumes.push(t.quantity_f64);
                    trade_sides.push(match t.side.as_str() {
                        "buy" | "Buy" | "BUY" | "b" | "B" => 1.0,
                        "sell" | "Sell" | "SELL" | "s" | "S" => -1.0,
                        _ => 0.0,
                    });
                }
                dataloader::MarketData::Candle(c) => {
                    // Use pre-computed or convert BigDecimal (slower path)
                    use bigdecimal::ToPrimitive;
                    let price = c.close.to_f64().unwrap_or(0.0);
                    prices.push(price);
                    timestamps.push(c.timestamp.timestamp_millis());
                    volumes.push(c.volume.to_f64().unwrap_or(0.0));
                    trade_sides.push(0.0); // No side info for candles
                }
                #[cfg(feature = "postgres")]
                dataloader::MarketData::Snapshot(s) => {
                    use bigdecimal::ToPrimitive;
                    let price = s.price_level.to_f64().unwrap_or(0.0);
                    prices.push(price);
                    timestamps.push(s.timestamp.timestamp_millis());
                    volumes.push(0.0);
                    trade_sides.push(0.0);
                }
                #[cfg(feature = "postgres")]
                dataloader::MarketData::Order(o) => {
                    use bigdecimal::ToPrimitive;
                    let price = o.price_level.to_f64().unwrap_or(0.0);
                    prices.push(price);
                    timestamps.push(o.timestamp.timestamp_millis());
                    volumes.push(o.quantity.to_f64().unwrap_or(0.0));
                    trade_sides.push(0.0);
                }
                #[allow(unreachable_patterns)]
                _ => {} // Fallback for any other variants
            }
            processed += 1;
            if processed % 1_000_000 == 0 {
                eprintln!("  ... extracted {}/{}M features", processed / 1_000_000, n / 1_000_000);
            }
        }
        eprintln!("  Feature extraction complete: {} records", processed);
        
        // Compute log returns
        eprintln!("  Computing log returns for {} prices...", prices.len());
        let log_returns = compute_log_returns(&prices);
        eprintln!("  Log returns computed: {} values", log_returns.len());
        
        // Compute EWMA volatility (vectorized)
        eprintln!("  Computing EWMA volatility...");
        let volatility = compute_ewma_volatility(&log_returns, 0.94);
        eprintln!("  Volatility computed: {} values", volatility.len());
        
        // Compute cumulative volumes
        eprintln!("  Computing cumulative volumes...");
        let (cum_buy_volume, cum_sell_volume) = compute_cumulative_volumes(&volumes, &trade_sides);
        eprintln!("  Feature computation complete!");
        
        Self {
            prices,
            timestamps,
            log_returns,
            volatility,
            trade_sides,
            volumes,
            cum_buy_volume,
            cum_sell_volume,
        }
    }
    
    /// Get the number of data points
    pub fn len(&self) -> usize {
        self.prices.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.prices.is_empty()
    }
    
    /// Create MarketFeatures from a TickSource (mmap'd binary file)
    /// 
    /// This is optimized for distributed workers that read from binary files
    /// rather than in-memory MarketData arrays.
    /// 
    /// # Arguments
    /// * `source` - A TickSource (typically BinaryFileReader or SlicedTickSource)
    /// * `max_ticks` - Maximum number of ticks to read (for train/test split)
    pub fn from_tick_source<T: dataloader::TickSource>(source: &T, max_ticks: usize) -> Self {
        let n = max_ticks.min(source.len());
        let mut prices = Vec::with_capacity(n);
        let mut timestamps = Vec::with_capacity(n);
        let mut trade_sides = Vec::with_capacity(n);
        let mut volumes = Vec::with_capacity(n);
        
        // Extract data from tick source
        for i in 0..n {
            if let Some(tick) = source.get(i) {
                prices.push(tick.price());
                timestamps.push(tick.timestamp().timestamp_millis());
                volumes.push(tick.quantity());
                trade_sides.push(match tick.side() {
                    dataloader::Side::Buy => 1.0,
                    dataloader::Side::Sell => -1.0,
                });
            }
            
            if i > 0 && i % 1_000_000 == 0 {
                eprintln!("  ... extracted {}/{}M tick features", i / 1_000_000, n / 1_000_000);
            }
        }
        eprintln!("  Tick feature extraction complete: {} records", prices.len());
        
        // Compute derived features
        let log_returns = compute_log_returns(&prices);
        let volatility = compute_ewma_volatility(&log_returns, 0.94);
        let (cum_buy_volume, cum_sell_volume) = compute_cumulative_volumes(&volumes, &trade_sides);
        
        Self {
            prices,
            timestamps,
            log_returns,
            volatility,
            trade_sides,
            volumes,
            cum_buy_volume,
            cum_sell_volume,
        }
    }
}

/// Compute log returns from prices (vectorized)
#[inline]
fn compute_log_returns(prices: &[f64]) -> Vec<f64> {
    if prices.len() < 2 {
        return vec![0.0; prices.len()];
    }
    
    let mut returns = Vec::with_capacity(prices.len());
    returns.push(0.0); // First return is 0
    
    for i in 1..prices.len() {
        let ret = if prices[i - 1] > 0.0 {
            (prices[i] / prices[i - 1]).ln()
        } else {
            0.0
        };
        returns.push(ret);
    }
    
    returns
}

/// Compute EWMA volatility (vectorized, annualized)
/// lambda = 0.94 is RiskMetrics standard
#[inline]
fn compute_ewma_volatility(returns: &[f64], lambda: f64) -> Vec<f64> {
    let n = returns.len();
    if n == 0 {
        return vec![];
    }
    
    let mut variance = Vec::with_capacity(n);
    let mut ewma_var = 0.0001; // Initial variance (10 bps)
    let mut ewma_mean = 0.0;
    
    // Annualization factor (assuming ~1ms between ticks, 252 trading days)
    // sqrt(252 * 24 * 60 * 60 * 1000) ≈ 147,580
    let annualization = 147580.0_f64.sqrt();
    
    // Volatility floor to prevent near-zero values during calm periods
    let vol_floor = 0.15; // 15% annualized minimum
    
    for &ret in returns {
        // EWMA update
        let deviation = ret - ewma_mean;
        ewma_mean = lambda * ewma_mean + (1.0 - lambda) * ret;
        ewma_var = lambda * ewma_var + (1.0 - lambda) * deviation * deviation;
        
        // Convert to annualized volatility with floor
        let vol = (ewma_var.sqrt() * annualization).max(vol_floor);
        variance.push(vol);
    }
    
    variance
}

/// Compute cumulative buy/sell volumes for order flow analysis
#[inline]
fn compute_cumulative_volumes(volumes: &[f64], sides: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = volumes.len();
    let mut cum_buy = Vec::with_capacity(n);
    let mut cum_sell = Vec::with_capacity(n);
    
    let mut total_buy = 0.0;
    let mut total_sell = 0.0;
    
    for i in 0..n {
        if sides[i] > 0.0 {
            total_buy += volumes[i];
        } else if sides[i] < 0.0 {
            total_sell += volumes[i];
        }
        cum_buy.push(total_buy);
        cum_sell.push(total_sell);
    }
    
    (cum_buy, cum_sell)
}

/// Avellaneda-Stoikov parameters for fast simulation
#[derive(Debug, Clone)]
pub struct FastASParams {
    pub risk_aversion: f64,
    pub inventory_target: f64,
    pub order_size_pct: f64,
    pub volatility_cap: f64,
    pub min_spread_bps: f64,
    pub time_horizon_minutes: f64,
    pub fee_rate: f64,
}

impl Default for FastASParams {
    fn default() -> Self {
        Self {
            risk_aversion: 0.5,
            inventory_target: 0.0,
            order_size_pct: 0.02,
            volatility_cap: 1.0,
            min_spread_bps: 10.0,
            time_horizon_minutes: 60.0,
            fee_rate: 0.0002, // 2 bps maker fee
        }
    }
}

/// Fast simulation result
#[derive(Debug, Clone)]
pub struct FastSimResult {
    pub total_pnl: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub num_trades: usize,
    pub profit_factor: f64,
    pub equity_curve: Vec<f64>,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub total_commission: f64,
}

impl Default for FastSimResult {
    fn default() -> Self {
        Self {
            total_pnl: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            win_rate: 0.0,
            num_trades: 0,
            profit_factor: 1.0,
            equity_curve: vec![],
            gross_profit: 0.0,
            gross_loss: 0.0,
            total_commission: 0.0,
        }
    }
}

/// Fast vectorized simulator for GA fitness evaluation
pub struct FastSimulator<'a> {
    features: &'a MarketFeatures,
    initial_capital: f64,
}

impl<'a> FastSimulator<'a> {
    /// Create a new fast simulator with pre-computed features
    pub fn new(features: &'a MarketFeatures, initial_capital: f64) -> Self {
        Self {
            features,
            initial_capital,
        }
    }
    
    /// Evaluate a parameter set and return simulation results
    /// This is the hot path - must be as fast as possible
    pub fn evaluate(&self, params: &FastASParams) -> FastSimResult {
        let n = self.features.len();
        if n < 100 {
            return FastSimResult::default();
        }
        
        // Pre-allocate result vectors
        let mut equity_curve = Vec::with_capacity(n / 100 + 1);
        let mut trade_pnls = Vec::with_capacity(n / 10);
        
        // State variables
        let mut cash = self.initial_capital;
        let mut inventory = 0.0; // Base asset position
        let mut total_commission = 0.0;
        let mut peak_equity = self.initial_capital;
        let mut max_drawdown = 0.0;
        
        // Calculate order size in base units
        let ref_price = self.features.prices[0];
        let order_size = (self.initial_capital * params.order_size_pct) / ref_price;
        
        // Time horizon in milliseconds
        let time_horizon_ms = params.time_horizon_minutes * 60.0 * 1000.0;
        
        // Quote tracking
        let mut last_bid_price = 0.0;
        let mut last_ask_price = 0.0;
        let mut last_quote_time: i64 = 0;
        let min_quote_lifetime_ms: i64 = 100;
        
        // Sample equity curve every N points for efficiency
        let sample_interval = (n / 500).max(1);
        
        for i in 0..n {
            let price = self.features.prices[i];
            let vol = self.features.volatility[i];
            let timestamp = self.features.timestamps[i];
            let trade_side = self.features.trade_sides[i];
            
            // Skip if volatility too high
            if vol > params.volatility_cap {
                continue;
            }
            
            // Calculate inventory skew (deviation from target)
            let inventory_value = inventory * price;
            let total_equity = cash + inventory_value;
            let inventory_pct = if total_equity > 0.0 { inventory_value / total_equity } else { 0.0 };
            let inventory_skew = inventory_pct - params.inventory_target;
            
            // Avellaneda-Stoikov optimal spread calculation
            // spread = gamma * sigma^2 * T + (2/gamma) * ln(1 + gamma/k)
            // Simplified: spread ≈ risk_aversion * vol^2 * T + base_spread
            let gamma = params.risk_aversion;
            let sigma2 = vol * vol;
            let t = time_horizon_ms / (24.0 * 60.0 * 60.0 * 1000.0); // Fraction of day
            
            let vol_component = gamma * sigma2 * t;
            let liquidity_component = (params.min_spread_bps / 10000.0) * price;
            let inventory_adjustment = inventory_skew * gamma * price * 0.001; // Skew quotes away from inventory
            
            let half_spread = (vol_component * price + liquidity_component).max(params.min_spread_bps / 10000.0 * price);
            
            // Quote prices with inventory adjustment
            let bid_price = price - half_spread - inventory_adjustment;
            let ask_price = price + half_spread - inventory_adjustment;
            
            // Update quotes if enough time passed
            if timestamp - last_quote_time > min_quote_lifetime_ms {
                last_bid_price = bid_price;
                last_ask_price = ask_price;
                last_quote_time = timestamp;
            }
            
            // Check for fills based on trade aggressor side
            // If a buy aggressor (trade_side > 0) hits our ask, we sell
            // If a sell aggressor (trade_side < 0) hits our bid, we buy
            
            if trade_side > 0.0 && last_ask_price > 0.0 && price >= last_ask_price {
                // Our ask got hit - we sell
                let fill_price = last_ask_price;
                let fill_size = order_size.min(inventory.max(0.0) + order_size); // Can go short
                
                if fill_size > 0.0 {
                    let trade_value = fill_size * fill_price;
                    let commission = trade_value * params.fee_rate;
                    
                    cash += trade_value - commission;
                    inventory -= fill_size;
                    total_commission += commission;
                    
                    // Track P&L for this trade
                    let trade_pnl = (fill_price - price) * fill_size - commission;
                    trade_pnls.push(trade_pnl);
                    
                    // Reset quote
                    last_ask_price = 0.0;
                }
            }
            
            if trade_side < 0.0 && last_bid_price > 0.0 && price <= last_bid_price {
                // Our bid got hit - we buy
                let fill_price = last_bid_price;
                let max_buy = cash / fill_price * 0.95; // Keep some cash buffer
                let fill_size = order_size.min(max_buy);
                
                if fill_size > 0.0 {
                    let trade_value = fill_size * fill_price;
                    let commission = trade_value * params.fee_rate;
                    
                    cash -= trade_value + commission;
                    inventory += fill_size;
                    total_commission += commission;
                    
                    // Track P&L for this trade
                    let trade_pnl = (price - fill_price) * fill_size - commission;
                    trade_pnls.push(trade_pnl);
                    
                    // Reset quote
                    last_bid_price = 0.0;
                }
            }
            
            // Update equity and drawdown
            let current_equity = cash + inventory * price;
            if current_equity > peak_equity {
                peak_equity = current_equity;
            }
            let drawdown = (peak_equity - current_equity) / peak_equity;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
            
            // Sample equity curve
            if i % sample_interval == 0 {
                equity_curve.push(current_equity);
            }
            
            // Early stopping if equity drops too much
            if current_equity < self.initial_capital * 0.5 {
                break;
            }
        }
        
        // Final equity
        let final_price = self.features.prices[n - 1];
        let final_equity = cash + inventory * final_price;
        equity_curve.push(final_equity);
        
        // Calculate metrics
        let total_pnl = final_equity - self.initial_capital;
        let num_trades = trade_pnls.len();
        
        let (gross_profit, gross_loss) = trade_pnls.iter().fold((0.0, 0.0), |(profit, loss), &pnl| {
            if pnl > 0.0 {
                (profit + pnl, loss)
            } else {
                (profit, loss + pnl.abs())
            }
        });
        
        let profit_factor = if gross_loss > 0.0 { gross_profit / gross_loss } else { gross_profit.max(1.0) };
        
        let winning_trades = trade_pnls.iter().filter(|&&pnl| pnl > 0.0).count();
        let win_rate = if num_trades > 0 { winning_trades as f64 / num_trades as f64 } else { 0.0 };
        
        // Calculate Sharpe ratio from equity curve returns
        let sharpe_ratio = calculate_sharpe_from_equity(&equity_curve, self.initial_capital);
        
        FastSimResult {
            total_pnl,
            sharpe_ratio,
            max_drawdown,
            win_rate,
            num_trades,
            profit_factor,
            equity_curve,
            gross_profit,
            gross_loss,
            total_commission,
        }
    }
}

/// Calculate Sharpe ratio from equity curve
fn calculate_sharpe_from_equity(equity: &[f64], _initial_capital: f64) -> f64 {
    if equity.len() < 2 {
        return 0.0;
    }
    
    // Calculate returns
    let mut returns = Vec::with_capacity(equity.len() - 1);
    for i in 1..equity.len() {
        if equity[i - 1] > 0.0 {
            returns.push((equity[i] - equity[i - 1]) / equity[i - 1]);
        }
    }
    
    if returns.is_empty() {
        return 0.0;
    }
    
    // Mean and std of returns
    let mean: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std = variance.sqrt();
    
    if std < 1e-10 {
        return 0.0;
    }
    
    // Annualize (assuming ~500 samples = ~1 day of data for sampled curve)
    let annualization = (252.0 * equity.len() as f64 / 500.0).sqrt();
    
    (mean / std) * annualization
}

/// Convert FastSimResult to standard BacktestResult for compatibility
impl From<FastSimResult> for crate::types::BacktestResult {
    fn from(fast: FastSimResult) -> Self {
        use bigdecimal::FromPrimitive;
        
        let mut result = crate::types::BacktestResult::default();
        result.total_pnl = bigdecimal::BigDecimal::from_f64(fast.total_pnl).unwrap_or_default();
        result.sharpe_ratio = Some(fast.sharpe_ratio);
        result.max_drawdown = bigdecimal::BigDecimal::from_f64(fast.max_drawdown * 100.0).unwrap_or_default(); // Store as percentage
        result.win_rate = Some(fast.win_rate);
        result.num_trades = fast.num_trades;
        result.profit_factor = Some(fast.profit_factor);
        result.equity_curve = fast.equity_curve;
        result.gross_profit = Some(fast.gross_profit);
        result.gross_loss = Some(fast.gross_loss);
        result.transaction_costs = Some(crate::types::TransactionCostAnalysis {
            total_commission: fast.total_commission,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            total_transaction_costs: fast.total_commission,
            costs_as_percentage_of_pnl: if fast.total_pnl != 0.0 { 
                (fast.total_commission / fast.total_pnl.abs()) * 100.0 
            } else { 
                0.0 
            },
            avg_cost_per_trade: if fast.num_trades > 0 { 
                fast.total_commission / fast.num_trades as f64 
            } else { 
                0.0 
            },
            commission_by_exchange: Default::default(),
        });
        result.net_profit = Some(fast.total_pnl);
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ewma_volatility() {
        let returns = vec![0.001, -0.002, 0.0015, -0.001, 0.002];
        let vol = compute_ewma_volatility(&returns, 0.94);
        
        assert_eq!(vol.len(), returns.len());
        // All volatilities should be positive
        assert!(vol.iter().all(|&v| v > 0.0));
        // Should have volatility floor
        assert!(vol.iter().all(|&v| v >= 0.15));
    }
    
    #[test]
    fn test_log_returns() {
        let prices = vec![100.0, 101.0, 100.5, 102.0];
        let returns = compute_log_returns(&prices);
        
        assert_eq!(returns.len(), prices.len());
        assert_eq!(returns[0], 0.0); // First return is 0
        assert!((returns[1] - (101.0_f64 / 100.0).ln()).abs() < 1e-10);
    }
    
    #[test]
    fn test_sharpe_calculation() {
        // Steadily increasing equity should have positive Sharpe
        let equity = vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0];
        let sharpe = calculate_sharpe_from_equity(&equity, 100.0);
        assert!(sharpe > 0.0);
        
        // Flat equity should have ~0 Sharpe
        let flat = vec![100.0, 100.0, 100.0, 100.0];
        let sharpe_flat = calculate_sharpe_from_equity(&flat, 100.0);
        assert!(sharpe_flat.abs() < 0.1);
    }
}
