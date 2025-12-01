//! SimulationLoop: Drives the backtest event loop
//!
//! Steps through market data, triggers strategies, matches orders, and updates state.

use strategy::{StrategyManager, StrategyContext};
use strategy::traits::{MarketSession, SessionType};
use orderbook::OrderBook;
#[cfg(feature = "postgres")]
use orderbook::{OrderBookEvent, BookSide};
use portfoliomanager::PortfolioState;
use riskmanager::RiskManager;
use dataloader::MarketData;
use crate::types::{BacktestEvent, BacktestResult, TransactionCostAnalysis, MarketImpactAnalysis, LiquidityWarning};
use crate::logging_facade::BACKTEST_LOGGER;
use metrics::{performance, risk, trade_stats};
use bigdecimal::{BigDecimal, ToPrimitive, FromPrimitive};
use rustc_hash::FxHashMap;

pub struct SimulationLoop {
    // Track transaction costs
    total_commission: f64,
    total_slippage: f64,
    total_market_impact: f64,
    commission_by_exchange: FxHashMap<String, f64>,
    // Track market impact
    trades_with_impact: usize,
    impact_costs_bps: Vec<f64>,
    liquidity_warnings: Vec<LiquidityWarning>,
    // Track 30-day rolling volume per exchange for fee tiers
    trade_history_by_exchange: FxHashMap<String, Vec<(chrono::DateTime<chrono::Utc>, f64)>>, // exchange -> (timestamp, volume_usd)
    current_30d_volume_by_exchange: FxHashMap<String, f64>,
}

impl SimulationLoop {
    pub fn new() -> Self {
        Self {
            total_commission: 0.0,
            total_slippage: 0.0,
            total_market_impact: 0.0,
            commission_by_exchange: FxHashMap::default(),
            trades_with_impact: 0,
            impact_costs_bps: Vec::with_capacity(1000),  // Pre-allocate for typical backtest
            liquidity_warnings: Vec::with_capacity(100),
            trade_history_by_exchange: FxHashMap::default(),
            current_30d_volume_by_exchange: FxHashMap::default(),
        }
    }
    
    /// Update 30-day rolling volume window for specific exchange
    fn update_30d_volume(&mut self, exchange: &str, current_time: chrono::DateTime<chrono::Utc>, trade_volume_usd: f64) {
        use chrono::Duration;
        
        // Get or create trade history for this exchange
        let trade_history = self.trade_history_by_exchange.entry(exchange.to_string()).or_insert_with(Vec::new);
        
        // Add new trade
        trade_history.push((current_time, trade_volume_usd));
        
        // Remove trades older than 30 days
        let cutoff_time = current_time - Duration::days(30);
        trade_history.retain(|(ts, _)| *ts >= cutoff_time);
        
        // Recalculate 30-day volume for this exchange
        let volume: f64 = trade_history.iter().map(|(_, vol)| vol).sum();
        self.current_30d_volume_by_exchange.insert(exchange.to_string(), volume);
    }
    
    /// Get applicable fee rate based on exchange's 30-day volume
    fn get_fee_rate(&self, config: &config::BacktestConfig, exchange: &str, is_maker: bool) -> f64 {
        if let Some(ref fee_config) = config.trading.exchange_fees {
            // Only use tiered fees if the exchange matches
            if fee_config.exchange.to_lowercase() == exchange.to_lowercase() {
                let volume = self.current_30d_volume_by_exchange.get(exchange).copied().unwrap_or(0.0);
                fee_config.get_fee_rate(volume, is_maker)
            } else {
                // Different exchange, use generic fees or create config for that exchange
                let exchange_config = config::ExchangeFeeConfig::for_exchange(exchange);
                let volume = self.current_30d_volume_by_exchange.get(exchange).copied().unwrap_or(0.0);
                exchange_config.get_fee_rate(volume, is_maker)
            }
        } else {
            // Fallback to simple commission rate
            config.trading.commission_rate
        }
    }
    
    /// Calculate realistic slippage based on order size and market conditions
    fn calculate_slippage(&self, order_size: f64, price: f64, config: &config::BacktestConfig) -> f64 {
        let slippage_bps = config.trading.slippage_bps;
        let slippage_ratio = slippage_bps / 10000.0; // Convert basis points to decimal
        price * order_size * slippage_ratio
    }
    
    /// Calculate market impact using configured model
    fn calculate_market_impact(&mut self, order_size: f64, price: f64, avg_volume: f64, config: &config::BacktestConfig, timestamp: chrono::DateTime<chrono::Utc>) -> f64 {
        let impact_config = &config.data.market_impact;
        
        if !impact_config.enabled || avg_volume == 0.0 {
            return 0.0;
        }
        
        let size_ratio = order_size / avg_volume;
        
        // Only apply impact if order is significant relative to volume
        if size_ratio < impact_config.min_impact_ratio {
            return 0.0;
        }
        
        let impact_bps = match impact_config.model {
            config::MarketImpactModel::None => 0.0,
            config::MarketImpactModel::Linear => {
                impact_config.linear_coefficient * size_ratio
            }
            config::MarketImpactModel::SquareRoot => {
                impact_config.sqrt_coefficient * size_ratio.sqrt()
            }
            config::MarketImpactModel::AlmgrenChriss => {
                // Simplified Almgren-Chriss: permanent + temporary impact
                let permanent = impact_config.linear_coefficient * 0.3 * size_ratio;
                let temporary = impact_config.sqrt_coefficient * size_ratio.sqrt();
                permanent + temporary
            }
        };
        
        self.trades_with_impact += 1;
        self.impact_costs_bps.push(impact_bps);
        
        // Check for liquidity warnings
        if size_ratio > 0.1 {
            self.liquidity_warnings.push(LiquidityWarning {
                timestamp,
                order_size,
                available_liquidity: avg_volume,
                liquidity_ratio: size_ratio,
                message: format!("Order size is {:.1}% of available volume - high market impact expected", size_ratio * 100.0),
            });
        }
        
        // Convert basis points to dollar cost
        let impact_cost = (impact_bps / 10000.0) * price * order_size;
        self.total_market_impact += impact_cost;
        
        impact_cost
    }

    pub async fn run(
        &mut self,
        strategy_manager: &mut StrategyManager,
        orderbook: &mut OrderBook,
        portfolio: &mut PortfolioState,
        _risk_manager: &mut RiskManager,
        market_data: Vec<MarketData>,
        config: &config::BacktestConfig,
    ) -> BacktestResult {
    let mut events: Vec<BacktestEvent> = Vec::with_capacity(market_data.len());
    let mut result = BacktestResult::default();

    // Collectors for metrics (pre-allocated)
    let mut trade_returns: Vec<f64> = Vec::with_capacity(100);
    let mut equity_curve: Vec<f64> = Vec::with_capacity(market_data.len() + 1);
    let mut trade_durations: Vec<f64> = Vec::with_capacity(100);

    // Start equity curve with initial capital
    equity_curve.push(config.trading.initial_capital);

    for data in market_data {
            // 1. Update orderbook with new market data if needed
            #[cfg(feature = "postgres")]
            match &data {
                MarketData::Snapshot(_snapshot) => {
                    // Convert snapshot to OrderBookEvent::Snapshot and apply
                    // (Assume conversion function exists)
                    // orderbook.apply_event(OrderBookEvent::Snapshot { ... })
                }
                MarketData::Order(order) => {
                    // Convert HistoricalOrder to OrderBookEvent and apply to orderbook
                    if let Some(event) = convert_historical_order_to_event(order) {
                        let _ = orderbook.apply_event(event);
                    }
                }
                MarketData::Trade(_trade) => {
                    // Convert trade to OrderBookEvent::Trade and apply
                    // orderbook.apply_event(OrderBookEvent::Trade { ... })
                }
                MarketData::Candle(_) => {
                    // Candles don't directly update orderbook
                }
            }
            
            #[cfg(not(feature = "postgres"))]
            match &data {
                MarketData::Trade(_trade) => {
                    // Convert trade to OrderBookEvent::Trade and apply
                    // orderbook.apply_event(OrderBookEvent::Trade { ... })
                }
                MarketData::Candle(_) => {
                    // Candles don't directly update orderbook
                }
            }

            // 2. Build strategy context (timestamp, portfolio, etc.)
            #[cfg(feature = "postgres")]
            let timestamp = match &data {
                MarketData::Snapshot(s) => s.timestamp,
                MarketData::Order(o) => o.timestamp,
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            
            #[cfg(not(feature = "postgres"))]
            let timestamp = match &data {
                MarketData::Candle(c) => c.timestamp,
                MarketData::Trade(t) => t.timestamp,
            };
            let context = StrategyContext {
                timestamp,
                portfolio_state: portfolio.clone(),
                price_history: Vec::new(), // Empty for simplicity, could be filled from historical data
                volume_history: Vec::new(), // Empty for simplicity 
                spread: {
                    // Calculate spread from orderbook best bid/ask
                    match (orderbook.best_bid(), orderbook.best_ask()) {
                        (Some(bid), Some(ask)) => Some(ask.clone() - bid.clone()),
                        _ => None,
                    }
                },
                market_session: MarketSession {
                    is_open: true,
                    session_type: SessionType::Regular,
                    next_open: None,
                    next_close: None,
                }, // Default session
            };

            // 3. Run all enabled strategies, collect signals
            let signals = strategy_manager.process_market_data(&data, &context).await.unwrap_or_default();
            for (_strategy_id, sigs) in signals {
                for signal in sigs {
                    // Extract order details for cost calculation
                    let order_price = signal.price.as_ref()
                        .and_then(|p| p.to_f64())
                        .unwrap_or(0.0);
                    let order_size = signal.quantity.as_ref()
                        .and_then(|q| q.to_f64())
                        .unwrap_or(0.0);
                    let exchange = orderbook.exchange.clone();
                    
                    // Calculate transaction costs
                    let slippage_cost = self.calculate_slippage(order_size, order_price, config);
                    self.total_slippage += slippage_cost;
                    
                    // Estimate market impact (using placeholder for avg_volume)
                    let avg_volume = 1000.0; // TODO: get from market data
                    let _impact_cost = self.calculate_market_impact(order_size, order_price, avg_volume, config, timestamp);
                    
                    // 4. Submit order to orderbook
                    let _ = orderbook.submit_order_from_signal(&signal, timestamp);

                    // 5. Simulate order matching/fills with proper fee calculation
                    let fills = orderbook.match_all_orders(timestamp);

                    // Track commission costs with dynamic fee rates per exchange
                    for fill in &fills {
                        let fill_price = fill.price.to_f64().unwrap_or(0.0);
                        let fill_quantity = fill.quantity.to_f64().unwrap_or(0.0);
                        let fill_value = fill_price * fill_quantity;
                        
                        // Determine if maker or taker based on original signal
                        // Limit orders (with price) = maker, market orders (no price) = taker
                        let is_maker = signal.price.is_some();
                        
                        // Get fee rate based on this exchange's 30-day volume
                        let fee_rate = self.get_fee_rate(config, &exchange, is_maker);
                        let commission = fill_value * fee_rate;
                        
                        // Update volume tracking for this exchange
                        self.update_30d_volume(&exchange, timestamp, fill_value);
                        
                        self.total_commission += commission;
                        *self.commission_by_exchange.entry(exchange.clone()).or_insert(0.0) += commission;
                    }

                    // 6. Update portfolio and risk manager with fills
                    if let Err(e) = portfolio.apply_fills(&fills) {
                        log_error!(BACKTEST_LOGGER, "Error applying fills: {}", e);
                    }
                    // risk_manager.update(&portfolio, &fills); // TODO: Implement this method in RiskManager

                    // 7. Record event
                    events.push(BacktestEvent::OrderSubmitted {
                        order_id: signal.id.clone(),
                        timestamp,
                    });
                }
            }

            // Collect equity value at each step
            equity_curve.push(portfolio.balance.to_f64().unwrap_or(0.0));
        }

        // Collect trade returns and durations from closed positions (do this after market data processing)
        for pos in &portfolio.positions {
            if let Some(close_time) = pos.close_time {
                // Only consider closed positions
                trade_returns.push(pos.realized_pnl.to_f64().unwrap_or(0.0));
                let duration = (close_time - pos.open_time).num_seconds() as f64;
                trade_durations.push(duration);
            }
        }

        // Compute metrics using collected data
        result.num_trades = performance::number_of_trades(&trade_returns);
        result.gross_profit = Some(performance::gross_profit(&trade_returns));
        result.gross_loss = Some(performance::gross_loss(&trade_returns));
        result.net_profit = Some(performance::net_profit(&trade_returns));
        result.avg_trade_return = performance::average_trade_return(&trade_returns);
        result.median_trade_return = performance::median_trade_return(&trade_returns);
        result.sharpe_ratio = performance::sharpe_ratio(&trade_returns, config.trading.risk_free_rate);
        result.profit_factor = performance::profit_factor(
            result.gross_profit.unwrap_or(0.0),
            result.gross_loss.unwrap_or(0.0),
        );
        result.win_rate = trade_stats::win_rate(&trade_returns);
        result.avg_trade_duration = trade_stats::average_trade_duration(&trade_durations);
        result.volatility = risk::volatility(&trade_returns);
        result.max_drawdown = risk::max_drawdown(&equity_curve)
            .map(|v| BigDecimal::from_f64(v).unwrap_or_default())
            .unwrap_or_default();
            
        // Calculate total P&L: ending portfolio value - initial capital
        let final_portfolio_value = portfolio.balance.clone() + 
            portfolio.positions.iter().map(|pos| pos.unrealized_pnl.clone()).sum::<BigDecimal>();
        let initial_capital_bd = BigDecimal::from_f64(config.trading.initial_capital).unwrap_or_default();
        result.total_pnl = final_portfolio_value - initial_capital_bd;

        // Include equity curve data for visualization
        result.equity_curve = equity_curve;
        result.initial_capital = config.trading.initial_capital;
        
        // Generate transaction cost analysis
        let total_transaction_costs = self.total_commission + self.total_slippage + self.total_market_impact;
        let costs_as_percentage = if result.total_pnl.to_f64().unwrap_or(0.0).abs() > 0.0 {
            (total_transaction_costs / result.total_pnl.to_f64().unwrap_or(1.0).abs()) * 100.0
        } else {
            0.0
        };
        
        result.transaction_costs = Some(TransactionCostAnalysis {
            total_commission: self.total_commission,
            total_slippage: self.total_slippage,
            total_market_impact: self.total_market_impact,
            total_transaction_costs,
            costs_as_percentage_of_pnl: costs_as_percentage,
            avg_cost_per_trade: if result.num_trades > 0 {
                total_transaction_costs / result.num_trades as f64
            } else {
                0.0
            },
            commission_by_exchange: self.commission_by_exchange.iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        });
        
        // Generate market impact analysis
        result.market_impact = Some(MarketImpactAnalysis {
            trades_with_impact: self.trades_with_impact,
            avg_impact_bps: if !self.impact_costs_bps.is_empty() {
                self.impact_costs_bps.iter().sum::<f64>() / self.impact_costs_bps.len() as f64
            } else {
                0.0
            },
            max_impact_bps: self.impact_costs_bps.iter().cloned().fold(0.0, f64::max),
            total_impact_cost: self.total_market_impact,
            liquidity_warnings: self.liquidity_warnings.clone(),
        });

        result
    }
}

/// Convert HistoricalOrder from database to OrderBookEvent
/// This allows us to reconstruct orderbook state during backtest simulation
#[cfg(feature = "postgres")]
fn convert_historical_order_to_event(order: &databaseschema::models::HistoricalOrder) -> Option<OrderBookEvent> {
    use chrono::Utc;
    
    let event_type = order.get_event_type();
    let order_id = order.get_order_id().to_string();
    let timestamp = *order.get_timestamp();
    
    match event_type {
        "NEW" => {
            // Parse side
            let side = match BookSide::from_str(order.get_side()) {
                Some(s) => s,
                None => {
                    BACKTEST_LOGGER.warn(&format!(
                        "Invalid side '{}' for order {}", 
                        order.get_side(), 
                        order_id
                    ));
                    return None;
                }
            };
            
            Some(OrderBookEvent::NewOrder {
                order_id,
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        "MODIFY" => {
            Some(OrderBookEvent::ModifyOrder {
                order_id,
                new_quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        "CANCEL" => {
            Some(OrderBookEvent::CancelOrder {
                order_id,
                timestamp,
            })
        }
        "TRADE" => {
            // Parse side for trade
            let side = match BookSide::from_str(order.get_side()) {
                Some(s) => s,
                None => {
                    BACKTEST_LOGGER.warn(&format!(
                        "Invalid side '{}' for trade order {}", 
                        order.get_side(), 
                        order_id
                    ));
                    return None;
                }
            };
            
            Some(OrderBookEvent::Trade {
                aggressor_order_id: None,  // Historical data may not have aggressor ID
                passive_order_id: Some(order_id),
                side,
                price: order.get_price_level().clone(),
                quantity: order.get_quantity().clone(),
                timestamp,
            })
        }
        _ => {
            BACKTEST_LOGGER.warn(&format!(
                "Unknown event type '{}' for order {}", 
                event_type, 
                order_id
            ));
            None
        }
    }
}
