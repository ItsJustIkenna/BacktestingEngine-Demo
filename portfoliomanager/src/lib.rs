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
    /// Apply multiple fills to the portfolio
    pub fn apply_fills(&mut self, fills: &[Fill]) -> Result<()> {
        for fill in fills {
            self.apply_fill(fill)?;
        }
        Ok(())
    }

    /// Apply a single fill to the portfolio with fee calculation
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<()> {
        // Update timestamp
        self.timestamp = fill.timestamp;
        
        // Calculate trade value
        let trade_value = &fill.price * &fill.quantity;
        
        // Apply fee
        self.fees_paid += &fill.fee_amount;
        
        // Update balance based on side and fees
        match fill.side {
            BookSide::Bid => {
                // Buying: reduce balance by trade value + fees
                self.balance -= &trade_value;
                self.balance -= &fill.fee_amount;
                self.net_position += fill.quantity.to_f64().unwrap_or(0.0);
            }
            BookSide::Ask => {
                // Selling: increase balance by trade value - fees
                self.balance += &trade_value;
                self.balance -= &fill.fee_amount; // Fees are always subtracted
                self.net_position -= fill.quantity.to_f64().unwrap_or(0.0);
            }
        }
        
        // Create or update position
        self.update_position(fill)?;
        
        Ok(())
    }

    /// Update or create a position based on the fill
    fn update_position(&mut self, fill: &Fill) -> Result<()> {
        let symbol = "BTC/USD".to_string(); // Simplified - in practice would be from fill or order
        
        // Find existing position or create new one
        let position_index = self.positions.iter().position(|p| p.symbol == symbol && p.close_time.is_none());
        
        match position_index {
            Some(index) => {
                // Update existing position
                let position = &mut self.positions[index];
                let fill_value = &fill.price * &fill.quantity;
                let quantity_change = fill.quantity.to_f64().unwrap_or(0.0);
                
                match fill.side {
                    BookSide::Bid if position.side == PositionSide::Long => {
                        // Adding to long position
                        let new_quantity = position.quantity + quantity_change;
                        let total_cost = &position.entry_price * BigDecimal::from_f64(position.quantity).unwrap_or_default() + &fill_value;
                        position.entry_price = &total_cost / BigDecimal::from_f64(new_quantity).unwrap_or(BigDecimal::from(1));
                        position.quantity = new_quantity;
                    }
                    BookSide::Ask if position.side == PositionSide::Long => {
                        // Reducing long position
                        let pnl = (&fill.price - &position.entry_price) * &fill.quantity;
                        position.realized_pnl += &pnl;
                        self.realized_pnl += &pnl;
                        position.quantity -= quantity_change;
                        
                        if position.quantity <= 0.0 {
                            position.close_time = Some(fill.timestamp);
                        }
                    }
                    _ => {
                        // Handle other cases (short positions, etc.)
                        // Simplified for now
                    }
                }
            }
            None => {
                // Create new position
                let side = match fill.side {
                    BookSide::Bid => PositionSide::Long,
                    BookSide::Ask => PositionSide::Short,
                };
                
                let position = Position {
                    symbol,
                    side,
                    quantity: fill.quantity.to_f64().unwrap_or(0.0),
                    entry_price: fill.price.clone(),
                    mark_price: Some(fill.price.clone()),
                    realized_pnl: BigDecimal::from(0),
                    unrealized_pnl: BigDecimal::from(0),
                    open_time: fill.timestamp,
                    close_time: None,
                };
                
                self.positions.push(position);
            }
        }
        
        Ok(())
    }

    /// Get total portfolio value including unrealized P&L
    pub fn get_total_value(&self) -> BigDecimal {
        &self.balance + &self.unrealized_pnl
    }

    /// Update unrealized P&L for all open positions based on current market price
    pub fn update_unrealized_pnl(&mut self, current_price: &BigDecimal) {
        let mut total_unrealized = BigDecimal::from(0);
        
        for position in &mut self.positions {
            if position.close_time.is_none() {
                position.mark_price = Some(current_price.clone());
                
                let pnl = match position.side {
                    PositionSide::Long => {
                        (current_price - &position.entry_price) * BigDecimal::from_f64(position.quantity).unwrap_or_default()
                    }
                    PositionSide::Short => {
                        (&position.entry_price - current_price) * BigDecimal::from_f64(position.quantity).unwrap_or_default()
                    }
                };
                
                position.unrealized_pnl = pnl.clone();
                total_unrealized += pnl;
            }
        }
        
        self.unrealized_pnl = total_unrealized;
    }
}
