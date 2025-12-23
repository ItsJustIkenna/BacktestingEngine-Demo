use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use thiserror::Error;

/// Variable latency simulation with jitter and spikes
pub mod variable_latency;

pub use variable_latency::{VariableLatencyConfig, LatencySimulator, TimeOfDayLatencyPattern};

/// Configuration validation errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing required field: {field}")]
    MissingField { field: String },
    
    #[error("Invalid value for {field}: {value} (reason: {reason})")]
    InvalidValue {
        field: String,
        value: String,
        reason: String,
    },
    
    #[error("File not found: {path}")]
    FileNotFound { path: String },
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Main backtesting configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestConfig {
    pub database: DatabaseConfig,
    pub trading: TradingConfig,
    pub data: DataConfig,
    pub output: OutputConfig,
    pub analysis: AnalysisConfig,
}

/// Database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: Option<u32>,
    pub connection_timeout: Option<u64>, // seconds
}

/// Trading-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    pub commission_rate: f64,      // e.g., 0.001 for 0.1% (fallback for simple fee)
    pub slippage_bps: f64,         // basis points
    pub initial_capital: f64,
    pub max_position_size: Option<f64>, // as percentage of portfolio
    pub risk_free_rate: f64,       // for Sharpe ratio calculation
    pub exchange_fees: Option<ExchangeFeeConfig>, // Maker/taker fee configuration
    pub routing_mode: Option<RoutingMode>, // Multi-exchange routing configuration
    /// Simulated network/exchange latency in milliseconds
    /// Orders placed within this time window of a trade won't be filled
    /// This simulates realistic order propagation delay (default: 50ms)
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u64,
    /// Latency jitter percentage (0.0-1.0)
    /// Adds random variation to latency: actual = latency_ms * (1 + jitter * random(-1, 1))
    /// Default: 0.2 (20% variation)
    #[serde(default = "default_latency_jitter")]
    pub latency_jitter_pct: f64,
    /// Minimum latency floor in milliseconds (even with negative jitter)
    #[serde(default = "default_min_latency_ms")]
    pub min_latency_ms: u64,
    /// Latency spike probability (0.0-1.0)
    /// Probability of experiencing a latency spike (3-10x normal latency)
    #[serde(default = "default_latency_spike_prob")]
    pub latency_spike_probability: f64,
    /// Performance optimization settings
    #[serde(default)]
    pub optimization: PerformanceOptimization,
}

/// Performance optimization settings for backtesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceOptimization {
    /// Enable adaptive sampling in GA (coarser data in early generations)
    /// Provides 1.3-1.5x speedup with minimal accuracy loss
    #[serde(default = "default_true")]
    pub adaptive_sampling_enabled: bool,
    /// Enable stable region skipping (skip strategy recalc when price unchanged)
    /// Provides 1.2-1.3x speedup depending on market activity
    #[serde(default = "default_true")]
    pub stable_region_skipping: bool,
    /// Price change threshold for stable region detection (as ratio, e.g., 0.0001 = 1bp)
    #[serde(default = "default_stable_threshold")]
    pub stable_region_threshold: f64,
    /// Maximum ticks to skip between forced strategy updates
    #[serde(default = "default_max_skip_ticks")]
    pub max_skip_ticks: usize,
    /// Enable SIMD-accelerated calculations
    #[serde(default = "default_true")]
    pub simd_enabled: bool,
    /// Override for MAX_CONCURRENT_EVALS (0 = auto-detect from memory)
    #[serde(default)]
    pub max_concurrent_evals: usize,
}

fn default_true() -> bool { true }
fn default_stable_threshold() -> f64 { 0.0001 } // 1 basis point
fn default_max_skip_ticks() -> usize { 100 }

impl Default for PerformanceOptimization {
    fn default() -> Self {
        Self {
            adaptive_sampling_enabled: true,
            stable_region_skipping: true,
            stable_region_threshold: 0.0001,
            max_skip_ticks: 100,
            simd_enabled: true,
            max_concurrent_evals: 0, // Auto-detect
        }
    }
}

fn default_latency_ms() -> u64 { 10 }
fn default_latency_jitter() -> f64 { 0.2 }
fn default_min_latency_ms() -> u64 { 1 }
fn default_latency_spike_prob() -> f64 { 0.01 }

/// Routing mode for multi-exchange strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingMode {
    /// Single exchange trading (default)
    SingleExchange {
        exchange: String,
    },
    /// Smart order routing across multiple exchanges
    SmartRouting {
        exchanges: Vec<String>,
        algorithm: RoutingAlgorithm,
        max_venues_per_order: usize,
        cross_exchange_latency_ms: f64,
    },
    /// Arbitrage-specific routing
    Arbitrage {
        exchanges: Vec<String>,
        min_profit_bps: f64,
        max_holding_time_seconds: u64,
        risk_limits: ArbitrageRiskLimits,
    },
}

/// Smart routing algorithm types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingAlgorithm {
    /// Time-Weighted Average Price
    TWAP,
    /// Volume-Weighted Average Price  
    VWAP,
    /// Implementation Shortfall
    ImplementationShortfall,
    /// Best execution across venues
    BestExecution,
}

/// Arbitrage-specific risk limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageRiskLimits {
    pub max_open_positions: u32,
    pub max_daily_loss: f64,
    pub max_exchange_exposure: f64,
    pub min_liquidity_ratio: f64,
}

impl Default for ArbitrageRiskLimits {
    fn default() -> Self {
        Self {
            max_open_positions: 5,
            max_daily_loss: 1000.0,
            max_exchange_exposure: 10000.0,
            min_liquidity_ratio: 5.0,
        }
    }
}

/// Exchange-specific fee configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeFeeConfig {
    pub exchange: String,          // "kraken", "coinbase", "binance", etc.
    pub maker_fee: f64,           // e.g., 0.0002 (0.02% for Kraken at high volume)
    pub taker_fee: f64,           // e.g., 0.0012 (0.12% for Kraken at high volume)
    pub volume_tiers: Vec<VolumeTier>,
    /// Starting 30-day volume to simulate an established market maker
    /// Set this to skip the highest fee tiers and use more realistic fees
    /// e.g., 1_000_000.0 for $1M 30-day volume = 0.06% maker / 0.16% taker on Kraken
    #[serde(default)]
    pub starting_30d_volume: f64,
    
    // === Order Size Limits ===
    /// Minimum order size in base currency (e.g., 0.0001 BTC for Kraken)
    #[serde(default = "default_min_order_size")]
    pub min_order_size: f64,
    /// Maximum order size in base currency
    #[serde(default = "default_max_order_size")]
    pub max_order_size: f64,
    
    // === Latency Modeling ===
    /// Mean latency in milliseconds for order execution
    #[serde(default = "default_latency_mean")]
    pub latency_mean_ms: f64,
    /// Standard deviation of latency (for jitter simulation)
    #[serde(default = "default_latency_std")]
    pub latency_std_ms: f64,
    
    // === Rate Limiting ===
    /// Maximum orders per second allowed by exchange
    #[serde(default = "default_max_orders_per_second")]
    pub max_orders_per_second: f64,
    /// Cooldown period in ms after rate limit hit
    #[serde(default = "default_rate_limit_cooldown")]
    pub rate_limit_cooldown_ms: i64,
    
    // === Funding Rate (for perpetuals) ===
    /// Funding rate per 8-hour interval (e.g., 0.0001 = 0.01%)
    #[serde(default = "default_funding_rate")]
    pub funding_rate_8h: f64,
    /// Funding interval in milliseconds (default 8 hours)
    #[serde(default = "default_funding_interval")]
    pub funding_interval_ms: i64,
}

// Default value functions for serde
fn default_min_order_size() -> f64 { 0.0001 }  // 0.0001 BTC typical minimum
fn default_max_order_size() -> f64 { 100.0 }   // 100 BTC typical maximum
fn default_latency_mean() -> f64 { 10.0 }
fn default_latency_std() -> f64 { 3.0 }
fn default_max_orders_per_second() -> f64 { 10.0 }
fn default_rate_limit_cooldown() -> i64 { 1000 }
fn default_funding_rate() -> f64 { 0.0001 }
fn default_funding_interval() -> i64 { 8 * 60 * 60 * 1000 }

impl Default for ExchangeFeeConfig {
    /// Default to Kraken fees starting at 0 volume (realistic for new market makers)
    /// Fees will decrease as 30-day volume builds during the backtest
    fn default() -> Self {
        Self::kraken()
    }
}

impl ExchangeFeeConfig {
    /// Kraken's complete fee tier structure (as of 2025)
    pub fn kraken_fee_tiers() -> Vec<VolumeTier> {
        vec![
            VolumeTier { min_volume_30d: 0.0,          maker_fee: 0.0025, taker_fee: 0.0040 },
            VolumeTier { min_volume_30d: 10_000.0,     maker_fee: 0.0020, taker_fee: 0.0035 },
            VolumeTier { min_volume_30d: 50_000.0,     maker_fee: 0.0014, taker_fee: 0.0024 },
            VolumeTier { min_volume_30d: 100_000.0,    maker_fee: 0.0012, taker_fee: 0.0022 },
            VolumeTier { min_volume_30d: 250_000.0,    maker_fee: 0.0010, taker_fee: 0.0020 },
            VolumeTier { min_volume_30d: 500_000.0,    maker_fee: 0.0008, taker_fee: 0.0018 },
            VolumeTier { min_volume_30d: 1_000_000.0,  maker_fee: 0.0006, taker_fee: 0.0016 },
            VolumeTier { min_volume_30d: 2_500_000.0,  maker_fee: 0.0004, taker_fee: 0.0014 },
            VolumeTier { min_volume_30d: 5_000_000.0,  maker_fee: 0.0002, taker_fee: 0.0012 },
            VolumeTier { min_volume_30d: 10_000_000.0, maker_fee: 0.0000, taker_fee: 0.0010 },
            VolumeTier { min_volume_30d: 100_000_000.0, maker_fee: 0.0000, taker_fee: 0.0008 },
        ]
    }
    
    /// Create Kraken exchange configuration (starting from 0 volume)
    pub fn kraken() -> Self {
        Self {
            exchange: "kraken".to_string(),
            maker_fee: 0.0025,  // 0.25% at 0 volume tier
            taker_fee: 0.0040,  // 0.40% at 0 volume tier
            volume_tiers: Self::kraken_fee_tiers(),
            starting_30d_volume: 0.0,
            // Kraken BTC/USD min: 0.0001 BTC, max: varies by pair
            min_order_size: 0.0001,
            max_order_size: 100.0,
            // Kraken REST API: ~50-100ms, Websocket: ~10-20ms
            latency_mean_ms: 15.0,
            latency_std_ms: 5.0,
            // Kraken rate limits: 15/sec burst for adding orders
            max_orders_per_second: 15.0,
            rate_limit_cooldown_ms: 1000,
            // Standard perpetual funding (Kraken Futures)
            funding_rate_8h: 0.0001,
            funding_interval_ms: 8 * 60 * 60 * 1000,
        }
    }
    
    /// Create Binance exchange configuration
    pub fn binance() -> Self {
        Self {
            exchange: "binance".to_string(),
            maker_fee: 0.0010,  // 0.10%
            taker_fee: 0.0010,  // 0.10%
            volume_tiers: vec![
                VolumeTier { min_volume_30d: 0.0,         maker_fee: 0.0010, taker_fee: 0.0010 },
                VolumeTier { min_volume_30d: 1_000_000.0, maker_fee: 0.0009, taker_fee: 0.0010 },
            ],
            starting_30d_volume: 0.0,
            // Binance BTC/USDT min: 0.00001 BTC
            min_order_size: 0.00001,
            max_order_size: 500.0,
            // Binance: Very low latency (~5ms)
            latency_mean_ms: 5.0,
            latency_std_ms: 2.0,
            // Binance rate limits: 10 orders/sec for Spot
            max_orders_per_second: 10.0,
            rate_limit_cooldown_ms: 1000,
            // Binance perpetual funding
            funding_rate_8h: 0.0001,
            funding_interval_ms: 8 * 60 * 60 * 1000,
        }
    }
    
    /// Create Coinbase exchange configuration
    pub fn coinbase() -> Self {
        Self {
            exchange: "coinbase".to_string(),
            maker_fee: 0.0040,  // 0.40%
            taker_fee: 0.0060,  // 0.60%
            volume_tiers: vec![
                VolumeTier { min_volume_30d: 0.0,          maker_fee: 0.0040, taker_fee: 0.0060 },
                VolumeTier { min_volume_30d: 10_000.0,     maker_fee: 0.0025, taker_fee: 0.0040 },
                VolumeTier { min_volume_30d: 100_000.0,    maker_fee: 0.0015, taker_fee: 0.0025 },
            ],
            starting_30d_volume: 0.0,
            // Coinbase BTC-USD min: 0.0001 BTC
            min_order_size: 0.0001,
            max_order_size: 200.0,
            // Coinbase: Moderate latency (~20ms)
            latency_mean_ms: 20.0,
            latency_std_ms: 8.0,
            // Coinbase rate limits: 10 req/sec
            max_orders_per_second: 10.0,
            rate_limit_cooldown_ms: 1000,
            // Coinbase doesn't have perpetuals in US, set nominal rate
            funding_rate_8h: 0.0,
            funding_interval_ms: 8 * 60 * 60 * 1000,
        }
    }
    
    /// Create DeepBook (SUI CLOB) exchange configuration
    /// DeepBook has very competitive fees for market makers
    pub fn deepbook() -> Self {
        Self {
            exchange: "deepbook".to_string(),
            maker_fee: 0.0005,  // 5 bps maker - very competitive
            taker_fee: 0.0010,  // 10 bps taker
            volume_tiers: vec![
                // DeepBook has flat fees (no volume tiers currently)
                VolumeTier { min_volume_30d: 0.0, maker_fee: 0.0005, taker_fee: 0.0010 },
            ],
            starting_30d_volume: 0.0,
            // DeepBook minimum lot sizes (SUI-based)
            min_order_size: 0.001,
            max_order_size: 1000.0,
            // DeepBook on SUI: Very fast on-chain (~400ms finality)
            latency_mean_ms: 400.0,
            latency_std_ms: 100.0,
            // No traditional rate limits (gas limited)
            max_orders_per_second: 100.0,
            rate_limit_cooldown_ms: 100,
            // No funding on spot CLOB
            funding_rate_8h: 0.0,
            funding_interval_ms: 8 * 60 * 60 * 1000,
        }
    }
    
    /// Create Cetus (SUI AMM) exchange configuration
    pub fn cetus() -> Self {
        Self {
            exchange: "cetus".to_string(),
            maker_fee: 0.0030,  // 30 bps - AMM pool fee
            taker_fee: 0.0030,  // 30 bps - same for AMM
            volume_tiers: vec![
                VolumeTier { min_volume_30d: 0.0, maker_fee: 0.0030, taker_fee: 0.0030 },
            ],
            starting_30d_volume: 0.0,
            // Cetus minimum swap sizes
            min_order_size: 0.001,
            max_order_size: 1000.0,
            // Cetus on SUI: Similar to DeepBook
            latency_mean_ms: 400.0,
            latency_std_ms: 100.0,
            // No traditional rate limits (gas limited)
            max_orders_per_second: 100.0,
            rate_limit_cooldown_ms: 100,
            // No funding on AMM
            funding_rate_8h: 0.0,
            funding_interval_ms: 8 * 60 * 60 * 1000,
        }
    }
    
    /// Create configuration for a specific exchange by name
    pub fn for_exchange(exchange: &str) -> Self {
        match exchange.to_lowercase().as_str() {
            "kraken" => Self::kraken(),
            "binance" => Self::binance(),
            "coinbase" => Self::coinbase(),
            "deepbook" | "sui" => Self::deepbook(),
            "cetus" => Self::cetus(),
            _ => {
                // Default to generic conservative fees with typical CEX parameters
                Self {
                    exchange: exchange.to_string(),
                    maker_fee: 0.0010,
                    taker_fee: 0.0015,
                    volume_tiers: vec![],
                    starting_30d_volume: 0.0,
                    // Conservative defaults for unknown exchanges
                    min_order_size: 0.0001,
                    max_order_size: 100.0,
                    latency_mean_ms: 50.0,
                    latency_std_ms: 20.0,
                    max_orders_per_second: 5.0,
                    rate_limit_cooldown_ms: 2000,
                    funding_rate_8h: 0.0001,
                    funding_interval_ms: 8 * 60 * 60 * 1000,
                }
            }
        }
    }
}

/// Volume-based fee tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeTier {
    pub min_volume_30d: f64,      // Minimum 30-day volume in USD
    pub maker_fee: f64,
    pub taker_fee: f64,
}

/// Data source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    pub default_timeframe: String, // e.g., "1d", "1h", "5m"
    pub data_sources: Vec<String>, // e.g., ["yahoo", "alpha_vantage"]
    pub cache_enabled: bool,
    pub cache_duration_hours: u32,
    pub gap_detection: GapDetectionConfig,
    pub market_impact: MarketImpactConfig,
}

/// Gap detection and handling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapDetectionConfig {
    /// Enable gap detection
    pub enabled: bool,
    /// Expected data frequency in seconds (e.g., 60 for 1-minute data)
    pub expected_frequency_seconds: i64,
    /// Gap threshold multiplier (gap = frequency * threshold)
    pub gap_threshold_multiplier: f64,
    /// Gap filling strategy
    pub fill_strategy: GapFillStrategy,
    /// Warn if gap exceeds this duration (seconds)
    pub warning_threshold_seconds: i64,
    /// Fail backtest if gap exceeds this duration (seconds)
    pub critical_threshold_seconds: Option<i64>,
    /// Order stream detection (for market making strategies)
    pub order_detection: Option<OrderDetectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GapFillStrategy {
    /// Do nothing - process data as-is
    None,
    /// Forward fill - use last known values
    ForwardFill,
    /// Linear interpolation between points
    LinearInterpolation,
    /// Mark gap periods and exclude from analysis
    ExcludeGaps,
}

/// Market impact modeling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketImpactConfig {
    /// Enable market impact modeling
    pub enabled: bool,
    /// Impact model type
    pub model: MarketImpactModel,
    /// Minimum order size ratio to trigger impact (order_size / avg_volume)
    pub min_impact_ratio: f64,
    /// Linear impact coefficient (basis points per 1% of volume)
    pub linear_coefficient: f64,
    /// Square root impact coefficient
    pub sqrt_coefficient: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MarketImpactModel {
    /// No market impact
    None,
    /// Linear model: impact = coefficient * (order_size / volume)
    Linear,
    /// Square root model: impact = coefficient * sqrt(order_size / volume)
    SquareRoot,
    /// Almgren-Chriss model (more sophisticated)
    AlmgrenChriss,
}

/// Order stream detection configuration (for market making strategies)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDetectionConfig {
    /// Enable order stream analysis
    pub enabled: bool,
    /// Expected quote update interval in milliseconds (your strategy's quote refresh rate)
    /// For co-located Kraken: 10-100ms, typical HFT market making: 50-200ms
    pub expected_quote_interval_ms: u64,
    /// Require bid-ask pairs (both sides present)
    pub require_paired_quotes: bool,
    /// Tolerance window for pairing (milliseconds)
    pub pairing_window_ms: u64,
    /// Maximum gap before warning (in number of missing quote cycles)
    pub warning_missing_cycles: u64,
}

impl Default for OrderDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            expected_quote_interval_ms: 50, // 50ms for co-located market making
            require_paired_quotes: true,
            pairing_window_ms: 20, // 20ms tolerance for bid-ask pairing
            warning_missing_cycles: 10, // Warn after 500ms of missing quotes (10 * 50ms)
        }
    }
}

impl Default for GapDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            expected_frequency_seconds: 60, // 1-minute data
            gap_threshold_multiplier: 2.0,  // gaps > 2x frequency
            fill_strategy: GapFillStrategy::ForwardFill,
            warning_threshold_seconds: 300, // 5 minutes
            critical_threshold_seconds: Some(3600), // 1 hour
            order_detection: None, // Disabled by default, enable for market making
        }
    }
}

impl Default for MarketImpactConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: MarketImpactModel::SquareRoot,
            min_impact_ratio: 0.01, // 1% of volume
            linear_coefficient: 10.0, // 10 bps per 1% of volume
            sqrt_coefficient: 5.0,
        }
    }
}

/// Output and reporting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub results_dir: String,
    pub reports_dir: String,
    pub charts_enabled: bool,
    pub export_formats: Vec<String>, // e.g., ["json", "csv", "html"]
}

/// Monte Carlo simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    /// Number of Monte Carlo simulation runs
    pub num_simulations: usize,
    /// Confidence level for percentile calculations
    pub confidence_level: f64,
    /// Random seed for reproducibility (optional)
    pub random_seed: Option<u64>,
    /// Bootstrap sample size as fraction of original data
    pub bootstrap_fraction: f64,
    /// Whether to bootstrap trade returns or equity curve
    pub bootstrap_method: BootstrapMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BootstrapMethod {
    /// Bootstrap individual trade returns
    #[serde(rename = "trades")]
    TradeReturns,
    /// Bootstrap blocks of equity curve data
    #[serde(rename = "blocks")]
    BlockBootstrap,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            num_simulations: 500,
            confidence_level: 0.95,
            random_seed: None,
            bootstrap_fraction: 1.0,
            bootstrap_method: BootstrapMethod::TradeReturns,
        }
    }
}

/// Report generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportConfig {
    pub output_dir: PathBuf,
    pub formats: Vec<ReportFormat>,
    pub include_trades: bool,
    pub include_charts: bool,
    pub template_dir: Option<PathBuf>,
    pub custom_css: Option<String>,
    /// Whether to persist report metadata to database (enabled by default for historical tracking)
    pub persist_to_database: bool,
    /// Database connection for persistence (if enabled, reads from DATABASE_URL env var)
    pub database_url: Option<String>,
    /// User or system generating the report
    pub generated_by: Option<String>,
    /// Source of report generation
    pub generation_source: String,
}

/// Supported report formats
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportFormat {
    Html,
    Json,
    Csv,
    Pdf,
}

impl std::fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportFormat::Html => write!(f, "html"),
            ReportFormat::Json => write!(f, "json"),
            ReportFormat::Csv => write!(f, "csv"),
            ReportFormat::Pdf => write!(f, "pdf"),
        }
    }
}

/// Genetic algorithm configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneticConfig {
    pub population_size: usize,
    pub generations: usize,
    pub crossover_rate: f64,
    pub mutation_rate: f64,
    
    // Lazy evaluation parameters for Monte Carlo consistency checking
    #[serde(default)]
    pub use_monte_carlo_fitness: bool,     // Enable MC consistency in fitness
    #[serde(default = "default_mc_sample_size")]
    pub mc_sample_size: usize,             // Number of MC runs for fitness (50-100)
    #[serde(default = "default_top_percentile")]
    pub top_percentile_threshold: f64,     // Only evaluate top N% (e.g., 0.10 for top 10%)
}

fn default_mc_sample_size() -> usize { 50 }
fn default_top_percentile() -> f64 { 0.10 }

/// Configuration for order book reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookConfig {
    /// Maximum number of price levels to maintain on each side
    pub max_levels: Option<usize>,
    /// Whether to maintain detailed order information
    pub track_individual_orders: bool,
    /// Snapshot validation frequency (in events)
    pub snapshot_validation_frequency: Option<usize>,
    /// Whether to enable strict validation
    pub strict_validation: bool,
}

/// Batch processing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    pub batch_size: usize,
    pub max_parallel: usize,
    pub retry_attempts: usize,
}

/// Strategy configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Unique strategy identifier
    pub id: String,
    /// Human-readable strategy name
    pub name: String,
    /// Strategy type/category
    pub strategy_type: String,
    /// Symbol to trade
    pub symbol: String,
    /// Exchange to trade on
    pub exchange: String,
    /// Strategy-specific parameters
    pub parameters: HashMap<String, ParameterValue>,
    /// Risk management settings
    pub risk_limits: RiskLimits,
    /// Whether the strategy is enabled
    pub enabled: bool,
}

/// Parameter value that can hold different types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    Int(i64),       // Renamed from Integer for consistency
    Float(f64),
    String(String),
    Bool(bool),     // Renamed from Boolean for consistency
    Array(Vec<ParameterValue>), // Added for array support
}

impl ParameterValue {
    /// Get as integer
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ParameterValue::Int(v) => Some(*v),
            ParameterValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Get as float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            ParameterValue::Float(v) => Some(*v),
            ParameterValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    
    /// Get as bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParameterValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
    
    /// Get as array
    pub fn as_array(&self) -> Option<&Vec<ParameterValue>> {
        match self {
            ParameterValue::Array(v) => Some(v),
            _ => None,
        }
    }

    /// Get as string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            ParameterValue::String(v) => Some(v),
            _ => None,
        }
    }
}

/// Risk management limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum position size
    pub max_position_size: f64,
    /// Maximum order size
    pub max_order_size: f64,
    /// Maximum daily loss threshold
    pub max_daily_loss: f64,
    /// Maximum portfolio value at risk
    pub max_var: Option<f64>,
    /// Stop loss percentage
    pub stop_loss_pct: Option<f64>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "trading_platform".to_string(),
            username: "postgres".to_string(),
            password: "password".to_string(),
            max_connections: Some(10),
            connection_timeout: Some(30),
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            commission_rate: 0.001,     // 0.1% (fallback, use exchange_fees instead)
            slippage_bps: 2.0,          // 2 basis points for co-located
            initial_capital: 100000.0,  // $100,000
            max_position_size: Some(0.1), // 10%
            risk_free_rate: 0.02,       // 2%
            exchange_fees: Some(ExchangeFeeConfig::default()), // Kraken at $5M volume
            routing_mode: None,         // Single exchange by default
            latency_ms: 10,             // 10ms simulated latency (realistic for co-located)
            latency_jitter_pct: 0.2,    // 20% variation
            min_latency_ms: 1,          // Minimum 1ms even with negative jitter
            latency_spike_probability: 0.01, // 1% chance of latency spike
            optimization: PerformanceOptimization::default(),
        }
    }
}

impl ExchangeFeeConfig {
    /// Kraken exchange fee configuration starting at 0 volume (new market maker)
    /// Delegates to the main kraken() factory method
    pub fn kraken_default() -> Self {
        Self::kraken()
    }

    /// Coinbase Pro exchange fee configuration
    /// Delegates to the main coinbase() factory method
    pub fn coinbase_default() -> Self {
        Self::coinbase()
    }

    /// Get applicable fee rate based on volume and liquidity type
    pub fn get_fee_rate(&self, volume_30d: f64, is_maker: bool) -> f64 {
        // Create a fallback tier
        let fallback_tier = VolumeTier {
            min_volume_30d: 0.0,
            maker_fee: self.maker_fee,
            taker_fee: self.taker_fee,
        };

        // Find the highest applicable tier
        let applicable_tier = self.volume_tiers
            .iter()
            .filter(|tier| volume_30d >= tier.min_volume_30d)
            .max_by(|a, b| a.min_volume_30d.partial_cmp(&b.min_volume_30d).unwrap())
            .unwrap_or(&fallback_tier);

        if is_maker {
            applicable_tier.maker_fee
        } else {
            applicable_tier.taker_fee
        }
    }
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            default_timeframe: "1d".to_string(),
            data_sources: vec!["yahoo".to_string()],
            cache_enabled: true,
            cache_duration_hours: 24,
            gap_detection: GapDetectionConfig::default(),
            market_impact: MarketImpactConfig::default(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            results_dir: "./results".to_string(),
            reports_dir: "./reports".to_string(),
            charts_enabled: true,
            export_formats: vec!["json".to_string(), "csv".to_string()],
        }
    }
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("./reports"),
            formats: vec![ReportFormat::Html, ReportFormat::Json],
            include_trades: true,
            include_charts: true,
            template_dir: None,
            custom_css: None,
            persist_to_database: false,
            database_url: None,
            generated_by: None,
            generation_source: "cli".to_string(),
        }
    }
}

impl Default for GeneticConfig {
    fn default() -> Self {
        Self {
            population_size: 50,
            generations: 100,
            crossover_rate: 0.8,
            mutation_rate: 0.1,
            use_monte_carlo_fitness: true,      // Enabled by default for robustness
            mc_sample_size: 50,                 // Fast MC for fitness eval
            top_percentile_threshold: 0.10,     // Only check top 10%
        }
    }
}

impl Default for OrderBookConfig {
    fn default() -> Self {
        Self {
            max_levels: Some(100), // Keep top 100 levels by default
            track_individual_orders: true,
            snapshot_validation_frequency: Some(1000),
            strict_validation: true,
        }
    }
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            max_parallel: 4,
            retry_attempts: 3,
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default Strategy".to_string(),
            strategy_type: "basic".to_string(),
            symbol: "BTCUSD".to_string(),
            exchange: "default".to_string(),
            parameters: HashMap::new(),
            risk_limits: RiskLimits::default(),
            enabled: true,
        }
    }
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_position_size: 1.0,
            max_order_size: 0.1,
            max_daily_loss: 1000.0,
            max_var: None,
            stop_loss_pct: None,
        }
    }
}

impl BacktestConfig {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        
        let config: BacktestConfig = serde_yaml::from_str(&contents)?;
        
        config.validate()?;
        Ok(config)
    }
    
    /// Save configuration to a YAML file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let contents = serde_yaml::to_string(self)?;
        
        fs::write(path, contents)?;
        
        Ok(())
    }
    
    /// Create a default configuration file if it doesn't exist
    pub fn create_default_if_missing<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        
        if path.exists() {
            Self::from_file(path)
        } else {
            let default_config = Self::default();
            default_config.to_file(path)?;
            Ok(default_config)
        }
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate database config
        if self.database.host.is_empty() {
            return Err(ConfigError::MissingField {
                field: "database.host".to_string(),
            });
        }
        
        if self.database.port == 0 {
            return Err(ConfigError::InvalidValue {
                field: "database.port".to_string(),
                value: "0".to_string(),
                reason: "Port must be greater than 0".to_string(),
            });
        }
        
        // Validate trading config
        if self.trading.commission_rate < 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "trading.commission_rate".to_string(),
                value: self.trading.commission_rate.to_string(),
                reason: "Commission rate cannot be negative".to_string(),
            });
        }
        
        if self.trading.initial_capital <= 0.0 {
            return Err(ConfigError::InvalidValue {
                field: "trading.initial_capital".to_string(),
                value: self.trading.initial_capital.to_string(),
                reason: "Initial capital must be positive".to_string(),
            });
        }
        
        if let Some(max_pos) = self.trading.max_position_size {
            if max_pos <= 0.0 || max_pos > 1.0 {
                return Err(ConfigError::InvalidValue {
                    field: "trading.max_position_size".to_string(),
                    value: max_pos.to_string(),
                    reason: "Max position size must be between 0 and 1".to_string(),
                });
            }
        }
        
        // Validate data config
        if self.data.default_timeframe.is_empty() {
            return Err(ConfigError::MissingField {
                field: "data.default_timeframe".to_string(),
            });
        }
        
        if self.data.data_sources.is_empty() {
            return Err(ConfigError::MissingField {
                field: "data.data_sources".to_string(),
            });
        }
        
        // Validate output config
        if self.output.results_dir.is_empty() {
            return Err(ConfigError::MissingField {
                field: "output.results_dir".to_string(),
            });
        }
        
        if self.output.export_formats.is_empty() {
            return Err(ConfigError::MissingField {
                field: "output.export_formats".to_string(),
            });
        }
        
        Ok(())
    }
}

/// Analysis configuration for Monte Carlo and Walk-Forward
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Analysis mode: "development", "validation", "production"
    pub mode: AnalysisMode,
    /// Skip genetic optimization (used when worker already did it)
    pub skip_genetic_optimization: bool,
    /// Enable automatic Monte Carlo simulation
    pub auto_monte_carlo: bool,
    /// Number of Monte Carlo runs (default: 500)
    pub monte_carlo_runs: usize,
    /// Enable automatic walk-forward analysis  
    pub auto_walk_forward: bool,
    /// Walk-forward training window size in days
    pub wf_training_days: i64,
    /// Walk-forward test window size in days
    pub wf_test_days: i64,
    /// Walk-forward step size in days
    pub wf_step_days: i64,
    /// Minimum backtest duration to trigger auto-analysis (days)
    pub min_duration_for_auto: i64,
    /// Minimum Sharpe ratio to trigger validation mode
    pub validation_sharpe_threshold: f64,
    /// Minimum profit factor to trigger validation mode
    pub validation_pf_threshold: f64,
    /// Minimum number of trades to trigger validation mode
    pub validation_min_trades: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Copy)]
pub enum AnalysisMode {
    /// Fast development mode - basic backtest only
    #[serde(rename = "development")]
    Development,
    /// Validation mode - basic + Monte Carlo
    #[serde(rename = "validation")]
    Validation, 
    /// Production mode - full analysis (basic + MC + walk-forward)
    #[serde(rename = "production")]
    Production,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        // Default to Production mode with full analysis pipeline:
        // 1. Genetic optimization (mandatory before backtesting)
        // 2. Walk-forward analysis (out-of-sample validation)
        // 3. Monte Carlo simulation (parameter robustness)
        Self {
            mode: AnalysisMode::Production,
            skip_genetic_optimization: false,
            auto_monte_carlo: true,
            monte_carlo_runs: 500,
            auto_walk_forward: true,
            wf_training_days: 252,  // 1 year training
            wf_test_days: 63,       // 3 months testing
            wf_step_days: 21,       // 1 month rolling steps
            min_duration_for_auto: 90, // Minimum 90 days for full analysis
            validation_sharpe_threshold: 1.2,
            validation_pf_threshold: 1.4,
            validation_min_trades: 50,
        }
    }
}

impl AnalysisConfig {
    pub fn development() -> Self {
        Self {
            mode: AnalysisMode::Development,
            auto_monte_carlo: false,
            auto_walk_forward: false,
            ..Default::default()
        }
    }
    
    pub fn validation() -> Self {
        Self {
            mode: AnalysisMode::Validation,
            auto_monte_carlo: true,
            auto_walk_forward: false,
            monte_carlo_runs: 250, // Faster for validation
            ..Default::default()
        }
    }
    
    pub fn production() -> Self {
        Self {
            mode: AnalysisMode::Production,
            auto_monte_carlo: true,
            auto_walk_forward: true,
            monte_carlo_runs: 1000, // More thorough for production
            ..Default::default()
        }
    }

    /// Check if auto-escalation to validation mode should trigger
    pub fn should_escalate_to_validation(&self, 
        duration_days: i64, 
        sharpe_ratio: f64, 
        profit_factor: f64, 
        num_trades: u64
    ) -> bool {
        matches!(self.mode, AnalysisMode::Development) &&
        duration_days >= self.min_duration_for_auto &&
        sharpe_ratio >= self.validation_sharpe_threshold &&
        profit_factor >= self.validation_pf_threshold &&
        num_trades >= self.validation_min_trades
    }

    /// Check if auto-escalation to production mode should trigger
    pub fn should_escalate_to_production(&self, 
        mc_confidence_95th: f64,
        export_requested: bool
    ) -> bool {
        matches!(self.mode, AnalysisMode::Validation) &&
        mc_confidence_95th > 1.0 && // Positive expected returns at 95th percentile
        export_requested
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_default_config_creation() {
        let config = BacktestConfig::default();
        assert_eq!(config.database.host, "localhost");
        assert_eq!(config.trading.initial_capital, 100000.0);
        assert_eq!(config.data.default_timeframe, "1d");
    }

    #[test]
    fn test_config_validation() {
        let mut config = BacktestConfig::default();
        
        // Test valid config
        assert!(config.validate().is_ok());
        
        // Test invalid commission rate
        config.trading.commission_rate = -0.1;
        assert!(config.validate().is_err());
        
        // Reset and test invalid capital
        config.trading.commission_rate = 0.001;
        config.trading.initial_capital = -1000.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_yaml_serialization() {
        let config = BacktestConfig::default();
        let yaml_str = serde_yaml::to_string(&config).unwrap();
        
        // Should contain expected fields
        assert!(yaml_str.contains("database"));
        assert!(yaml_str.contains("trading"));
        assert!(yaml_str.contains("data"));
        assert!(yaml_str.contains("output"));
        
        // Should be able to deserialize back
        let parsed: BacktestConfig = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(parsed.database.host, config.database.host);
    }

    #[test]
    fn test_file_operations() {
        let temp_path = PathBuf::from("test_config.yaml");
        let config = BacktestConfig::default();
        
        // Test saving
        config.to_file(&temp_path).unwrap();
        assert!(temp_path.exists());
        
        // Test loading
        let loaded_config = BacktestConfig::from_file(&temp_path).unwrap();
        assert_eq!(loaded_config.database.host, config.database.host);
        assert_eq!(loaded_config.trading.initial_capital, config.trading.initial_capital);
        
        // Cleanup
        fs::remove_file(&temp_path).ok();
    }
}
