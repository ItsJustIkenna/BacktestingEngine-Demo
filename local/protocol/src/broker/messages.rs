use prost::Message;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct Wallet {
    #[prost(string, tag = "1")]
    pub user_id: String,
    #[prost(string, tag = "2")]
    pub symbol: String,
    #[prost(float, tag = "3")]
    pub balance: f32,
    #[prost(string, tag = "4")]
    pub currency: String,
    #[prost(int64, tag = "5")]
    pub last_updated: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct WalletData {
    #[prost(string, tag = "1")]
    pub exchange: String,
    #[prost(message, repeated, tag = "2")]
    pub wallets: Vec<Wallet>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct Trade {
    #[prost(string, tag = "1")]
    pub symbol: String,
    #[prost(string, tag = "2")]
    pub exchange: String,
    #[prost(string, tag = "3")]
    pub side: String,
    #[prost(float, tag = "4")]
    pub price: f32,
    #[prost(float, tag = "5")]
    pub quantity: f32,
    #[prost(float, tag = "6")]
    pub qty: f32,
    #[prost(string, tag = "7")]
    pub ord_type: String,
    #[prost(int64, tag = "8")]
    pub timestamp: i64,
    #[prost(string, tag = "9")]
    pub trade_id: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct Trades {
    #[prost(message, repeated, tag = "1")]
    pub trades: Vec<Trade>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct Order {
    #[prost(string, tag = "1")]
    pub unique_id: String,
    #[prost(string, tag = "2")]
    pub symbol: String,
    #[prost(string, tag = "3")]
    pub exchange: String,
    #[prost(float, tag = "4")]
    pub price_level: f32,
    #[prost(float, tag = "5")]
    pub quantity: f32,
    #[prost(string, tag = "6")]
    pub side: String,
    #[prost(string, tag = "7")]
    pub event: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct Orders {
    #[prost(message, repeated, tag = "1")]
    pub orders: Vec<Order>,
}

pub mod market_message {
    use super::*;
    #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
    pub enum Payload {
        TradesPayload(Trades),
        OrdersPayload(Orders),
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct MarketMessage {
    pub market_id: String,
    pub payload: Option<market_message::Payload>,
}

pub mod portfolio_message {
    use super::*;

    #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
    pub struct ExecutionReport {
        pub message: String,
    }

    #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
    pub struct RiskAlert {
        pub message: String,
    }

    #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
    pub enum Payload {
        WalletsPayload(WalletData),
        Position(Wallet),
        Balance(Wallet),
        Update(ExecutionReport),
        Risk(RiskAlert),
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct PortfolioMessage {
    pub portfolio_id: String,
    pub payload: Option<portfolio_message::Payload>,
}

pub mod publish_request {
    use super::*;
    #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
    pub enum Payload {
        MarketPayload(MarketMessage),
        PortfolioPayload(PortfolioMessage),
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct PublishRequest {
    pub topic: String,
    pub payload: Option<publish_request::Payload>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct BacktestChunk {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(int32, tag = "2")]
    pub chunk_id: i32,
    #[prost(int32, tag = "3")]
    pub total_chunks: i32,
    #[prost(string, tag = "4")]
    pub start_time: String,
    #[prost(string, tag = "5")]
    pub end_time: String,
    #[prost(string, tag = "6")]
    pub symbol: String,
    #[prost(string, tag = "7")]
    pub exchange: String,
    #[prost(bytes = "vec", tag = "8")]
    pub strategy_config: Vec<u8>,
    #[prost(double, tag = "9")]
    pub initial_capital: f64,
    #[prost(double, tag = "10")]
    pub starting_position: f64,
    #[prost(double, tag = "11")]
    pub starting_cash: f64,
    #[prost(bool, tag = "12")]
    pub is_monte_carlo: bool,
    #[prost(int32, tag = "13")]
    pub monte_carlo_run: i32,
    #[prost(int32, tag = "14")]
    pub priority: i32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct BacktestChunkResult {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(int32, tag = "2")]
    pub chunk_id: i32,
    #[prost(string, tag = "3")]
    pub worker_id: String,
    #[prost(bool, tag = "4")]
    pub success: bool,
    #[prost(string, tag = "5")]
    pub error_message: String,
    #[prost(double, tag = "6")]
    pub net_pnl: f64,
    #[prost(double, tag = "7")]
    pub gross_profit: f64,
    #[prost(double, tag = "8")]
    pub gross_loss: f64,
    #[prost(int32, tag = "9")]
    pub num_trades: i32,
    #[prost(int32, tag = "10")]
    pub winning_trades: i32,
    #[prost(double, tag = "11")]
    pub max_drawdown: f64,
    #[prost(double, tag = "12")]
    pub transaction_costs: f64,
    #[prost(double, tag = "13")]
    pub ending_position: f64,
    #[prost(double, tag = "14")]
    pub ending_cash: f64,
    #[prost(bytes = "vec", tag = "15")]
    pub daily_returns: Vec<u8>,
    #[prost(int64, tag = "16")]
    pub events_processed: i64,
    #[prost(int64, tag = "17")]
    pub processing_duration_ms: i64,
    #[prost(int64, tag = "18")]
    pub peak_memory_bytes: i64,
    #[prost(int32, tag = "19")]
    pub monte_carlo_run: i32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct BacktestProgress {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(int32, tag = "2")]
    pub chunks_completed: i32,
    #[prost(int32, tag = "3")]
    pub total_chunks: i32,
    #[prost(float, tag = "4")]
    pub percent_complete: f32,
    #[prost(string, tag = "5")]
    pub phase: String,
    #[prost(int64, tag = "6")]
    pub estimated_remaining_secs: i64,
    #[prost(double, tag = "7")]
    pub running_pnl: f64,
    #[prost(int32, tag = "8")]
    pub running_trades: i32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct BacktestCancelRequest {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(string, tag = "2")]
    pub reason: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct BacktestAggregatedResult {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(double, tag = "2")]
    pub total_net_pnl: f64,
    #[prost(double, tag = "3")]
    pub total_gross_profit: f64,
    #[prost(double, tag = "4")]
    pub total_gross_loss: f64,
    #[prost(int32, tag = "5")]
    pub total_trades: i32,
    #[prost(int32, tag = "6")]
    pub total_winning_trades: i32,
    #[prost(float, tag = "7")]
    pub win_rate: f32,
    #[prost(float, tag = "8")]
    pub profit_factor: f32,
    #[prost(double, tag = "9")]
    pub max_drawdown: f64,
    #[prost(float, tag = "10")]
    pub sharpe_ratio: f32,
    #[prost(float, tag = "11")]
    pub sortino_ratio: f32,
    #[prost(float, tag = "12")]
    pub calmar_ratio: f32,
    #[prost(double, tag = "13")]
    pub total_transaction_costs: f64,
    #[prost(int64, tag = "14")]
    pub total_events: i64,
    #[prost(int64, tag = "15")]
    pub total_processing_time_ms: i64,
    #[prost(int32, tag = "16")]
    pub num_workers: i32,
    #[prost(int32, tag = "17")]
    pub chunks_processed: i32,
    #[prost(int32, tag = "18")]
    pub chunks_failed: i32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct ChromosomeEvalRequest {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(int32, tag = "2")]
    pub generation: i32,
    #[prost(int32, tag = "3")]
    pub chromosome_id: i32,
    #[prost(int32, tag = "4")]
    pub total_chromosomes: i32,
    #[prost(bytes = "vec", tag = "5")]
    pub chromosome_params: Vec<u8>,
    #[prost(string, tag = "6")]
    pub symbol: String,
    #[prost(string, tag = "7")]
    pub exchange: String,
    #[prost(double, tag = "8")]
    pub initial_capital: f64,
    #[prost(string, tag = "9")]
    pub data_cache_key: String,
    #[prost(bool, tag = "10")]
    pub is_final_evaluation: bool,
    #[prost(bool, tag = "11")]
    pub run_monte_carlo: bool,
    #[prost(bool, tag = "12")]
    pub run_walk_forward: bool,
    #[prost(int64, tag = "13")]
    pub train_end_idx: i64,
    #[prost(int64, tag = "14")]
    pub test_end_idx: i64,
    #[prost(int64, tag = "15")]
    pub total_ticks: i64,
    #[prost(bytes = "vec", tag = "16")]
    pub fitness_weights_json: Vec<u8>,
    #[prost(double, tag = "17")]
    pub initial_30d_volume: f64,
    #[prost(double, tag = "18")]
    pub initial_inventory_pct: f64,
    #[prost(uint32, tag = "19")]
    pub monte_carlo_runs: u32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct ChromosomeEvalResult {
    #[prost(string, tag = "1")]
    pub job_id: String,
    #[prost(int32, tag = "2")]
    pub generation: i32,
    #[prost(int32, tag = "3")]
    pub chromosome_id: i32,
    #[prost(string, tag = "4")]
    pub worker_id: String,
    #[prost(bool, tag = "5")]
    pub success: bool,
    #[prost(string, tag = "6")]
    pub error_message: String,
    #[prost(double, tag = "7")]
    pub fitness: f64,
    #[prost(double, tag = "8")]
    pub net_pnl: f64,
    #[prost(double, tag = "9")]
    pub sharpe_ratio: f64,
    #[prost(int32, tag = "10")]
    pub num_trades: i32,
    #[prost(double, tag = "11")]
    pub max_drawdown: f64,
    #[prost(bytes = "vec", tag = "12")]
    pub equity_curve: Vec<u8>,
    #[prost(int64, tag = "13")]
    pub processing_duration_ms: i64,
    #[prost(double, tag = "14")]
    pub win_rate: f64,
    #[prost(double, tag = "15")]
    pub profit_factor: f64,
    #[prost(int32, tag = "16")]
    pub closed_trades: i32,
    #[prost(int32, tag = "17")]
    pub total_orders: i32,
    #[prost(double, tag = "18")]
    pub starting_capital: f64,
    #[prost(double, tag = "19")]
    pub ending_capital: f64,
    #[prost(double, tag = "20")]
    pub total_return_pct: f64,
    #[prost(bytes = "vec", tag = "21")]
    pub monte_carlo_results: Vec<u8>,
    #[prost(bytes = "vec", tag = "22")]
    pub walk_forward_results: Vec<u8>,
    #[prost(bytes = "vec", tag = "23")]
    pub closed_trades_data: Vec<u8>,
    #[prost(bool, tag = "24")]
    pub is_final_evaluation: bool,
    #[prost(string, tag = "25")]
    pub statistical_significance: String,
    #[prost(double, tag = "26")]
    pub best_trade_pnl: f64,
    #[prost(double, tag = "27")]
    pub worst_trade_pnl: f64,
    #[prost(double, tag = "28")]
    pub avg_trade_pnl: f64,
    #[prost(double, tag = "29")]
    pub avg_time_in_trade_ms: f64,
    #[prost(double, tag = "30")]
    pub total_volume: f64,
    #[prost(double, tag = "31")]
    pub total_fees: f64,
    #[prost(bytes = "vec", tag = "32")]
    pub inventory_metrics: Vec<u8>,
    #[prost(bytes = "vec", tag = "33")]
    pub execution_metrics: Vec<u8>,
    #[prost(double, tag = "34")]
    pub avg_inventory: f64,
    #[prost(double, tag = "35")]
    pub max_inventory: f64,
    #[prost(int32, tag = "36")]
    pub zero_crossings: i32,
    #[prost(double, tag = "37")]
    pub avg_fill_rate: f64,
    #[prost(double, tag = "38")]
    pub avg_queue_position: f64,
    #[prost(double, tag = "39")]
    pub partial_fill_pct: f64,
    #[prost(double, tag = "40")]
    pub avg_latency_ms: f64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct DataBroadcastRequest {
    #[prost(string, tag = "1")]
    pub cache_key: String,
    #[prost(bytes = "vec", tag = "2")]
    pub payload: Vec<u8>,
    #[prost(bytes = "vec", tag = "3")]
    pub compressed_data: Vec<u8>,
    #[prost(int64, tag = "4")]
    pub ttl_seconds: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct DataLoadRequest {
    #[prost(string, tag = "1")]
    pub cache_key: String,
    #[prost(string, repeated, tag = "2")]
    pub symbols: Vec<String>,
    #[prost(string, tag = "3")]
    pub exchange: String,
    #[prost(string, tag = "4")]
    pub start_time: String,
    #[prost(string, tag = "5")]
    pub end_time: String,
    #[prost(int64, tag = "6")]
    pub ttl_seconds: i64,
    #[prost(double, tag = "7")]
    pub initial_capital: f64,
    #[prost(string, tag = "8")]
    pub job_id: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Message)]
pub struct DataCacheAck {
    #[prost(string, tag = "1")]
    pub cache_key: String,
    #[prost(bool, tag = "2")]
    pub success: bool,
    #[prost(string, tag = "3")]
    pub worker_id: String,
    #[prost(int32, tag = "4")]
    pub capacity: i32,
    #[prost(int64, tag = "5")]
    pub tick_count: i64,
}
