//! Portfolio Manager Module
//!
//! Provides portfolio state tracking, position management, and P&L calculation for trading systems.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};
use orderbook::{Fill, BookSide};
use anyhow::Result;

/// Represents the state of the trading portfolio at a point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioState {
    pub timestamp: DateTime<Utc>,
    pub balance: BigDecimal,           // Total account balance (quote currency)
    pub net_position: f64,             // Net position (base asset, e.g. BTC)
    pub positions: Vec<Position>,      // Open positions
    pub realized_pnl: BigDecimal,      // Realized profit and loss
    pub unrealized_pnl: BigDecimal,    // Unrealized profit and loss
    pub fees_paid: BigDecimal,         // Total fees paid
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            balance: BigDecimal::from(0),
            net_position: 0.0,
            positions: Vec::new(),
            realized_pnl: BigDecimal::from(0),
            unrealized_pnl: BigDecimal::from(0),
            fees_paid: BigDecimal::from(0),
        }
    }
}

/// Represents an open or closed position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,                // Trading pair (e.g. BTC/USDT)
    pub side: PositionSide,            // Long or Short
    pub quantity: f64,                 // Position size (base asset)
    pub entry_price: BigDecimal,       // Entry price
    pub mark_price: Option<BigDecimal>,// Current mark price
    pub realized_pnl: BigDecimal,      // Realized P&L for this position
    pub unrealized_pnl: BigDecimal,    // Unrealized P&L for this position
    pub open_time: DateTime<Utc>,
    pub close_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionSide {
    Long,
    Short,
}

impl PortfolioState {
    /// Create a new portfolio with an initial balance
    pub fn with_balance(balance: BigDecimal) -> Self {
        Self {
            balance,
            ..Default::default()
        }
    }

    /// Apply multiple fills to the portfolio
    pub fn apply_fills(&mut self, fills: &[Fill]) -> Result<()> {
        for fill in fills {
            self.apply_fill(fill)?;
        }
        Ok(())
    }

    /// Get the available quantity for a given symbol and side
    /// For sells: returns the quantity of Long positions we can sell
    /// For buys: returns max quantity affordable with current balance
    fn get_available_quantity(&self, symbol: &str, side: &BookSide, price: &BigDecimal) -> f64 {
        match side {
            BookSide::Ask => {
                // For selling, we can only sell what we have in Long positions
                self.positions.iter()
                    .filter(|p| p.symbol == symbol && p.close_time.is_none() && p.side == PositionSide::Long)
                    .map(|p| p.quantity.max(0.0)) // Ensure no negative quantities leak through
                    .sum()
            }
            BookSide::Bid => {
                // For buying, limited by available balance
                // Calculate max quantity we can afford: balance / price
                let price_f64 = price.to_f64().unwrap_or(1.0);
                if price_f64 <= 0.0 {
                    return 0.0;
                }
                let balance_f64 = self.balance.to_f64().unwrap_or(0.0);
                if balance_f64 <= 0.0 {
                    return 0.0; // No balance, can't buy
                }
                // Allow buying up to 95% of balance to leave room for fees
                (balance_f64 * 0.95) / price_f64
            }
        }
    }

    /// Apply a single fill to the portfolio with proper validation
    /// 
    /// This method ensures:
    /// 1. You cannot sell more than you own (prevents negative Long positions)
    /// 2. Balance and position updates are atomic and consistent
    /// 3. Fees are proportionally adjusted if quantity is reduced
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<()> {
        let symbol = "BTC/USD".to_string(); // TODO: Get from fill/order in production
        
        // Update timestamp
        self.timestamp = fill.timestamp;
        
        let requested_quantity = fill.quantity.to_f64().unwrap_or(0.0);
        if requested_quantity <= 0.0 {
            return Ok(()); // Nothing to do
        }
        
        // Validate and cap the execution quantity based on:
        // - For sells: available position quantity
        // - For buys: available balance
        let available = self.get_available_quantity(&symbol, &fill.side, &fill.price);
        let executed_quantity = requested_quantity.min(available);
        
        // Skip if we can't execute anything meaningful
        if executed_quantity < 1e-10 {
            return Ok(());
        }
        
        // Calculate proportional adjustments if we're executing less than requested
        let execution_ratio = executed_quantity / requested_quantity;
        let executed_qty_bd = BigDecimal::from_f64(executed_quantity).unwrap_or_default();
        let trade_value = &fill.price * &executed_qty_bd;
        let fee_amount = &fill.fee_amount * BigDecimal::from_f64(execution_ratio).unwrap_or_default();
        
        // Apply fee
        self.fees_paid += &fee_amount;
        
        // Update balance based on trade side
        match fill.side {
            BookSide::Bid => {
                // Buying: reduce balance by trade value + fees
                self.balance -= &trade_value;
                self.balance -= &fee_amount;
                self.net_position += executed_quantity;
                
                // Update or create Long position
                self.add_to_long_position(&symbol, executed_quantity, &fill.price, fill.timestamp);
            }
            BookSide::Ask => {
                // Selling: increase balance by trade value - fees
                self.balance += &trade_value;
                self.balance -= &fee_amount;
                self.net_position -= executed_quantity;
                
                // Reduce Long position and realize P&L
                let realized = self.reduce_long_position(&symbol, executed_quantity, &fill.price, fill.timestamp);
                self.realized_pnl += &realized;
            }
        }
        
        Ok(())
    }

    /// Add to an existing Long position or create a new one
    /// Uses weighted average for entry price when adding to existing position
    fn add_to_long_position(
        &mut self, 
        symbol: &str, 
        quantity: f64, 
        price: &BigDecimal, 
        timestamp: DateTime<Utc>
    ) {
        // Find existing open Long position
        let existing = self.positions.iter_mut()
            .find(|p| p.symbol == symbol && p.close_time.is_none() && p.side == PositionSide::Long);
        
        match existing {
            Some(position) => {
                // Update with weighted average entry price
                let old_value = &position.entry_price * BigDecimal::from_f64(position.quantity).unwrap_or_default();
                let new_value = price * BigDecimal::from_f64(quantity).unwrap_or_default();
                let new_quantity = position.quantity + quantity;
                
                if new_quantity > 0.0 {
                    position.entry_price = (old_value + new_value) / BigDecimal::from_f64(new_quantity).unwrap_or(BigDecimal::from(1));
                }
                position.quantity = new_quantity;
                position.mark_price = Some(price.clone());
            }
            None => {
                // Create new Long position
                self.positions.push(Position {
                    symbol: symbol.to_string(),
                    side: PositionSide::Long,
                    quantity,
                    entry_price: price.clone(),
                    mark_price: Some(price.clone()),
                    realized_pnl: BigDecimal::from(0),
                    unrealized_pnl: BigDecimal::from(0),
                    open_time: timestamp,
                    close_time: None,
                });
            }
        }
    }

    /// Reduce a Long position by selling
    /// Returns the realized P&L from the sale
    /// 
    /// INVARIANT: quantity must be <= available position quantity
    /// This is enforced by apply_fill() before calling this method
    fn reduce_long_position(
        &mut self, 
        symbol: &str, 
        quantity: f64, 
        price: &BigDecimal, 
        timestamp: DateTime<Utc>
    ) -> BigDecimal {
        let mut remaining_to_sell = quantity;
        let mut total_realized_pnl = BigDecimal::from(0);
        
        // Process positions in order (FIFO)
        for position in self.positions.iter_mut()
            .filter(|p| p.symbol == symbol && p.close_time.is_none() && p.side == PositionSide::Long) 
        {
            if remaining_to_sell <= 0.0 {
                break;
            }
            
            // How much can we sell from this position?
            let sell_from_this = remaining_to_sell.min(position.quantity);
            
            if sell_from_this > 0.0 {
                // Calculate realized P&L: (sell_price - entry_price) * quantity
                let sell_qty_bd = BigDecimal::from_f64(sell_from_this).unwrap_or_default();
                let pnl = (price - &position.entry_price) * &sell_qty_bd;
                
                position.realized_pnl += &pnl;
                total_realized_pnl += &pnl;
                
                position.quantity -= sell_from_this;
                remaining_to_sell -= sell_from_this;
                
                // Close position if fully sold (use epsilon for float comparison)
                if position.quantity < 1e-10 {
                    position.quantity = 0.0;
                    position.close_time = Some(timestamp);
                }
            }
        }
        
        total_realized_pnl
    }

    /// Get total portfolio value including the market value of all open positions
    /// Total Value = Cash Balance + Sum(current_price * quantity) for all open positions
    /// This correctly reflects the portfolio's worth at current market prices
    pub fn get_total_value(&self) -> BigDecimal {
        let mut position_value = BigDecimal::from(0);
        
        for position in &self.positions {
            if position.close_time.is_none() && position.quantity > 0.0 {
                // Use mark_price if available, otherwise fall back to entry_price
                let price = position.mark_price.as_ref().unwrap_or(&position.entry_price);
                let qty = BigDecimal::from_f64(position.quantity).unwrap_or_default();
                
                match position.side {
                    PositionSide::Long => {
                        // Long position: we own the asset, add its value
                        position_value += price * &qty;
                    }
                    PositionSide::Short => {
                        // Short position: we owe the asset, subtract its value
                        // Note: For a market maker that only goes long, this may not apply
                        position_value -= price * &qty;
                    }
                }
            }
        }
        
        &self.balance + position_value
    }

    /// Update unrealized P&L for all open positions based on current market price
    pub fn update_unrealized_pnl(&mut self, current_price: &BigDecimal) {
        let mut total_unrealized = BigDecimal::from(0);
        
        for position in &mut self.positions {
            if position.close_time.is_none() && position.quantity > 0.0 {
                position.mark_price = Some(current_price.clone());
                
                let qty = BigDecimal::from_f64(position.quantity).unwrap_or_default();
                let pnl = match position.side {
                    PositionSide::Long => {
                        (current_price - &position.entry_price) * &qty
                    }
                    PositionSide::Short => {
                        (&position.entry_price - current_price) * &qty
                    }
                };
                
                position.unrealized_pnl = pnl.clone();
                total_unrealized += pnl;
            }
        }
        
        self.unrealized_pnl = total_unrealized;
    }

    /// Get a summary of open positions for debugging
    pub fn get_position_summary(&self) -> String {
        let open_positions: Vec<_> = self.positions.iter()
            .filter(|p| p.close_time.is_none())
            .collect();
        
        if open_positions.is_empty() {
            return "No open positions".to_string();
        }
        
        open_positions.iter()
            .map(|p| format!("{} {:?} qty={:.8} entry={}", p.symbol, p.side, p.quantity, p.entry_price))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orderbook::LiquidityType;

    fn create_fill(side: BookSide, price: f64, quantity: f64) -> Fill {
        Fill {
            order_id: "test".to_string(),
            side,
            price: BigDecimal::from_f64(price).unwrap(),
            base_price: None,
            quantity: BigDecimal::from_f64(quantity).unwrap(),
            liquidity_type: LiquidityType::Maker,
            fee_rate: 0.001,
            fee_amount: BigDecimal::from_f64(price * quantity * 0.001).unwrap(),
            slippage_bps: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_cannot_sell_more_than_owned() {
        let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100_000));
        
        // Buy 1 BTC at $50,000 (well within $100k balance)
        let buy_fill = create_fill(BookSide::Bid, 50000.0, 1.0);
        portfolio.apply_fill(&buy_fill).unwrap();
        
        // Should have bought ~1 BTC (might be slightly less due to 95% balance limit)
        assert!(portfolio.net_position > 0.9, "Should have bought close to 1 BTC, got {}", portfolio.net_position);
        
        // Try to sell 2 BTC (should only sell what we have)
        let sell_fill = create_fill(BookSide::Ask, 51000.0, 2.0);
        portfolio.apply_fill(&sell_fill).unwrap();
        
        // Should have sold only what we had
        assert!(portfolio.net_position.abs() < 1e-10, "Net position should be ~0, got {}", portfolio.net_position);
        
        // Position should be closed, not negative
        let open_positions: Vec<_> = portfolio.positions.iter()
            .filter(|p| p.close_time.is_none())
            .collect();
        assert!(open_positions.is_empty(), "Should have no open positions");
        
        // No position should have negative quantity
        for pos in &portfolio.positions {
            assert!(pos.quantity >= 0.0, "Position quantity should never be negative: {}", pos.quantity);
        }
    }

    #[test]
    fn test_sell_without_position_is_noop() {
        let mut portfolio = PortfolioState::with_balance(BigDecimal::from(100_000));
        let initial_balance = portfolio.balance.clone();
        
        // Try to sell without owning anything
        let sell_fill = create_fill(BookSide::Ask, 50000.0, 1.0);
        portfolio.apply_fill(&sell_fill).unwrap();
        
        // Balance should be unchanged (no phantom sells)
        assert_eq!(portfolio.balance, initial_balance);
        assert_eq!(portfolio.net_position, 0.0);
    }

    #[test]
    fn test_buy_and_partial_sell() {
        let mut portfolio = PortfolioState::with_balance(BigDecimal::from(200_000));
        
        // Buy 2 BTC at $50,000 (=$100k, within $200k balance)
        let buy_fill = create_fill(BookSide::Bid, 50000.0, 2.0);
        portfolio.apply_fill(&buy_fill).unwrap();
        
        assert!((portfolio.net_position - 2.0).abs() < 0.1, "Should have ~2 BTC, got {}", portfolio.net_position);
        
        // Sell 1 BTC at $51,000
        let sell_fill = create_fill(BookSide::Ask, 51000.0, 1.0);
        portfolio.apply_fill(&sell_fill).unwrap();
        
        // Should still have ~1 BTC
        assert!((portfolio.net_position - 1.0).abs() < 0.1, "Should have ~1 BTC, got {}", portfolio.net_position);
        
        // Realized P&L should be positive (sold higher than bought)
        assert!(portfolio.realized_pnl > BigDecimal::from(0));
    }

    #[test]
    fn test_cannot_buy_more_than_balance_allows() {
        let mut portfolio = PortfolioState::with_balance(BigDecimal::from(10_000));
        
        // Try to buy 1 BTC at $50,000 (but only have $10k)
        let buy_fill = create_fill(BookSide::Bid, 50000.0, 1.0);
        portfolio.apply_fill(&buy_fill).unwrap();
        
        // Should only buy what we can afford: $10k * 0.95 / $50k = 0.19 BTC
        assert!(portfolio.net_position < 0.2, "Should only buy ~0.19 BTC with $10k, got {}", portfolio.net_position);
        assert!(portfolio.net_position > 0.1, "Should have bought some BTC, got {}", portfolio.net_position);
        
        // Balance should never go negative
        assert!(portfolio.balance >= BigDecimal::from(0), "Balance should never be negative: {}", portfolio.balance);
    }

    #[test]
    fn test_balance_never_goes_negative() {
        let mut portfolio = PortfolioState::with_balance(BigDecimal::from(10_000));
        
        // Try to buy massive amounts repeatedly
        for _ in 0..10 {
            let buy_fill = create_fill(BookSide::Bid, 90000.0, 100.0); // Try to buy $9M worth
            portfolio.apply_fill(&buy_fill).unwrap();
        }
        
        // Balance should never go negative
        assert!(portfolio.balance.to_f64().unwrap_or(-1.0) >= -0.01, 
            "Balance should never be significantly negative: {}", portfolio.balance);
        
        // Position should be limited by what we could afford
        assert!(portfolio.net_position < 0.2, 
            "Position should be limited to what $10k can buy at $90k/BTC: {}", portfolio.net_position);
    }
}
