//! Portfolio-Level Risk Management and Multi-Asset Correlation
//!
//! This module provides institutional-grade portfolio risk analytics:
//! - Multi-asset correlation tracking
//! - Portfolio-level VaR and Expected Shortfall
//! - Correlation breakdown detection
//! - Concentration risk metrics
//! - Cross-asset hedging effectiveness

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rolling correlation window configuration
const DEFAULT_CORRELATION_WINDOW: usize = 60; // 60 periods for correlation
const MIN_PERIODS_FOR_CORRELATION: usize = 20;

/// Portfolio position for a single asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPosition {
    pub symbol: String,
    pub quantity: f64,
    pub entry_price: f64,
    pub current_price: f64,
    pub unrealized_pnl: f64,
    pub notional_value: f64,
}

/// Correlation matrix entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationEntry {
    pub asset1: String,
    pub asset2: String,
    pub correlation: f64,
    pub num_observations: usize,
    pub is_reliable: bool, // true if enough observations
}

/// Portfolio-level risk metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioRiskMetrics {
    /// Total portfolio notional value
    pub total_notional: f64,
    /// Net portfolio delta (directional exposure)
    pub net_delta: f64,
    /// Gross exposure (sum of absolute positions)
    pub gross_exposure: f64,
    /// Portfolio-level VaR (95%)
    pub portfolio_var_95: f64,
    /// Portfolio-level Expected Shortfall (95%)
    pub portfolio_es_95: f64,
    /// Diversification ratio (undiversified VaR / diversified VaR)
    pub diversification_ratio: f64,
    /// Herfindahl-Hirschman Index for concentration
    pub concentration_hhi: f64,
    /// Maximum single-asset exposure as % of portfolio
    pub max_single_exposure_pct: f64,
    /// Correlation stress: portfolio VaR if all correlations go to 1
    pub stressed_var_95: f64,
    /// Beta to reference asset (e.g., BTC)
    pub portfolio_beta: f64,
}

/// Asset return series for correlation calculation
#[derive(Debug, Clone)]
pub struct AssetReturns {
    pub symbol: String,
    pub returns: Vec<f64>,
    pub timestamps: Vec<i64>,
}

/// Multi-asset portfolio risk manager
pub struct PortfolioRiskManager {
    /// Asset positions
    positions: HashMap<String, AssetPosition>,
    /// Historical returns by asset
    returns_history: HashMap<String, AssetReturns>,
    /// Rolling correlation matrix
    correlation_matrix: Vec<CorrelationEntry>,
    /// Correlation window size
    correlation_window: usize,
    /// Reference asset for beta calculation
    reference_asset: String,
    /// Individual asset volatilities (annualized)
    asset_volatilities: HashMap<String, f64>,
}

impl PortfolioRiskManager {
    pub fn new(reference_asset: &str) -> Self {
        Self {
            positions: HashMap::new(),
            returns_history: HashMap::new(),
            correlation_matrix: Vec::new(),
            correlation_window: DEFAULT_CORRELATION_WINDOW,
            reference_asset: reference_asset.to_string(),
            asset_volatilities: HashMap::new(),
        }
    }
    
    /// Update position for an asset
    pub fn update_position(&mut self, symbol: &str, quantity: f64, entry_price: f64, current_price: f64) {
        let unrealized_pnl = quantity * (current_price - entry_price);
        let notional_value = quantity.abs() * current_price;
        
        self.positions.insert(symbol.to_string(), AssetPosition {
            symbol: symbol.to_string(),
            quantity,
            entry_price,
            current_price,
            unrealized_pnl,
            notional_value,
        });
    }
    
    /// Add a return observation for an asset
    pub fn add_return(&mut self, symbol: &str, return_value: f64, timestamp: i64) {
        let entry = self.returns_history.entry(symbol.to_string()).or_insert_with(|| {
            AssetReturns {
                symbol: symbol.to_string(),
                returns: Vec::new(),
                timestamps: Vec::new(),
            }
        });
        
        entry.returns.push(return_value);
        entry.timestamps.push(timestamp);
        
        // Keep only the correlation window size
        while entry.returns.len() > self.correlation_window * 2 {
            entry.returns.remove(0);
            entry.timestamps.remove(0);
        }
    }
    
    /// Calculate correlation between two assets
    pub fn calculate_correlation(&self, asset1: &str, asset2: &str) -> Option<f64> {
        let returns1 = self.returns_history.get(asset1)?;
        let returns2 = self.returns_history.get(asset2)?;
        
        // Align timestamps and calculate correlation
        let n = returns1.returns.len().min(returns2.returns.len());
        if n < MIN_PERIODS_FOR_CORRELATION {
            return None;
        }
        
        let r1: Vec<f64> = returns1.returns.iter().rev().take(n).cloned().collect();
        let r2: Vec<f64> = returns2.returns.iter().rev().take(n).cloned().collect();
        
        let mean1: f64 = r1.iter().sum::<f64>() / n as f64;
        let mean2: f64 = r2.iter().sum::<f64>() / n as f64;
        
        let mut cov = 0.0;
        let mut var1 = 0.0;
        let mut var2 = 0.0;
        
        for i in 0..n {
            let d1 = r1[i] - mean1;
            let d2 = r2[i] - mean2;
            cov += d1 * d2;
            var1 += d1 * d1;
            var2 += d2 * d2;
        }
        
        let denom = (var1 * var2).sqrt();
        if denom > 1e-10 {
            Some(cov / denom)
        } else {
            None
        }
    }
    
    /// Update the full correlation matrix
    pub fn update_correlation_matrix(&mut self) {
        self.correlation_matrix.clear();
        
        let symbols: Vec<String> = self.returns_history.keys().cloned().collect();
        
        for i in 0..symbols.len() {
            for j in (i + 1)..symbols.len() {
                let asset1 = &symbols[i];
                let asset2 = &symbols[j];
                
                let correlation = self.calculate_correlation(asset1, asset2);
                let n = self.returns_history.get(asset1)
                    .map(|r| r.returns.len())
                    .unwrap_or(0)
                    .min(
                        self.returns_history.get(asset2)
                            .map(|r| r.returns.len())
                            .unwrap_or(0)
                    );
                
                self.correlation_matrix.push(CorrelationEntry {
                    asset1: asset1.clone(),
                    asset2: asset2.clone(),
                    correlation: correlation.unwrap_or(0.0),
                    num_observations: n,
                    is_reliable: n >= MIN_PERIODS_FOR_CORRELATION,
                });
            }
        }
        
        // Update individual volatilities
        for (symbol, returns) in &self.returns_history {
            if returns.returns.len() >= MIN_PERIODS_FOR_CORRELATION {
                let vol = calculate_volatility(&returns.returns) * (252.0_f64).sqrt(); // Annualize
                self.asset_volatilities.insert(symbol.clone(), vol);
            }
        }
    }
    
    /// Calculate portfolio-level risk metrics
    pub fn calculate_risk_metrics(&self) -> PortfolioRiskMetrics {
        let positions: Vec<&AssetPosition> = self.positions.values().collect();
        
        if positions.is_empty() {
            return PortfolioRiskMetrics::default();
        }
        
        // Basic aggregations
        let total_notional: f64 = positions.iter().map(|p| p.notional_value).sum();
        let net_delta: f64 = positions.iter().map(|p| p.quantity * p.current_price).sum();
        let gross_exposure: f64 = positions.iter().map(|p| p.notional_value).sum();
        
        // Concentration (HHI)
        let weights: Vec<f64> = positions.iter()
            .map(|p| p.notional_value / total_notional.max(1.0))
            .collect();
        let concentration_hhi: f64 = weights.iter().map(|w| w * w).sum();
        let max_single_exposure_pct: f64 = weights.iter().cloned().fold(0.0, f64::max) * 100.0;
        
        // Individual VaRs (95%, 1-day)
        let z_95 = 1.645;
        let mut individual_vars: Vec<f64> = Vec::new();
        
        for pos in &positions {
            let vol = self.asset_volatilities.get(&pos.symbol).cloned().unwrap_or(0.20);
            let daily_vol = vol / (252.0_f64).sqrt();
            let var = pos.notional_value * daily_vol * z_95;
            individual_vars.push(var);
        }
        
        let undiversified_var: f64 = individual_vars.iter().sum();
        
        // Portfolio VaR with correlations (simplified parametric)
        // For full accuracy, would need covariance matrix eigendecomposition
        let portfolio_var_95 = self.calculate_portfolio_var(&positions, &individual_vars, z_95);
        
        let diversification_ratio = if portfolio_var_95 > 0.0 {
            undiversified_var / portfolio_var_95
        } else {
            1.0
        };
        
        // Stressed VaR (all correlations = 1)
        let stressed_var_95 = undiversified_var; // This IS the stressed case
        
        // ES approximation (assumes normal distribution, ES ≈ 1.18 * VaR for 95%)
        let portfolio_es_95 = portfolio_var_95 * 1.18;
        
        // Portfolio beta to reference asset
        let portfolio_beta = self.calculate_portfolio_beta(&positions);
        
        PortfolioRiskMetrics {
            total_notional,
            net_delta,
            gross_exposure,
            portfolio_var_95,
            portfolio_es_95,
            diversification_ratio,
            concentration_hhi,
            max_single_exposure_pct,
            stressed_var_95,
            portfolio_beta,
        }
    }
    
    /// Calculate portfolio VaR accounting for correlations
    fn calculate_portfolio_var(&self, positions: &[&AssetPosition], individual_vars: &[f64], z: f64) -> f64 {
        let n = positions.len();
        if n == 0 {
            return 0.0;
        }
        if n == 1 {
            return individual_vars.get(0).cloned().unwrap_or(0.0);
        }
        
        // Build correlation matrix for positions
        let mut variance = 0.0;
        
        for i in 0..n {
            for j in 0..n {
                let corr = if i == j {
                    1.0
                } else {
                    // Find correlation between positions[i] and positions[j]
                    self.get_correlation(&positions[i].symbol, &positions[j].symbol)
                        .unwrap_or(0.5) // Default 0.5 correlation for unknown pairs
                };
                
                let vi = individual_vars.get(i).cloned().unwrap_or(0.0) / z; // Convert back to std dev
                let vj = individual_vars.get(j).cloned().unwrap_or(0.0) / z;
                
                variance += vi * vj * corr;
            }
        }
        
        variance.sqrt() * z
    }
    
    /// Get correlation between two assets from cached matrix
    fn get_correlation(&self, asset1: &str, asset2: &str) -> Option<f64> {
        for entry in &self.correlation_matrix {
            if (entry.asset1 == asset1 && entry.asset2 == asset2) ||
               (entry.asset1 == asset2 && entry.asset2 == asset1) {
                if entry.is_reliable {
                    return Some(entry.correlation);
                }
            }
        }
        None
    }
    
    /// Calculate portfolio beta to reference asset
    fn calculate_portfolio_beta(&self, positions: &[&AssetPosition]) -> f64 {
        let total_notional: f64 = positions.iter().map(|p| p.notional_value).sum();
        if total_notional <= 0.0 {
            return 0.0;
        }
        
        let mut weighted_beta = 0.0;
        
        for pos in positions {
            let weight = pos.notional_value / total_notional;
            
            // Calculate beta = correlation * (vol_asset / vol_reference)
            let corr = self.get_correlation(&pos.symbol, &self.reference_asset).unwrap_or(1.0);
            let vol_asset = self.asset_volatilities.get(&pos.symbol).cloned().unwrap_or(0.20);
            let vol_ref = self.asset_volatilities.get(&self.reference_asset).cloned().unwrap_or(0.20);
            
            let beta = if vol_ref > 0.0 {
                corr * (vol_asset / vol_ref)
            } else {
                1.0
            };
            
            weighted_beta += weight * beta;
        }
        
        weighted_beta
    }
    
    /// Get the current correlation matrix
    pub fn get_correlation_matrix(&self) -> &[CorrelationEntry] {
        &self.correlation_matrix
    }
    
    /// Detect correlation breakdown (sudden increase in correlations)
    pub fn detect_correlation_stress(&self, threshold: f64) -> Vec<String> {
        let mut stressed_pairs = Vec::new();
        
        for entry in &self.correlation_matrix {
            if entry.is_reliable && entry.correlation > threshold {
                stressed_pairs.push(format!("{}/{}: {:.3}", entry.asset1, entry.asset2, entry.correlation));
            }
        }
        
        stressed_pairs
    }
}

impl Default for PortfolioRiskMetrics {
    fn default() -> Self {
        Self {
            total_notional: 0.0,
            net_delta: 0.0,
            gross_exposure: 0.0,
            portfolio_var_95: 0.0,
            portfolio_es_95: 0.0,
            diversification_ratio: 1.0,
            concentration_hhi: 0.0,
            max_single_exposure_pct: 0.0,
            stressed_var_95: 0.0,
            portfolio_beta: 0.0,
        }
    }
}

/// Calculate volatility from returns
fn calculate_volatility(returns: &[f64]) -> f64 {
    if returns.len() < 2 {
        return 0.0;
    }
    
    let n = returns.len();
    let mean: f64 = returns.iter().sum::<f64>() / n as f64;
    let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_correlation_calculation() {
        let mut manager = PortfolioRiskManager::new("BTC/USD");
        
        // Add perfectly correlated returns
        for i in 0..30 {
            let r = (i as f64) * 0.01;
            manager.add_return("BTC/USD", r, i as i64);
            manager.add_return("ETH/USD", r * 1.5, i as i64); // Scaled but same direction
        }
        
        manager.update_correlation_matrix();
        
        let corr = manager.calculate_correlation("BTC/USD", "ETH/USD");
        assert!(corr.is_some());
        assert!((corr.unwrap() - 1.0).abs() < 0.01); // Should be ~1.0
    }
    
    #[test]
    fn test_portfolio_concentration() {
        let mut manager = PortfolioRiskManager::new("BTC/USD");
        
        // Add concentrated position
        manager.update_position("BTC/USD", 1.0, 50000.0, 50000.0);
        manager.update_position("ETH/USD", 0.1, 3000.0, 3000.0);
        
        let metrics = manager.calculate_risk_metrics();
        
        assert!(metrics.concentration_hhi > 0.8); // Highly concentrated
        assert!(metrics.max_single_exposure_pct > 90.0);
    }
}
