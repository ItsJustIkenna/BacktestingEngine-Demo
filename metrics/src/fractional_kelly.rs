//! Fractional Kelly Criterion with Drawdown Constraints
//!
//! Provides sophisticated position sizing that balances growth optimization
//! with risk management. Unlike basic Kelly, this implementation:
//! 
//! 1. **Fractional Kelly**: Uses a fraction (typically 0.25-0.5) of full Kelly
//!    to reduce volatility while maintaining positive expected growth
//! 
//! 2. **Drawdown Constraints**: Dynamically reduces position size as drawdown
//!    increases, preventing catastrophic losses
//! 
//! 3. **Regime-Aware Sizing**: Adjusts Kelly fraction based on market regime
//! 
//! 4. **Risk of Ruin Protection**: Ensures position sizes never exceed
//!    levels that could lead to unrecoverable losses

use serde::{Deserialize, Serialize};

/// Configuration for fractional Kelly with drawdown constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FractionalKellyConfig {
    /// Base Kelly fraction (0.0 - 1.0, typically 0.25-0.5)
    /// 0.5 = Half Kelly (recommended), 0.25 = Quarter Kelly (conservative)
    pub base_kelly_fraction: f64,
    
    /// Maximum position size as fraction of capital (hard cap)
    pub max_position_fraction: f64,
    
    /// Minimum position size (don't trade if Kelly suggests less)
    pub min_position_fraction: f64,
    
    // ========== Drawdown Constraints ==========
    
    /// Drawdown level at which to start reducing position (e.g., 0.05 = 5%)
    pub drawdown_reduction_start: f64,
    
    /// Drawdown level at which position is reduced to minimum (e.g., 0.15 = 15%)
    pub drawdown_reduction_full: f64,
    
    /// Drawdown level at which trading stops entirely (e.g., 0.20 = 20%)
    pub drawdown_stop_trading: f64,
    
    /// How aggressively to reduce position with drawdown
    /// Linear = proportional, Convex = faster reduction, Concave = slower
    pub drawdown_reduction_curve: ReductionCurve,
    
    // ========== Volatility Adjustments ==========
    
    /// Whether to reduce position in high volatility regimes
    pub volatility_adjustment_enabled: bool,
    
    /// Volatility threshold (as multiple of baseline) for reduction
    pub high_volatility_threshold: f64,
    
    /// Kelly fraction multiplier in high volatility (e.g., 0.5 = halve position)
    pub high_volatility_kelly_mult: f64,
    
    /// Volatility threshold for extreme regime
    pub extreme_volatility_threshold: f64,
    
    /// Kelly fraction multiplier in extreme volatility
    pub extreme_volatility_kelly_mult: f64,
    
    // ========== Risk of Ruin Protection ==========
    
    /// Target probability of ruin (e.g., 0.01 = 1% chance of 50% drawdown)
    pub target_ruin_probability: f64,
    
    /// Ruin threshold (drawdown level considered "ruin")
    pub ruin_threshold: f64,
    
    // ========== Growth Optimization ==========
    
    /// Minimum edge required to trade (expected value per trade)
    pub min_edge_to_trade: f64,
    
    /// Whether to use geometric (log) growth optimization
    pub use_geometric_growth: bool,
}

impl Default for FractionalKellyConfig {
    fn default() -> Self {
        Self {
            base_kelly_fraction: 0.5,        // Half Kelly (standard recommendation)
            max_position_fraction: 0.25,      // Never risk more than 25%
            min_position_fraction: 0.01,      // Minimum 1% to be worth trading
            
            drawdown_reduction_start: 0.05,   // Start reducing at 5% DD
            drawdown_reduction_full: 0.15,    // Full reduction at 15% DD
            drawdown_stop_trading: 0.20,      // Stop at 20% DD
            drawdown_reduction_curve: ReductionCurve::Convex,
            
            volatility_adjustment_enabled: true,
            high_volatility_threshold: 1.5,   // 50% above baseline
            high_volatility_kelly_mult: 0.5,  // Halve in high vol
            extreme_volatility_threshold: 2.5, // 150% above baseline
            extreme_volatility_kelly_mult: 0.2, // 80% reduction in extreme vol
            
            target_ruin_probability: 0.01,    // 1% chance of ruin
            ruin_threshold: 0.50,             // 50% drawdown = ruin
            
            min_edge_to_trade: 0.0001,        // 1 bps minimum edge
            use_geometric_growth: true,
        }
    }
}

/// Curve shape for drawdown reduction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReductionCurve {
    /// Linear reduction
    Linear,
    /// Faster reduction as drawdown increases (more conservative)
    Convex,
    /// Slower reduction initially, faster later
    Concave,
    /// Step function at midpoint
    Step,
}

/// Result of Kelly position sizing calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KellyPositionResult {
    /// Final recommended position size as fraction of capital
    pub position_fraction: f64,
    
    /// Full Kelly position (before any adjustments)
    pub full_kelly: f64,
    
    /// Kelly after applying base fraction
    pub fractional_kelly: f64,
    
    /// Position after drawdown adjustment
    pub drawdown_adjusted: f64,
    
    /// Position after volatility adjustment
    pub volatility_adjusted: f64,
    
    /// Position after risk of ruin check
    pub ruin_adjusted: f64,
    
    /// Whether position was capped by max limit
    pub was_capped: bool,
    
    /// Whether trading is disabled due to drawdown
    pub trading_disabled: bool,
    
    /// Whether edge is too low to trade
    pub insufficient_edge: bool,
    
    /// Adjustment factors applied
    pub adjustments: PositionAdjustments,
    
    /// Expected geometric growth rate at this position
    pub expected_growth_rate: f64,
    
    /// Risk assessment
    pub risk_assessment: RiskAssessment,
}

/// Individual adjustment factors
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionAdjustments {
    pub base_fraction: f64,
    pub drawdown_multiplier: f64,
    pub volatility_multiplier: f64,
    pub ruin_multiplier: f64,
}

/// Risk assessment for the position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Estimated probability of hitting stop loss
    pub prob_stop_loss: f64,
    /// Estimated probability of max drawdown
    pub prob_max_drawdown: f64,
    /// Value at Risk (95%) as fraction of position
    pub var_95: f64,
    /// Expected Shortfall (CVaR) at 95%
    pub cvar_95: f64,
    /// Risk level classification
    pub risk_level: RiskLevel,
}

/// Risk level classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Moderate,
    Elevated,
    High,
    Extreme,
}

/// Fractional Kelly calculator with drawdown constraints
pub struct FractionalKellyCalculator {
    config: FractionalKellyConfig,
    /// Baseline volatility for regime detection
    baseline_volatility: f64,
    /// Current drawdown (0.0 to 1.0)
    current_drawdown: f64,
    /// Current volatility
    current_volatility: f64,
    /// Peak equity (for drawdown calculation)
    peak_equity: f64,
    /// Current equity
    current_equity: f64,
}

impl FractionalKellyCalculator {
    pub fn new(config: FractionalKellyConfig) -> Self {
        Self {
            config,
            baseline_volatility: 0.01, // Default 1% daily vol
            current_drawdown: 0.0,
            current_volatility: 0.01,
            peak_equity: 100000.0,
            current_equity: 100000.0,
        }
    }
    
    /// Update equity and calculate current drawdown
    pub fn update_equity(&mut self, equity: f64) {
        self.current_equity = equity;
        if equity > self.peak_equity {
            self.peak_equity = equity;
        }
        self.current_drawdown = (self.peak_equity - equity) / self.peak_equity;
    }
    
    /// Update volatility estimate
    pub fn update_volatility(&mut self, volatility: f64, baseline: Option<f64>) {
        self.current_volatility = volatility;
        if let Some(b) = baseline {
            self.baseline_volatility = b;
        }
    }
    
    /// Calculate optimal position size given trade statistics
    /// 
    /// # Arguments
    /// * `win_probability` - Historical probability of winning trade
    /// * `avg_win` - Average win as fraction (e.g., 0.02 = 2%)
    /// * `avg_loss` - Average loss as fraction (absolute value)
    pub fn calculate_position(
        &self,
        win_probability: f64,
        avg_win: f64,
        avg_loss: f64,
    ) -> KellyPositionResult {
        // Edge validation
        let edge = win_probability * avg_win - (1.0 - win_probability) * avg_loss;
        
        if edge < self.config.min_edge_to_trade {
            return KellyPositionResult {
                position_fraction: 0.0,
                full_kelly: 0.0,
                fractional_kelly: 0.0,
                drawdown_adjusted: 0.0,
                volatility_adjusted: 0.0,
                ruin_adjusted: 0.0,
                was_capped: false,
                trading_disabled: false,
                insufficient_edge: true,
                adjustments: PositionAdjustments::default(),
                expected_growth_rate: 0.0,
                risk_assessment: self.assess_risk(0.0, avg_loss),
            };
        }
        
        // Check if trading disabled due to drawdown
        if self.current_drawdown >= self.config.drawdown_stop_trading {
            return KellyPositionResult {
                position_fraction: 0.0,
                full_kelly: 0.0,
                fractional_kelly: 0.0,
                drawdown_adjusted: 0.0,
                volatility_adjusted: 0.0,
                ruin_adjusted: 0.0,
                was_capped: false,
                trading_disabled: true,
                insufficient_edge: false,
                adjustments: PositionAdjustments::default(),
                expected_growth_rate: 0.0,
                risk_assessment: self.assess_risk(0.0, avg_loss),
            };
        }
        
        // Step 1: Calculate full Kelly
        // f* = (p * b - q) / b where b = avg_win / avg_loss
        let b = if avg_loss > 0.0 { avg_win / avg_loss } else { 1.0 };
        let p = win_probability;
        let q = 1.0 - p;
        
        let full_kelly = if b > 0.0 {
            ((p * b - q) / b).max(0.0)
        } else {
            0.0
        };
        
        // Step 2: Apply base Kelly fraction
        let fractional_kelly = full_kelly * self.config.base_kelly_fraction;
        
        // Step 3: Apply drawdown reduction
        let drawdown_multiplier = self.calculate_drawdown_multiplier();
        let drawdown_adjusted = fractional_kelly * drawdown_multiplier;
        
        // Step 4: Apply volatility adjustment
        let volatility_multiplier = self.calculate_volatility_multiplier();
        let volatility_adjusted = drawdown_adjusted * volatility_multiplier;
        
        // Step 5: Apply risk of ruin constraint
        let ruin_multiplier = self.calculate_ruin_multiplier(avg_loss);
        let ruin_adjusted = volatility_adjusted * ruin_multiplier;
        
        // Step 6: Apply caps
        let mut final_position = ruin_adjusted;
        let mut was_capped = false;
        
        if final_position > self.config.max_position_fraction {
            final_position = self.config.max_position_fraction;
            was_capped = true;
        }
        
        if final_position < self.config.min_position_fraction {
            final_position = 0.0; // Don't trade if below minimum
        }
        
        // Calculate expected growth rate
        let expected_growth_rate = self.calculate_growth_rate(
            final_position, p, b
        );
        
        KellyPositionResult {
            position_fraction: final_position,
            full_kelly,
            fractional_kelly,
            drawdown_adjusted,
            volatility_adjusted,
            ruin_adjusted,
            was_capped,
            trading_disabled: false,
            insufficient_edge: false,
            adjustments: PositionAdjustments {
                base_fraction: self.config.base_kelly_fraction,
                drawdown_multiplier,
                volatility_multiplier,
                ruin_multiplier,
            },
            expected_growth_rate,
            risk_assessment: self.assess_risk(final_position, avg_loss),
        }
    }
    
    /// Calculate drawdown multiplier based on current drawdown
    fn calculate_drawdown_multiplier(&self) -> f64 {
        if self.current_drawdown < self.config.drawdown_reduction_start {
            return 1.0;
        }
        
        if self.current_drawdown >= self.config.drawdown_reduction_full {
            // Minimum position (but not zero unless stop trading)
            return 0.1;
        }
        
        // Calculate position in reduction range (0.0 to 1.0)
        let range = self.config.drawdown_reduction_full - self.config.drawdown_reduction_start;
        let position_in_range = (self.current_drawdown - self.config.drawdown_reduction_start) / range;
        
        // Apply curve shape
        let reduction = match self.config.drawdown_reduction_curve {
            ReductionCurve::Linear => position_in_range,
            ReductionCurve::Convex => position_in_range.powf(0.5), // Faster reduction
            ReductionCurve::Concave => position_in_range.powf(2.0), // Slower reduction
            ReductionCurve::Step => if position_in_range > 0.5 { 1.0 } else { 0.0 },
        };
        
        // Return multiplier (1.0 - reduction, but minimum 0.1)
        (1.0 - reduction * 0.9).max(0.1)
    }
    
    /// Calculate volatility multiplier
    fn calculate_volatility_multiplier(&self) -> f64 {
        if !self.config.volatility_adjustment_enabled {
            return 1.0;
        }
        
        let vol_ratio = if self.baseline_volatility > 0.0 {
            self.current_volatility / self.baseline_volatility
        } else {
            1.0
        };
        
        if vol_ratio >= self.config.extreme_volatility_threshold {
            self.config.extreme_volatility_kelly_mult
        } else if vol_ratio >= self.config.high_volatility_threshold {
            // Linear interpolation between high and extreme
            let t = (vol_ratio - self.config.high_volatility_threshold)
                / (self.config.extreme_volatility_threshold - self.config.high_volatility_threshold);
            self.config.high_volatility_kelly_mult
                + t * (self.config.extreme_volatility_kelly_mult - self.config.high_volatility_kelly_mult)
        } else {
            1.0
        }
    }
    
    /// Calculate multiplier to limit risk of ruin
    fn calculate_ruin_multiplier(&self, avg_loss: f64) -> f64 {
        // Based on risk of ruin formula for geometric random walk
        // P(ruin) ≈ (1 - edge/volatility)^(ruin_threshold * capital / position_size)
        
        // We want: P(ruin) < target_ruin_probability
        // This gives us a maximum position size
        
        // Simplified: position < capital * ln(target_ruin) / ln(1 - loss_rate) / ruin_threshold
        
        if avg_loss <= 0.0 {
            return 1.0;
        }
        
        let target_p = self.config.target_ruin_probability;
        let ruin_threshold = self.config.ruin_threshold;
        
        // Conservative estimate of maximum safe position
        // Using approximation: max_f ≈ -ln(target_p) * avg_loss / ruin_threshold
        let max_safe_position = (-target_p.ln()) * avg_loss / ruin_threshold;
        
        // This is a hard cap, so if Kelly exceeds it, we reduce
        // We apply as a multiplier that could reduce but never increase
        1.0_f64.min(max_safe_position / self.config.max_position_fraction)
    }
    
    /// Calculate expected geometric growth rate
    fn calculate_growth_rate(&self, position: f64, win_prob: f64, win_loss_ratio: f64) -> f64 {
        if position <= 0.0 {
            return 0.0;
        }
        
        // g(f) = p * ln(1 + b*f) + q * ln(1 - f)
        // where f = position fraction, b = win/loss ratio
        let p = win_prob;
        let q = 1.0 - p;
        let b = win_loss_ratio;
        
        let win_term = if 1.0 + b * position > 0.0 {
            p * (1.0 + b * position).ln()
        } else {
            f64::NEG_INFINITY
        };
        
        let loss_term = if 1.0 - position > 0.0 {
            q * (1.0 - position).ln()
        } else {
            f64::NEG_INFINITY
        };
        
        win_term + loss_term
    }
    
    /// Assess risk for given position
    fn assess_risk(&self, position: f64, avg_loss: f64) -> RiskAssessment {
        let potential_loss = position * avg_loss;
        
        // VaR estimate (assuming normal, scale by 1.65 for 95%)
        let var_95 = potential_loss * 1.65;
        
        // CVaR estimate (expected shortfall beyond VaR)
        let cvar_95 = var_95 * 1.3; // Rough approximation
        
        // Risk level based on position size
        let risk_level = if position <= 0.05 {
            RiskLevel::Low
        } else if position <= 0.10 {
            RiskLevel::Moderate
        } else if position <= 0.15 {
            RiskLevel::Elevated
        } else if position <= 0.25 {
            RiskLevel::High
        } else {
            RiskLevel::Extreme
        };
        
        // Probability estimates (rough)
        let prob_stop_loss = (position * 10.0).min(0.5); // Rough estimate
        let prob_max_drawdown = (self.current_drawdown * 2.0 + position * 5.0).min(0.3);
        
        RiskAssessment {
            prob_stop_loss,
            prob_max_drawdown,
            var_95,
            cvar_95,
            risk_level,
        }
    }
    
    /// Get current drawdown
    pub fn current_drawdown(&self) -> f64 {
        self.current_drawdown
    }
    
    /// Reset peak equity (e.g., after confirmed recovery)
    pub fn reset_peak(&mut self) {
        self.peak_equity = self.current_equity;
        self.current_drawdown = 0.0;
    }
}

/// Convenience function to calculate Kelly position
pub fn calculate_fractional_kelly(
    win_probability: f64,
    avg_win: f64,
    avg_loss: f64,
    current_drawdown: f64,
    current_volatility: f64,
    baseline_volatility: f64,
    config: Option<FractionalKellyConfig>,
) -> KellyPositionResult {
    let config = config.unwrap_or_default();
    let mut calculator = FractionalKellyCalculator::new(config);
    
    calculator.current_drawdown = current_drawdown;
    calculator.update_volatility(current_volatility, Some(baseline_volatility));
    
    calculator.calculate_position(win_probability, avg_win, avg_loss)
}

/// Optimal Kelly fraction finder that maximizes growth subject to constraints
pub fn find_optimal_kelly_fraction(
    win_probability: f64,
    avg_win: f64,
    avg_loss: f64,
    max_drawdown_target: f64,
    max_volatility_target: f64,
) -> OptimalFractionResult {
    // Search for fraction that maximizes growth while meeting constraints
    let mut best_fraction = 0.0;
    let mut best_growth = f64::NEG_INFINITY;
    let mut best_meets_constraints = false;
    
    let b = if avg_loss > 0.0 { avg_win / avg_loss } else { 1.0 };
    let p = win_probability;
    let q = 1.0 - p;
    
    // Full Kelly
    let full_kelly = if b > 0.0 { ((p * b - q) / b).max(0.0) } else { 0.0 };
    
    // Search fractions from 0.1 to 1.0 of full Kelly
    for fraction_pct in 1..=100 {
        let fraction = fraction_pct as f64 / 100.0;
        let position = full_kelly * fraction;
        
        if position <= 0.0 || position >= 1.0 {
            continue;
        }
        
        // Calculate growth rate
        let growth = p * (1.0 + b * position).ln() + q * (1.0 - position).ln();
        
        // Estimate drawdown (simplified)
        // Expected max drawdown scales with position volatility
        let position_vol = position * (p * q).sqrt() * (1.0 + b);
        let estimated_max_dd = position_vol * 2.0; // Rough 2-sigma
        
        // Estimated return volatility
        let estimated_vol = position * avg_loss.max(avg_win);
        
        let meets_dd = estimated_max_dd <= max_drawdown_target;
        let meets_vol = estimated_vol <= max_volatility_target;
        let meets_constraints = meets_dd && meets_vol;
        
        if meets_constraints && growth > best_growth {
            best_fraction = fraction;
            best_growth = growth;
            best_meets_constraints = true;
        } else if !best_meets_constraints && growth > best_growth {
            best_fraction = fraction;
            best_growth = growth;
        }
    }
    
    OptimalFractionResult {
        optimal_fraction: best_fraction,
        kelly_position: full_kelly * best_fraction,
        expected_growth_rate: best_growth,
        meets_constraints: best_meets_constraints,
        full_kelly,
    }
}

/// Result of optimal fraction search
#[derive(Debug, Clone)]
pub struct OptimalFractionResult {
    pub optimal_fraction: f64,
    pub kelly_position: f64,
    pub expected_growth_rate: f64,
    pub meets_constraints: bool,
    pub full_kelly: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_kelly_calculation() {
        let config = FractionalKellyConfig::default();
        let calculator = FractionalKellyCalculator::new(config);
        
        // 60% win rate, 2% avg win, 1% avg loss
        let result = calculator.calculate_position(0.6, 0.02, 0.01);
        
        assert!(result.position_fraction > 0.0, "Should have positive position");
        assert!(result.full_kelly > result.fractional_kelly, "Full Kelly > Fractional");
        assert!(!result.trading_disabled);
        assert!(!result.insufficient_edge);
    }
    
    #[test]
    fn test_drawdown_reduction() {
        let config = FractionalKellyConfig::default();
        let mut calculator = FractionalKellyCalculator::new(config);
        
        // No drawdown
        calculator.update_equity(100000.0);
        let result_no_dd = calculator.calculate_position(0.6, 0.02, 0.01);
        
        // 10% drawdown
        calculator.update_equity(90000.0);
        let result_with_dd = calculator.calculate_position(0.6, 0.02, 0.01);
        
        assert!(result_with_dd.position_fraction < result_no_dd.position_fraction,
            "Position should be reduced in drawdown");
        assert!(result_with_dd.adjustments.drawdown_multiplier < 1.0,
            "Drawdown multiplier should be < 1");
    }
    
    #[test]
    fn test_trading_disabled_at_max_drawdown() {
        let config = FractionalKellyConfig::default();
        let mut calculator = FractionalKellyCalculator::new(config);
        
        // 25% drawdown (above stop threshold)
        calculator.peak_equity = 100000.0;
        calculator.update_equity(75000.0);
        
        let result = calculator.calculate_position(0.6, 0.02, 0.01);
        
        assert!(result.trading_disabled, "Trading should be disabled at max drawdown");
        assert_eq!(result.position_fraction, 0.0);
    }
    
    #[test]
    fn test_volatility_reduction() {
        let config = FractionalKellyConfig::default();
        let mut calculator = FractionalKellyCalculator::new(config);
        
        // Normal volatility
        calculator.update_volatility(0.01, Some(0.01));
        let result_normal = calculator.calculate_position(0.6, 0.02, 0.01);
        
        // High volatility (2x baseline)
        calculator.update_volatility(0.02, Some(0.01));
        let result_high_vol = calculator.calculate_position(0.6, 0.02, 0.01);
        
        assert!(result_high_vol.position_fraction < result_normal.position_fraction,
            "Position should be reduced in high volatility");
    }
    
    #[test]
    fn test_insufficient_edge() {
        let config = FractionalKellyConfig::default();
        let calculator = FractionalKellyCalculator::new(config);
        
        // Negative edge (50% win rate, equal win/loss)
        let result = calculator.calculate_position(0.5, 0.01, 0.01);
        
        assert!(result.position_fraction == 0.0 || result.insufficient_edge,
            "Should not trade with no edge");
    }
    
    #[test]
    fn test_position_capping() {
        let config = FractionalKellyConfig {
            max_position_fraction: 0.10, // 10% max
            base_kelly_fraction: 1.0,     // Full Kelly
            ..Default::default()
        };
        let calculator = FractionalKellyCalculator::new(config);
        
        // Very favorable odds that would suggest large Kelly
        let result = calculator.calculate_position(0.8, 0.05, 0.01);
        
        assert!(result.position_fraction <= 0.10, "Position should be capped at max");
        assert!(result.was_capped, "Should indicate position was capped");
    }
    
    #[test]
    fn test_optimal_fraction_finder() {
        let result = find_optimal_kelly_fraction(
            0.6,    // 60% win rate
            0.02,   // 2% avg win
            0.01,   // 1% avg loss
            0.15,   // 15% max drawdown target
            0.05,   // 5% max volatility target
        );
        
        assert!(result.optimal_fraction > 0.0 && result.optimal_fraction <= 1.0);
        assert!(result.kelly_position > 0.0);
    }
}
