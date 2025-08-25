//! Enhanced Strategy Export Configuration
//! 
//! This module provides ConfigMap-driven configuration for strategy export,
//! replacing hardcoded defaults with Kubernetes-native configuration management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;

/// Strategy approval levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalLevel {
    Development,
    Staging,
    Production,
}

/// Strategy export configuration loaded from ConfigMap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// Performance evaluation thresholds
    pub performance_thresholds: PerformanceThresholds,
    
    /// Deployment configuration
    pub deployment: DeploymentConfig,
    
    /// File system configuration
    pub filesystem: FilesystemConfig,
    
    /// Approval workflow configuration
    pub approval: ApprovalConfig,
    
    /// Risk management configuration
    pub risk_management: RiskManagementConfig,
}

/// Performance thresholds from ConfigMap instead of hardcoded defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceThresholds {
    /// Minimum Sharpe ratio required
    #[serde(default = "default_min_sharpe_ratio")]
    pub min_sharpe_ratio: f64,
    
    /// Minimum profit factor required  
    #[serde(default = "default_min_profit_factor")]
    pub min_profit_factor: f64,
    
    /// Minimum win rate required (0.0 to 1.0)
    #[serde(default = "default_min_win_rate")]
    pub min_win_rate: f64,
    
    /// Maximum drawdown allowed (0.0 to 1.0)
    #[serde(default = "default_max_drawdown")]
    pub max_drawdown: f64,
    
    /// Minimum number of trades required
    #[serde(default = "default_min_trades")]
    pub min_trades: u32,
    
    /// Minimum backtest duration in days
    #[serde(default = "default_min_backtest_days")]
    pub min_backtest_days: u32,
    
    /// Minimum total PnL in base currency
    #[serde(default = "default_min_total_pnl")]
    pub min_total_pnl: bigdecimal::BigDecimal,
    
    /// Minimum average trade return percentage
    #[serde(default = "default_min_avg_trade_return")]
    pub min_avg_trade_return: f64,
    
    /// Maximum volatility (annualized)
    #[serde(default = "default_max_volatility")]
    pub max_volatility: f64,
    
    /// Minimum absolute profit required
    #[serde(default = "default_min_net_profit")]
    pub min_net_profit: f64,
    
    /// Maximum average trade duration (hours)
    #[serde(default = "default_max_avg_trade_duration")]
    pub max_avg_trade_duration: f64,
    
    /// Minimum volatility-adjusted return
    #[serde(default = "default_min_volatility_adjusted_return")]
    pub min_volatility_adjusted_return: f64,
    
    /// Minimum Calmar ratio (return/max_drawdown)
    #[serde(default = "default_min_calmar_ratio")]  
    pub min_calmar_ratio: f64,
    
    /// Whether to require positive return skew
    #[serde(default = "default_require_positive_skew")]
    pub require_positive_skew: bool,
    
    /// Environment-specific overrides
    pub environment_overrides: HashMap<String, EnvironmentThresholds>,
}

/// Environment-specific threshold overrides (dev/staging/prod)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentThresholds {
    pub min_sharpe_ratio: Option<f64>,
    pub min_profit_factor: Option<f64>,
    pub min_win_rate: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub min_trades: Option<u32>,
    pub min_backtest_days: Option<u32>,
    pub min_total_pnl: Option<bigdecimal::BigDecimal>,
    pub min_avg_trade_return: Option<f64>,
    pub max_volatility: Option<f64>,
    pub min_net_profit: Option<f64>,
}

/// Deployment configuration from ConfigMap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    /// SignalEngine deployment path
    pub signal_engine_path: String,
    
    /// Deployment environments and their paths
    pub environments: HashMap<String, EnvironmentConfig>,
    
    /// Auto-deployment settings
    pub auto_deployment: AutoDeploymentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub path: String,
    pub enabled: bool,
    pub max_strategies: Option<u32>,
    pub approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDeploymentConfig {
    pub enabled: bool,
    pub target_environment: String, // "development", "staging", "production"
    pub batch_size: u32,
    pub delay_between_deployments: u64, // seconds
}

/// Filesystem configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    pub export_base_path: String,
    pub strategy_source_path: String,
    pub backup_enabled: bool,
    pub backup_retention_days: u32,
}

/// Approval workflow configuration  
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    /// Required approvals per environment
    pub environments: HashMap<String, ApprovalRequirement>,
    
    /// Notification settings
    pub notifications: NotificationConfig,
    
    /// Audit settings
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequirement {
    pub required_approvers: u32,
    pub allowed_roles: Vec<String>,
    pub auto_approve_threshold: Option<f64>, // Auto-approve if score > threshold
    pub timeout_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub email_recipients: Vec<String>,
    pub slack_webhook: Option<String>,
    pub teams_webhook: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    pub enabled: bool,
    pub retention_days: u32,
    pub include_performance_data: bool,
    pub include_parameters: bool,
}

/// Risk management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskManagementConfig {
    /// Global risk limits
    pub global_limits: GlobalRiskLimits,
    
    /// Per-strategy risk limits
    pub strategy_limits: StrategyRiskLimits,
    
    /// Environment-specific limits
    pub environment_limits: HashMap<String, EnvironmentRiskLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalRiskLimits {
    pub max_total_exposure: f64,
    pub max_strategies_per_symbol: u32,
    pub max_strategies_per_exchange: u32,
    pub emergency_stop_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRiskLimits {
    pub max_position_size: f64,
    pub max_order_size: f64,
    pub max_daily_loss: f64,
    pub max_leverage: f64,
    pub position_concentration_limit: f64,
    // Additional fields for LiveTradingConfig compatibility
    pub max_position_pct: f64,
    pub stop_loss_pct: f64,
    pub take_profit_pct: Option<f64>,
    pub daily_loss_limit: bigdecimal::BigDecimal,
    pub cooldown_minutes: u32,
    pub risk_scaling: f64,
}

impl Default for StrategyRiskLimits {
    fn default() -> Self {
        Self {
            max_position_size: 10000.0,
            max_order_size: 1000.0,
            max_daily_loss: 5000.0,
            max_leverage: 5.0,
            position_concentration_limit: 0.2,
            max_position_pct: 0.1,
            stop_loss_pct: 0.05,
            take_profit_pct: Some(0.15),
            daily_loss_limit: bigdecimal::BigDecimal::from(500),
            cooldown_minutes: 60,
            risk_scaling: 0.8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentRiskLimits {
    pub capital_allocation_limit: f64,
    pub max_concurrent_strategies: u32,
    pub testing_mode: bool,
    // Additional fields for deployment config
    pub paper_trading: bool,
    pub paper_trading_days: u32,
}

impl Default for EnvironmentRiskLimits {
    fn default() -> Self {
        Self {
            capital_allocation_limit: 100000.0,
            max_concurrent_strategies: 5,
            testing_mode: true,
            paper_trading: true,
            paper_trading_days: 7,
        }
    }
}

impl ExportConfig {
    /// Load configuration from ConfigMap-mounted file
    pub fn from_configmap(config_path: &str) -> Result<Self> {
        let config_content = std::fs::read_to_string(config_path)?;
        let mut config: ExportConfig = serde_yaml::from_str(&config_content)?;
        
        // Apply environment variable overrides
        config.apply_env_overrides();
        
        // Validate configuration
        config.validate()?;
        
        Ok(config)
    }
    
    /// Apply environment variable overrides (for Kubernetes secrets, etc.)
    pub fn apply_env_overrides(&mut self) {
        // Override SignalEngine path from env
        if let Ok(se_path) = std::env::var("SIGNAL_ENGINE_PATH") {
            self.deployment.signal_engine_path = se_path;
        }
        
        // Override performance thresholds based on environment
        if let Ok(env) = std::env::var("ENVIRONMENT") {
            if let Some(env_thresholds) = self.performance_thresholds.environment_overrides.get(&env) {
                if let Some(sharpe) = env_thresholds.min_sharpe_ratio {
                    self.performance_thresholds.min_sharpe_ratio = sharpe;
                }
                if let Some(profit_factor) = env_thresholds.min_profit_factor {
                    self.performance_thresholds.min_profit_factor = profit_factor;
                }
                // ... apply other overrides
            }
        }
        
        // Override notification settings from secrets
        if let Ok(slack_webhook) = std::env::var("SLACK_WEBHOOK_URL") {
            self.approval.notifications.slack_webhook = Some(slack_webhook);
        }
    }
    
    /// Validate the loaded configuration
    pub fn validate(&self) -> Result<()> {
        // Validate performance thresholds
        if self.performance_thresholds.min_sharpe_ratio < 0.0 {
            return Err(anyhow::anyhow!("min_sharpe_ratio must be >= 0.0"));
        }
        if self.performance_thresholds.min_win_rate < 0.0 || self.performance_thresholds.min_win_rate > 1.0 {
            return Err(anyhow::anyhow!("min_win_rate must be between 0.0 and 1.0"));
        }
        if self.performance_thresholds.max_drawdown < 0.0 || self.performance_thresholds.max_drawdown > 1.0 {
            return Err(anyhow::anyhow!("max_drawdown must be between 0.0 and 1.0"));
        }
        
        // Validate deployment paths exist
        if !std::path::Path::new(&self.deployment.signal_engine_path).exists() {
            return Err(anyhow::anyhow!("SignalEngine path does not exist: {}", self.deployment.signal_engine_path));
        }
        
        // Validate risk limits
        if self.risk_management.global_limits.max_total_exposure <= 0.0 {
            return Err(anyhow::anyhow!("max_total_exposure must be > 0.0"));
        }
        
        Ok(())
    }

    /// Load configuration from environment variables (for local development)
    pub fn from_env() -> Result<Self> {
        // For now, return a default configuration
        // In production, this would load from mounted ConfigMap at /etc/config/export-config.yaml
        let config_path = std::env::var("EXPORT_CONFIG_PATH")
            .unwrap_or_else(|_| "/etc/config/export-config.yaml".to_string());
        
        if std::path::Path::new(&config_path).exists() {
            Self::from_configmap(&config_path)
        } else {
            // Fallback to default configuration for development
            Ok(Self::default())
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            performance_thresholds: PerformanceThresholds::default(),
            deployment: DeploymentConfig::default(),
            filesystem: FilesystemConfig::default(),
            approval: ApprovalConfig::default(),
            risk_management: RiskManagementConfig::default(),
        }
    }
}

impl Default for PerformanceThresholds {
    fn default() -> Self {
        Self {
            min_sharpe_ratio: default_min_sharpe_ratio(),
            min_profit_factor: default_min_profit_factor(),
            min_win_rate: default_min_win_rate(),
            max_drawdown: default_max_drawdown(),
            min_trades: default_min_trades(),
            min_backtest_days: default_min_backtest_days(),
            min_total_pnl: default_min_total_pnl(),
            min_avg_trade_return: default_min_avg_trade_return(),
            max_volatility: default_max_volatility(),
            min_net_profit: default_min_net_profit(),
            max_avg_trade_duration: default_max_avg_trade_duration(),
            min_volatility_adjusted_return: default_min_volatility_adjusted_return(),
            min_calmar_ratio: default_min_calmar_ratio(),
            require_positive_skew: default_require_positive_skew(),
            environment_overrides: HashMap::new(),
        }
    }
}

impl Default for DeploymentConfig {
    fn default() -> Self {
        Self {
            signal_engine_path: "../SignalEngine".to_string(),
            environments: HashMap::new(),
            auto_deployment: AutoDeploymentConfig::default(),
        }
    }
}

impl Default for AutoDeploymentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_environment: "development".to_string(),
            batch_size: 1,
            delay_between_deployments: 60,
        }
    }
}

impl Default for FilesystemConfig {
    fn default() -> Self {
        Self {
            export_base_path: "./exported_strategies".to_string(),
            strategy_source_path: "./strategy/src/strategies".to_string(),
            backup_enabled: true,
            backup_retention_days: 30,
        }
    }
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            environments: HashMap::new(),
            notifications: NotificationConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            email_recipients: Vec::new(),
            slack_webhook: None,
            teams_webhook: None,
        }
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
            include_performance_data: true,
            include_parameters: true,
        }
    }
}

impl Default for RiskManagementConfig {
    fn default() -> Self {
        Self {
            global_limits: GlobalRiskLimits::default(),
            strategy_limits: StrategyRiskLimits::default(),
            environment_limits: HashMap::new(),
        }
    }
}

impl Default for GlobalRiskLimits {
    fn default() -> Self {
        Self {
            max_total_exposure: 1000000.0,
            max_strategies_per_symbol: 3,
            max_strategies_per_exchange: 10,
            emergency_stop_enabled: true,
        }
    }
}

// Default value functions for serde defaults
fn default_min_sharpe_ratio() -> f64 { 1.5 }
fn default_min_profit_factor() -> f64 { 1.3 }
fn default_min_win_rate() -> f64 { 0.55 }
fn default_max_drawdown() -> f64 { 0.15 }
fn default_min_trades() -> u32 { 100 }
fn default_min_backtest_days() -> u32 { 90 }
fn default_min_total_pnl() -> bigdecimal::BigDecimal { bigdecimal::BigDecimal::from(1000) }
fn default_min_avg_trade_return() -> f64 { 0.001 }
fn default_max_volatility() -> f64 { 0.30 }
fn default_min_net_profit() -> f64 { 10000.0 }
fn default_max_avg_trade_duration() -> f64 { 24.0 }
fn default_min_volatility_adjusted_return() -> f64 { 0.8 }
fn default_min_calmar_ratio() -> f64 { 1.2 }
fn default_require_positive_skew() -> bool { true }
