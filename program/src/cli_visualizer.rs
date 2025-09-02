use backtest::BacktestResult;
use bigdecimal::ToPrimitive;
use super::config::VisualizationConfig;

pub struct CliVisualizer {
    config: VisualizationConfig,
}

impl CliVisualizer {
    pub fn new(config: VisualizationConfig) -> Self {
        Self { config }
    }
    
    pub fn with_default() -> Self {
        Self::new(VisualizationConfig::default())
    }
    
    pub fn display_enhanced_results(&self, result: &BacktestResult) {
        println!("\n🚀 ENHANCED BACKTEST RESULTS");
        println!("═══════════════════════════════════════════════════════════════════");
        
        // Core metrics display
        self.display_core_metrics(result);
        
        // Risk metrics
        self.display_risk_metrics(result);
        
        // Trade statistics
        self.display_trade_stats(result);
        
        // Equity curve visualization
        self.display_equity_curve(result);
        
        // Visual performance summary
        self.display_performance_summary(result);
        
        // Strategy recommendations
        self.display_recommendations(result);
        
        println!("═══════════════════════════════════════════════════════════════════");
    }
    
    fn display_core_metrics(&self, result: &BacktestResult) {
        println!("\n💰 PROFIT & LOSS:");
        let total_pnl = result.total_pnl.to_f64().unwrap_or(0.0);
        if total_pnl >= 0.0 {
            println!("   \x1b[32mTotal P&L: ${:.2}\x1b[0m", total_pnl); // Green for profit
        } else {
            println!("   \x1b[31mTotal P&L: ${:.2}\x1b[0m", total_pnl); // Red for loss
        }
        
        if let Some(net_profit) = result.net_profit {
            if net_profit >= 0.0 {
                println!("   \x1b[32mNet Profit: ${:.2}\x1b[0m", net_profit);
            } else {
                println!("   \x1b[31mNet Profit: ${:.2}\x1b[0m", net_profit);
            }
        }
        
        if let Some(gross_profit) = result.gross_profit {
            println!("   \x1b[32mGross Profit: ${:.2}\x1b[0m", gross_profit);
        }
        
        if let Some(gross_loss) = result.gross_loss {
            println!("   \x1b[31mGross Loss: ${:.2}\x1b[0m", gross_loss);
        }
        
        // Initial capital calculation
        let initial_capital = self.config.default_initial_capital;
        let return_pct = (total_pnl / initial_capital) * 100.0;
        
        if return_pct >= 0.0 {
            println!("   \x1b[32mReturn: +{:.2}%\x1b[0m", return_pct);
        } else {
            println!("   \x1b[31mReturn: {:.2}%\x1b[0m", return_pct);
        }
    }
    
    fn display_risk_metrics(&self, result: &BacktestResult) {
        println!("\n📊 RISK METRICS:");
        
        // Max Drawdown
        let drawdown = result.max_drawdown.to_f64().unwrap_or(0.0);
        if drawdown > self.config.thresholds.financial.significant_drawdown {
            println!("   \x1b[31m⚠️  Max Drawdown: ${:.2}\x1b[0m", drawdown);
        } else {
            println!("   Max Drawdown: ${:.2}", drawdown);
        }
        
        // Sharpe Ratio with stars
        if let Some(sharpe) = result.sharpe_ratio {
            let thresholds = &self.config.thresholds.sharpe_ratio;
            let stars = if sharpe > thresholds.excellent { "★★★★★" } 
                       else if sharpe > thresholds.very_good { "★★★★☆" }
                       else if sharpe > thresholds.good { "★★★☆☆" }
                       else if sharpe > thresholds.fair { "★★☆☆☆" }
                       else { "★☆☆☆☆" };
            
            if sharpe > thresholds.display_green {
                println!("   \x1b[32mSharpe Ratio: {:.3} {}\x1b[0m", sharpe, stars);
            } else {
                println!("   \x1b[33mSharpe Ratio: {:.3} {}\x1b[0m", sharpe, stars);
            }
        }
        
        // Volatility
        if let Some(vol) = result.volatility {
            let vol_thresholds = &self.config.thresholds.volatility;
            if vol > vol_thresholds.high_risk {
                println!("   \x1b[31m⚠️  Volatility: {:.2}% (High Risk)\x1b[0m", vol);
            } else if vol > vol_thresholds.medium_risk {
                println!("   \x1b[33mVolatility: {:.2}% (Moderate Risk)\x1b[0m", vol);
            } else {
                println!("   \x1b[32mVolatility: {:.2}% (Low Risk)\x1b[0m", vol);
            }
        }
        
        // Profit Factor
        if let Some(pf) = result.profit_factor {
            let pf_thresholds = &self.config.thresholds.profit_factor;
            if pf > pf_thresholds.excellent {
                println!("   \x1b[32mProfit Factor: {:.2} (Excellent)\x1b[0m", pf);
            } else if pf > pf_thresholds.good {
                println!("   \x1b[32mProfit Factor: {:.2} (Good)\x1b[0m", pf);
            } else if pf > pf_thresholds.marginal {
                println!("   \x1b[33mProfit Factor: {:.2} (Marginal)\x1b[0m", pf);
            } else {
                println!("   \x1b[31mProfit Factor: {:.2} (Poor)\x1b[0m", pf);
            }
        }
    }
    
    fn display_trade_stats(&self, result: &BacktestResult) {
        println!("\n📈 TRADE STATISTICS:");
        println!("   Total Trades: {}", result.num_trades);
        
        if let Some(win_rate) = result.win_rate {
            let wr_thresholds = &self.config.thresholds.win_rate;
            if win_rate > wr_thresholds.strong {
                println!("   \x1b[32mWin Rate: {:.1}% (Strong)\x1b[0m", win_rate);
            } else if win_rate > wr_thresholds.average {
                println!("   \x1b[33mWin Rate: {:.1}% (Average)\x1b[0m", win_rate);
            } else {
                println!("   \x1b[31mWin Rate: {:.1}% (Weak)\x1b[0m", win_rate);
            }
        }
        
        if let Some(avg_return) = result.avg_trade_return {
            let min_positive = self.config.thresholds.financial.minimum_return_positive;
            if avg_return > min_positive {
                println!("   \x1b[32mAvg Trade Return: +{:.2}%\x1b[0m", avg_return);
            } else {
                println!("   \x1b[31mAvg Trade Return: {:.2}%\x1b[0m", avg_return);
            }
        }
        
        if let Some(duration) = result.avg_trade_duration {
            println!("   Avg Trade Duration: {:.1}h", duration);
        }
    }
    
    fn display_performance_summary(&self, result: &BacktestResult) {
        println!("\n📊 PERFORMANCE VISUALIZATION:");
        
        // Simple ASCII chart showing performance
        let total_pnl = result.total_pnl.to_f64().unwrap_or(0.0);
        let chart_width = self.config.chart.width;
        let max_value = self.config.chart.max_value;
        
        let bar_length = ((total_pnl.abs() / max_value) * chart_width as f64) as usize;
        let bar_length = bar_length.min(chart_width);
        
        if total_pnl >= 0.0 {
            println!("   P&L Chart: \x1b[32m{}\x1b[0m ${:.0}", 
                    "█".repeat(bar_length), total_pnl);
        } else {
            println!("   P&L Chart: \x1b[31m{}\x1b[0m ${:.0}", 
                    "█".repeat(bar_length), total_pnl);
        }
        
        // Risk-Return scatter plot representation
        if let Some(sharpe) = result.sharpe_ratio {
            if let Some(vol) = result.volatility {
                println!("\n   Risk-Return Profile:");
                let vol_thresholds = &self.config.thresholds.volatility;
                let risk_level = if vol > vol_thresholds.high_risk { "High Risk" } 
                               else if vol > vol_thresholds.medium_risk { "Med Risk" } 
                               else { "Low Risk" };
                               
                let sharpe_thresholds = &self.config.thresholds.sharpe_ratio;
                let return_level = if sharpe > sharpe_thresholds.very_good { "High Return" } 
                                 else if sharpe > 0.8 { "Med Return" } 
                                 else { "Low Return" };
                                 
                println!("   {} × {} → Strategy: {}", risk_level, return_level, 
                        self.classify_strategy(sharpe, vol));
            }
        }
    }
    
    fn display_equity_curve(&self, result: &BacktestResult) {
        if result.equity_curve.is_empty() {
            return;
        }
        
        println!("\n📈 EQUITY CURVE:");
        
        let equity_curve = &result.equity_curve;
        let chart_width = self.config.chart.width;
        let initial_capital = result.initial_capital;
        
        // Find min and max values for scaling
        let min_equity = equity_curve.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        let max_equity = equity_curve.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let range = max_equity - min_equity;
        
        if range == 0.0 {
            println!("   Flat equity curve at ${:.2}", initial_capital);
            return;
        }
        
        // Create sparkline representation
        let chart_height = 8;
        let mut chart_lines: Vec<String> = vec![String::new(); chart_height];
        
        for (i, &value) in equity_curve.iter().enumerate() {
            if i >= chart_width { break; }
            
            // Normalize value to chart height (0 to chart_height-1)
            let normalized = ((value - min_equity) / range * (chart_height - 1) as f64) as usize;
            let y = chart_height - 1 - normalized; // Flip y-axis for display
            
            // Use different characters for different equity levels
            let symbol = if value > initial_capital { 
                "▲" // Above initial capital
            } else if value < initial_capital * 0.95 {
                "▼" // Significant loss
            } else {
                "■" // Around initial capital
            };
            
            // Place symbol in the appropriate line
            while chart_lines[y].len() <= i {
                chart_lines[y].push(' ');
            }
            // Use a simpler approach to avoid Unicode boundary issues
            let mut chars: Vec<char> = chart_lines[y].chars().collect();
            if i < chars.len() {
                chars[i] = match symbol {
                    "▲" => '▲',
                    "▼" => '▼', 
                    _ => '■',
                };
            } else {
                chars.push(match symbol {
                    "▲" => '▲',
                    "▼" => '▼',
                    _ => '■',
                });
            }
            chart_lines[y] = chars.iter().collect();
        }
        
        // Display the chart with value labels
        println!("   ${:.0} ┐", max_equity);
        for (i, line) in chart_lines.iter().enumerate() {
            if i == 0 {
                if max_equity > initial_capital {
                    println!("        │\x1b[32m{}\x1b[0m", line); // Green for gains
                } else {
                    println!("        │{}", line);
                }
            } else if i == chart_height - 1 {
                if min_equity < initial_capital {
                    println!("        │\x1b[31m{}\x1b[0m", line); // Red for losses
                } else {
                    println!("        │{}", line);
                }
            } else {
                println!("        │{}", line);
            }
        }
        println!("   ${:.0} └{}", min_equity, "─".repeat(chart_width.min(equity_curve.len())));
        
        // Summary statistics
        let final_equity = equity_curve.last().unwrap_or(&initial_capital);
        let total_return = ((final_equity - initial_capital) / initial_capital) * 100.0;
        let max_peak = max_equity;
        let max_dd_pct = ((max_peak - min_equity) / max_peak) * 100.0;
        
        if total_return >= 0.0 {
            println!("   \x1b[32m↗ Portfolio: ${:.0} → ${:.0} ({:+.1}%)\x1b[0m", 
                    initial_capital, final_equity, total_return);
        } else {
            println!("   \x1b[31m↘ Portfolio: ${:.0} → ${:.0} ({:+.1}%)\x1b[0m", 
                    initial_capital, final_equity, total_return);
        }
        
        if max_dd_pct > 10.0 {
            println!("   \x1b[31m⚠️  Peak-to-Trough: {:.1}%\x1b[0m", max_dd_pct);
        } else {
            println!("   📊 Peak-to-Trough: {:.1}%", max_dd_pct);
        }
    }
    
    fn display_recommendations(&self, result: &BacktestResult) {
        println!("\n🎯 INTELLIGENT RECOMMENDATIONS:");
        
        let total_pnl = result.total_pnl.to_f64().unwrap_or(0.0);
        let sharpe = result.sharpe_ratio.unwrap_or(0.0);
        let vol = result.volatility.unwrap_or(0.0);
        let win_rate = result.win_rate.unwrap_or(0.0);
        
        let thresholds = &self.config.thresholds;
        
        // Generate recommendations based on performance
        if total_pnl > thresholds.financial.recommendation_profit_check && sharpe > thresholds.sharpe_ratio.display_green {
            println!("   ✅ Strategy shows strong profitability");
            if vol < thresholds.volatility.medium_risk {
                println!("   ✅ Risk management is effective");
            }
        } else {
            println!("   ⚠️  Strategy needs optimization");
        }
        
        if win_rate < thresholds.win_rate.average {
            println!("   📝 Consider improving entry signals");
        }
        
        if vol > thresholds.volatility.high_risk {
            println!("   📝 Implement stricter position sizing");
        }
        
        if let Some(pf) = result.profit_factor {
            if pf < 1.2 {
                println!("   📝 Review exit strategy and stop losses");
            }
        }
        
        // Strategy classification
        println!("\n🏷️  Strategy Classification: {}", 
                self.classify_strategy(sharpe, vol));
    }
    
    fn classify_strategy(&self, sharpe: f64, volatility: f64) -> &str {
        let sharpe_good = sharpe > self.config.thresholds.sharpe_ratio.display_green;
        let vol_low = volatility < self.config.thresholds.volatility.medium_risk;
        
        match (sharpe_good, vol_low) {
            (true, true) => "Conservative Winner",
            (true, false) => "Aggressive Growth",
            (false, true) => "Defensive",
            (false, false) => "High Risk/Low Reward",
        }
    }
}
