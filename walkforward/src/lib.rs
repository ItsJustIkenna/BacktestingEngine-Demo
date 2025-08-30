//! Walk-forward analysis module

use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

pub mod window;
pub mod runner;
pub mod result;
pub mod auto_export;

// Trading performance metrics for walkforward analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingMetrics {
    pub total_return: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_trades: usize,
    pub avg_trade_duration_hours: f64,
    pub volatility: f64,
}

impl Default for TradingMetrics {
    fn default() -> Self {
        Self {
            total_return: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            win_rate: 0.0,
            profit_factor: 1.0,
            total_trades: 0,
            avg_trade_duration_hours: 0.0,
            volatility: 0.0,
        }
    }
}

impl TradingMetrics {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: u32,
    pub optimization_window: OptimizationWindow,
    pub test_window: TestWindow,
}

// Main types needed by integration tests
#[derive(Debug, Clone)]
pub struct WalkForwardConfig {
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
    pub optimization_window_size: std::time::Duration,
    pub test_window_size: std::time::Duration,
    pub max_parameters: usize,
    pub optimization_period_days: i64,
    pub test_period_days: i64,
    pub step_size_days: i64,
    pub min_trades_per_period: usize,
    pub parameters_to_optimize: Vec<String>,
    pub optimization_metric: String,
}

impl WalkForwardConfig {
    pub fn new(
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
        optimization_window_size: std::time::Duration,
        test_window_size: std::time::Duration,
        max_parameters: usize,
    ) -> Self {
        let optimization_period_days = optimization_window_size.as_secs() / (24 * 3600);
        let test_period_days = test_window_size.as_secs() / (24 * 3600);
        
        Self {
            start_date,
            end_date,
            optimization_window_size,
            test_window_size,
            max_parameters,
            optimization_period_days: optimization_period_days as i64,
            test_period_days: test_period_days as i64,
            step_size_days: 21, // Default to 1 month steps
            min_trades_per_period: 10,
            parameters_to_optimize: vec!["ma_period".to_string()],
            optimization_metric: "sharpe_ratio".to_string(),
        }
    }
}

impl Default for WalkForwardConfig {
    fn default() -> Self {
        use chrono::TimeZone;
        let start_date = chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap();
        let end_date = chrono::Utc.with_ymd_and_hms(2023, 12, 31, 23, 59, 59).unwrap();
        
        Self {
            start_date,
            end_date,
            optimization_window_size: std::time::Duration::from_secs(252 * 24 * 3600),
            test_window_size: std::time::Duration::from_secs(63 * 24 * 3600),
            max_parameters: 5,
            optimization_period_days: 252, // 1 year
            test_period_days: 63, // 3 months
            step_size_days: 21, // 1 month
            min_trades_per_period: 10,
            parameters_to_optimize: vec!["ma_period".to_string()],
            optimization_metric: "sharpe_ratio".to_string(),
        }
    }
}

impl WalkForwardConfig {
    pub fn is_valid(&self) -> bool {
        self.optimization_period_days > 30 &&
        self.test_period_days > 0 &&
        self.step_size_days > 0 &&
        self.min_trades_per_period > 0 &&
        !self.parameters_to_optimize.is_empty() &&
        !self.optimization_metric.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationWindow {
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestWindow {
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowResult {
    pub window_id: u32,
    pub optimization_window: OptimizationWindow,
    pub test_window: TestWindow,
    pub optimal_parameters: HashMap<String, f64>,
    pub optimization_score: f64,
    pub out_of_sample_performance: TradingMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardResult {
    pub window_results: Vec<WindowResult>,
    pub overall_metrics: TradingMetrics,
    pub parameter_stability_score: f64,
    pub total_windows: usize,
    pub successful_windows: usize,
    pub avg_optimization_time_ms: u64,
}

pub struct WalkForwardAnalyzer {
    pub config: WalkForwardConfig,
    backtest_config: config::BacktestConfig,
}

impl WalkForwardAnalyzer {
    pub fn new(config: WalkForwardConfig, backtest_config: config::BacktestConfig) -> Self {
        Self {
            config,
            backtest_config,
        }
    }
    
    pub fn get_optimization_period_days(&self) -> i64 {
        self.config.optimization_period_days
    }
    
    pub fn get_config(&self) -> &WalkForwardConfig {
        &self.config
    }
    
    pub async fn run_analysis(&self) -> anyhow::Result<WalkForwardResult> {
        // Placeholder implementation
        Ok(WalkForwardResult {
            window_results: Vec::new(),
            overall_metrics: TradingMetrics::default(),
            parameter_stability_score: 0.0,
            total_windows: 0,
            successful_windows: 0,
            avg_optimization_time_ms: 0,
        })
    }
    
    pub fn validate_config(&self) -> anyhow::Result<()> {
        if !self.config.is_valid() {
            return Err(anyhow::anyhow!("Invalid walk forward configuration"));
        }
        Ok(())
    }
    
    pub fn create_windows(&self, start_date: DateTime<Utc>, end_date: DateTime<Utc>) -> Vec<Window> {
        // Create a temporary config with the provided dates
        let temp_analyzer = WalkForwardAnalyzer {
            config: WalkForwardConfig {
                start_date,
                end_date,
                ..self.config.clone()
            },
            backtest_config: self.backtest_config.clone(),
        };
        temp_analyzer.generate_windows()
    }
    
    pub fn generate_windows(&self) -> Vec<Window> {
        let start_date = self.config.start_date;
        let end_date = self.config.end_date;
        let mut windows = Vec::new();
        let mut window_id = 1;
        let mut current_opt_start = start_date;
        
        loop {
            let opt_end = current_opt_start + chrono::Duration::days(self.config.optimization_period_days);
            let test_start = opt_end;
            let test_end = test_start + chrono::Duration::days(self.config.test_period_days);
            
            if test_end > end_date {
                break;
            }
            
            windows.push(Window {
                id: window_id,
                optimization_window: OptimizationWindow {
                    start_date: current_opt_start,
                    end_date: opt_end,
                },
                test_window: TestWindow {
                    start_date: test_start,
                    end_date: test_end,
                },
            });
            
            window_id += 1;
            current_opt_start = current_opt_start + chrono::Duration::days(self.config.step_size_days);
        }
        
        windows
    }

    pub async fn optimize_parameters(
        &self,
        parameter_ranges: &HashMap<String, (f64, f64)>,
        _optimization_window: &OptimizationWindow
    ) -> anyhow::Result<HashMap<String, f64>> {
        // Mock optimization - return middle values from ranges
        let mut optimal_params = HashMap::new();
        
        for (param_name, (min_val, max_val)) in parameter_ranges {
            let optimal_value = (min_val + max_val) / 2.0;
            optimal_params.insert(param_name.clone(), optimal_value);
        }
        
        // Add some constraint logic for MA parameters
        if let (Some(short_ma), Some(long_ma)) = (optimal_params.get("short_ma"), optimal_params.get("long_ma")) {
            if short_ma >= long_ma {
                optimal_params.insert("short_ma".to_string(), long_ma - 5.0);
            }
        }
        
        Ok(optimal_params)
    }
    
    pub async fn run_backtest_with_parameters(
        &self,
        _parameters: &HashMap<String, f64>,
        _test_window: &TestWindow
    ) -> anyhow::Result<TradingMetrics> {
        // Mock backtest results
        Ok(TradingMetrics {
            total_return: 8.5,
            sharpe_ratio: 1.4,
            max_drawdown: 6.2,
            win_rate: 0.62,
            profit_factor: 1.7,
            total_trades: 18,
            avg_trade_duration_hours: 28.0,
            volatility: 0.16,
        })
    }
    
    pub async fn run_walk_forward_analysis(
        &self,
        parameter_ranges: HashMap<String, (f64, f64)>,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>
    ) -> anyhow::Result<WalkForwardResult> {
        let windows = self.create_windows(start_date, end_date);
        let mut window_results = Vec::new();
        let start_time = std::time::Instant::now();
        
        for window in windows {
            let optimal_params = self.optimize_parameters(&parameter_ranges, &window.optimization_window).await?;
            let out_of_sample_performance = self.run_backtest_with_parameters(&optimal_params, &window.test_window).await?;
            
            let window_result = WindowResult {
                window_id: window.id,
                optimization_window: window.optimization_window,
                test_window: window.test_window,
                optimal_parameters: optimal_params,
                optimization_score: out_of_sample_performance.sharpe_ratio,
                out_of_sample_performance,
            };
            
            window_results.push(window_result);
        }
        
        let overall_metrics = Self::aggregate_metrics(&window_results);
        let parameter_stability_score = Self::calculate_parameter_stability(&window_results);
        let total_time = start_time.elapsed().as_millis();
        
        Ok(WalkForwardResult {
            window_results: window_results.clone(),
            overall_metrics,
            parameter_stability_score,
            total_windows: window_results.len(),
            successful_windows: window_results.len(), // All successful in mock
            avg_optimization_time_ms: (total_time / window_results.len() as u128) as u64,
        })
    }
    
    pub fn aggregate_metrics(window_results: &[WindowResult]) -> TradingMetrics {
        if window_results.is_empty() {
            return TradingMetrics::default();
        }
        
        let total_return = window_results.iter().map(|w| w.out_of_sample_performance.total_return).sum::<f64>() / window_results.len() as f64;
        let sharpe_ratio = window_results.iter().map(|w| w.out_of_sample_performance.sharpe_ratio).sum::<f64>() / window_results.len() as f64;
        let max_drawdown = window_results.iter().map(|w| w.out_of_sample_performance.max_drawdown).fold(0.0, f64::max);
        let win_rate = window_results.iter().map(|w| w.out_of_sample_performance.win_rate).sum::<f64>() / window_results.len() as f64;
        let profit_factor = window_results.iter().map(|w| w.out_of_sample_performance.profit_factor).sum::<f64>() / window_results.len() as f64;
        let total_trades = window_results.iter().map(|w| w.out_of_sample_performance.total_trades).sum::<usize>();
        let avg_trade_duration = window_results.iter().map(|w| w.out_of_sample_performance.avg_trade_duration_hours).sum::<f64>() / window_results.len() as f64;
        let volatility = window_results.iter().map(|w| w.out_of_sample_performance.volatility).sum::<f64>() / window_results.len() as f64;
        
        TradingMetrics {
            total_return,
            sharpe_ratio,
            max_drawdown,
            win_rate,
            profit_factor,
            total_trades,
            avg_trade_duration_hours: avg_trade_duration,
            volatility,
        }
    }
    
    pub fn calculate_parameter_stability(window_results: &[WindowResult]) -> f64 {
        if window_results.len() < 2 {
            return 1.0;
        }
        
        let mut total_stability = 0.0;
        let mut param_count = 0;
        
        // Get all parameter names
        let all_param_names: std::collections::HashSet<String> = window_results.iter()
            .flat_map(|w| w.optimal_parameters.keys().cloned())
            .collect();
        
        for param_name in all_param_names {
            let values: Vec<f64> = window_results.iter()
                .filter_map(|w| w.optimal_parameters.get(&param_name))
                .copied()
                .collect();
            
            if values.len() >= 2 {
                let mean = values.iter().sum::<f64>() / values.len() as f64;
                let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
                let coefficient_of_variation = if mean != 0.0 { variance.sqrt() / mean.abs() } else { 0.0 };
                
                // Higher stability for lower coefficient of variation
                let param_stability = 1.0 / (1.0 + coefficient_of_variation);
                total_stability += param_stability;
                param_count += 1;
            }
        }
        
        if param_count > 0 {
            total_stability / param_count as f64
        } else {
            1.0
        }
    }
}
