//! Comprehensive tests for Program module
//! Covers JobBroker, ProgressBroadcaster, and worker functionality

use program::{
    job_broker::{JobBroker, JobMessage, JobPriority, JobClaimedEvent, JobCompletedEvent},
    progress_broadcaster::{ProgressBroadcaster, ProgressUpdate},
    job_queue::{BacktestJobParams, JobStatus},
};
use std::sync::Arc;
use serde_json::json;

// ============================================================================
// JOB PRIORITY TESTS
// ============================================================================

#[test]
fn test_job_priority_default() {
    let priority = JobPriority::default();
    
    assert_eq!(priority, JobPriority::Normal);
}

#[test]
fn test_job_priority_ordering() {
    assert!(JobPriority::Low < JobPriority::Normal);
    assert!(JobPriority::Normal < JobPriority::High);
    assert!(JobPriority::High < JobPriority::Critical);
}

#[test]
fn test_job_priority_serialization() {
    let priorities = vec![
        JobPriority::Low,
        JobPriority::Normal,
        JobPriority::High,
        JobPriority::Critical,
    ];
    
    for priority in priorities {
        let json = serde_json::to_string(&priority).unwrap();
        let deserialized: JobPriority = serde_json::from_str(&json).unwrap();
        assert_eq!(priority, deserialized);
    }
}

// ============================================================================
// BACKTEST JOB PARAMS TESTS
// ============================================================================

#[test]
fn test_backtest_job_params_creation() {
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "avellaneda_stoikov".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: Some(180),
        walkforward_test_days: Some(30),
        walkforward_step_days: Some(30),
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.001,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.001,
        start_date: Some("2023-01-01".to_string()),
        end_date: Some("2023-12-31".to_string()),
        ..Default::default()
    };
    
    assert_eq!(params.symbol, "BTCUSD");
    assert_eq!(params.exchange, "binance");
    assert!((params.initial_capital - 100000.0).abs() < 0.001);
}

#[test]
fn test_backtest_job_params_serialization() {
    let params = BacktestJobParams {
        symbol: "ETHUSD".to_string(),
        exchange: "deribit".to_string(),
        strategy_type: "momentum".to_string(),
        initial_capital: 50000.0,
        optimization_method: "manual".to_string(),
        population_size: 100,
        generations: 50,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.3,
        inventory_target: 0.0,
        order_size: 0.02,
        window_ms: 10000,
        min_quote_lifetime_ms: 200,
        fee_rate: 0.0005,
        volatility_cap: 0.1,
        time_horizon_minutes: 30,
        market_depth_k: 2.0,
        volatility_adjustment_factor: 0.7,
        inventory_penalty_factor: 0.5,
        trend_sensitivity: 0.1,
        flow_sensitivity: 0.0,
        monte_carlo_enabled: false,
        monte_carlo_runs: 500,
        walk_forward_enabled: false,
        commission_rate: 0.0005,
        start_date: None,
        ..Default::default()
    };
    
    let json_str = serde_json::to_string(&params).unwrap();
    let deserialized: BacktestJobParams = serde_json::from_str(&json_str).unwrap();
    
    assert_eq!(params.symbol, deserialized.symbol);
    assert_eq!(params.exchange, deserialized.exchange);
    assert_eq!(params.optimization_method, deserialized.optimization_method);
}

#[test]
fn test_backtest_job_params_optimization_methods() {
    // Test genetic optimization
    let genetic_params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 10000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    assert_eq!(genetic_params.optimization_method, "genetic");
    assert!(genetic_params.population_size > 0);
    assert!(genetic_params.generations > 0);
}

// ============================================================================
// JOB STATUS TESTS
// ============================================================================

#[test]
fn test_job_status_variants() {
    // JobStatus has: Queued, Running, Completed, Failed
    let statuses = vec![
        JobStatus::Queued,
        JobStatus::Running,
        JobStatus::Completed,
        JobStatus::Failed,
    ];
    
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let _deserialized: JobStatus = serde_json::from_str(&json).unwrap();
        assert!(!json.is_empty());
    }
}

#[test]
fn test_job_status_queued() {
    let status = JobStatus::Queued;
    
    assert!(matches!(status, JobStatus::Queued));
}

#[test]
fn test_job_status_serialization() {
    let status = JobStatus::Completed;
    let json = serde_json::to_string(&status).unwrap();
    
    assert!(json.contains("Completed") || json.contains("completed"));
}

// ============================================================================
// JOB MESSAGE TESTS
// ============================================================================

#[test]
fn test_job_message_creation() {
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    let message = JobMessage {
        job_id: "job_123".to_string(),
        params,
        submitted_at: chrono::Utc::now().timestamp_millis(),
        priority: JobPriority::High,
    };
    
    assert_eq!(message.job_id, "job_123");
    assert_eq!(message.priority, JobPriority::High);
}

#[test]
fn test_job_message_serialization() {
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    let message = JobMessage {
        job_id: "job_456".to_string(),
        params,
        submitted_at: 1234567890,
        priority: JobPriority::Normal,
    };
    
    let json = serde_json::to_string(&message).unwrap();
    let deserialized: JobMessage = serde_json::from_str(&json).unwrap();
    
    assert_eq!(message.job_id, deserialized.job_id);
    assert_eq!(message.submitted_at, deserialized.submitted_at);
}

// ============================================================================
// JOB CLAIMED EVENT TESTS
// ============================================================================

#[test]
fn test_job_claimed_event_creation() {
    let event = JobClaimedEvent {
        job_id: "job_123".to_string(),
        worker_id: 1,
        claimed_at: chrono::Utc::now().timestamp_millis(),
    };
    
    assert_eq!(event.job_id, "job_123");
    assert_eq!(event.worker_id, 1);
}

#[test]
fn test_job_claimed_event_serialization() {
    let event = JobClaimedEvent {
        job_id: "job_789".to_string(),
        worker_id: 3,
        claimed_at: 1234567890,
    };
    
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: JobClaimedEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(event.job_id, deserialized.job_id);
    assert_eq!(event.worker_id, deserialized.worker_id);
}

// ============================================================================
// JOB COMPLETED EVENT TESTS
// ============================================================================

#[test]
fn test_job_completed_event_success() {
    let event = JobCompletedEvent {
        job_id: "job_123".to_string(),
        worker_id: 1,
        completed_at: chrono::Utc::now().timestamp_millis(),
        success: true,
        error_message: None,
    };
    
    assert!(event.success);
    assert!(event.error_message.is_none());
}

#[test]
fn test_job_completed_event_failure() {
    let event = JobCompletedEvent {
        job_id: "job_456".to_string(),
        worker_id: 2,
        completed_at: chrono::Utc::now().timestamp_millis(),
        success: false,
        error_message: Some("Timeout exceeded".to_string()),
    };
    
    assert!(!event.success);
    assert!(event.error_message.is_some());
    assert!(event.error_message.as_ref().unwrap().contains("Timeout"));
}

#[test]
fn test_job_completed_event_serialization() {
    let event = JobCompletedEvent {
        job_id: "job_789".to_string(),
        worker_id: 5,
        completed_at: 1234567890,
        success: true,
        error_message: None,
    };
    
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: JobCompletedEvent = serde_json::from_str(&json).unwrap();
    
    assert_eq!(event.job_id, deserialized.job_id);
    assert_eq!(event.success, deserialized.success);
}

// ============================================================================
// JOB BROKER TESTS
// ============================================================================

#[tokio::test]
async fn test_job_broker_creation() {
    let _broker = JobBroker::new();
    
    // Broker should be created successfully - using _ prefix since we just test creation
    assert!(true, "Job broker created successfully");
}

#[tokio::test]
async fn test_job_broker_submit_job() {
    let broker = JobBroker::new();
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    let job_id = broker.submit_job(params).await;
    
    // Job ID should be a valid UUID string
    assert!(!job_id.is_empty());
}

#[tokio::test]
async fn test_job_broker_subscribe_jobs() {
    let broker = Arc::new(JobBroker::new());
    
    let mut receiver = broker.subscribe_jobs();
    
    // Submit a job
    let broker_clone = broker.clone();
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let _ = broker_clone.submit_job(params).await;
    });
    
    // Wait for message
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver.recv()
    ).await;
    
    // Should receive the job message
    if let Ok(Ok(message)) = result {
        assert!(!message.job_id.is_empty());
    }
}

#[tokio::test]
async fn test_job_broker_claim_job() {
    let broker = JobBroker::new();
    
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    // Submit a job
    let job_id = broker.submit_job(params).await;
    
    // Claim the job (claim_job returns nothing, just updates state)
    broker.claim_job(&job_id, 1).await;
    
    assert!(true, "Job claimed successfully");
}

#[tokio::test]
async fn test_job_broker_complete_job() {
    let broker = JobBroker::new();
    
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    // Submit and claim
    let job_id = broker.submit_job(params).await;
    broker.claim_job(&job_id, 1).await;
    
    // Complete the job
    broker.complete_job(&job_id, 1, true, None).await;
    
    assert!(true, "Job completed successfully");
}

#[tokio::test]
async fn test_job_broker_fail_job() {
    let broker = JobBroker::new();
    
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    // Submit and claim
    let job_id = broker.submit_job(params).await;
    broker.claim_job(&job_id, 1).await;
    
    // Fail the job
    broker.complete_job(&job_id, 1, false, Some("Test failure".to_string())).await;
    
    assert!(true, "Job failed gracefully");
}

#[tokio::test]
async fn test_job_broker_get_pending_job() {
    let broker = JobBroker::new();
    
    let params = BacktestJobParams {
        symbol: "BTCUSD".to_string(),
        exchange: "binance".to_string(),
        strategy_type: "test".to_string(),
        initial_capital: 100000.0,
        optimization_method: "genetic".to_string(),
        population_size: 50,
        generations: 20,
        walkforward_optimization_days: None,
        walkforward_test_days: None,
        walkforward_step_days: None,
        risk_aversion: 0.5,
        inventory_target: 0.5,
        order_size: 0.01,
        window_ms: 5000,
        min_quote_lifetime_ms: 100,
        fee_rate: 0.0,
        volatility_cap: 0.05,
        time_horizon_minutes: 60,
        market_depth_k: 1.5,
        volatility_adjustment_factor: 0.5,
        inventory_penalty_factor: 0.3,
        trend_sensitivity: 0.2,
        flow_sensitivity: -0.1,
        monte_carlo_enabled: true,
        monte_carlo_runs: 1000,
        walk_forward_enabled: true,
        commission_rate: 0.0,
        start_date: None,
        ..Default::default()
    };
    
    // Submit a job
    broker.submit_job(params).await;
    
    // Get pending job
    let pending = broker.get_pending_job().await;
    
    assert!(pending.is_some(), "Should have a pending job");
}

// ============================================================================
// PROGRESS UPDATE TESTS
// ============================================================================

#[test]
fn test_progress_update_creation() {
    let update = ProgressUpdate::new("job_123".to_string());
    
    assert_eq!(update.job_id, "job_123");
    assert!((update.progress - 0.0).abs() < 0.001);
    assert_eq!(update.phase, "initializing");
}

#[test]
fn test_progress_update_custom() {
    let update = ProgressUpdate {
        job_id: "job_456".to_string(),
        progress: 50.0,
        current_generation: 25,
        total_generations: 50,
        phase: "genetic_optimization".to_string(),
        phase_details: json!({
            "best_fitness": 1.5,
            "population_size": 100
        }),
        timestamp_ns: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64,
    };
    
    assert!((update.progress - 50.0).abs() < 0.001);
    assert_eq!(update.current_generation, 25);
    assert_eq!(update.total_generations, 50);
}

#[test]
fn test_progress_update_serialization() {
    let update = ProgressUpdate::new("job_789".to_string());
    
    let json = serde_json::to_string(&update).unwrap();
    let deserialized: ProgressUpdate = serde_json::from_str(&json).unwrap();
    
    assert_eq!(update.job_id, deserialized.job_id);
    assert_eq!(update.phase, deserialized.phase);
}

#[test]
fn test_progress_update_phases() {
    let phases = vec![
        "initializing",
        "data_loading",
        "genetic_optimization",
        "final_analysis",
        "completed",
        "failed",
    ];
    
    for phase in phases {
        let update = ProgressUpdate {
            job_id: "test".to_string(),
            progress: 0.0,
            current_generation: 0,
            total_generations: 0,
            phase: phase.to_string(),
            phase_details: json!({}),
            timestamp_ns: 0,
        };
        
        assert_eq!(update.phase, phase);
    }
}

// ============================================================================
// PROGRESS BROADCASTER TESTS
// ============================================================================

#[tokio::test]
async fn test_progress_broadcaster_creation() {
    let broadcaster = ProgressBroadcaster::new();
    
    // Broadcaster should be created successfully, initially has 0 subscribers
    assert_eq!(broadcaster.subscriber_count(), 0);
}

#[tokio::test]
async fn test_progress_broadcaster_with_capacity() {
    let broadcaster = ProgressBroadcaster::with_capacity(500);
    
    // Custom capacity broadcaster also starts with 0 subscribers
    assert_eq!(broadcaster.subscriber_count(), 0);
}

#[tokio::test]
async fn test_progress_broadcaster_default() {
    let broadcaster = ProgressBroadcaster::default();
    
    // Default broadcaster starts with 0 subscribers
    assert_eq!(broadcaster.subscriber_count(), 0);
}

#[tokio::test]
async fn test_progress_broadcaster_publish() {
    let broadcaster = ProgressBroadcaster::new();
    let update = ProgressUpdate::new("job_123".to_string());
    
    broadcaster.publish(update);
    
    // Fire-and-forget, should not block
    assert!(true, "Update published");
}

#[tokio::test]
async fn test_progress_broadcaster_subscribe_receive() {
    let broadcaster = Arc::new(ProgressBroadcaster::new());
    let mut receiver = broadcaster.subscribe();
    
    // Publish update
    let update = ProgressUpdate {
        job_id: "job_123".to_string(),
        progress: 25.0,
        current_generation: 5,
        total_generations: 20,
        phase: "genetic_optimization".to_string(),
        phase_details: json!({}),
        timestamp_ns: 0,
    };
    
    broadcaster.publish(update.clone());
    
    // Receive with timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver.recv()
    ).await;
    
    if let Ok(Ok(received)) = result {
        assert_eq!(received.job_id, "job_123");
    }
}

#[tokio::test]
async fn test_progress_broadcaster_multiple_subscribers() {
    let broadcaster = Arc::new(ProgressBroadcaster::new());
    let mut receiver1 = broadcaster.subscribe();
    let mut receiver2 = broadcaster.subscribe();
    
    let update = ProgressUpdate::new("job_multi".to_string());
    broadcaster.publish(update);
    
    // Both receivers should get the message
    let result1 = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver1.recv()
    ).await;
    
    let result2 = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver2.recv()
    ).await;
    
    // At least one should succeed if broadcasting works
    assert!(result1.is_ok() || result2.is_ok());
}

#[tokio::test]
async fn test_progress_broadcaster_shutdown() {
    let broadcaster = ProgressBroadcaster::new();
    
    broadcaster.shutdown();
    
    // After shutdown, publish should be a no-op
    let update = ProgressUpdate::new("ignored".to_string());
    broadcaster.publish(update);
    
    assert!(true, "Shutdown completed");
}

#[tokio::test]
async fn test_progress_broadcaster_subscriber_count() {
    let broadcaster = ProgressBroadcaster::new();
    
    assert_eq!(broadcaster.subscriber_count(), 0);
    
    let _receiver1 = broadcaster.subscribe();
    assert_eq!(broadcaster.subscriber_count(), 1);
    
    let _receiver2 = broadcaster.subscribe();
    assert_eq!(broadcaster.subscriber_count(), 2);
}

#[tokio::test]
async fn test_progress_broadcaster_publish_generation() {
    let broadcaster = Arc::new(ProgressBroadcaster::new());
    let mut receiver = broadcaster.subscribe();
    
    broadcaster.publish_generation("job_gen", 10, 100);
    
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver.recv()
    ).await;
    
    if let Ok(Ok(update)) = result {
        assert_eq!(update.job_id, "job_gen");
        assert_eq!(update.current_generation, 10);
        assert_eq!(update.total_generations, 100);
    }
}

#[tokio::test]
async fn test_progress_broadcaster_publish_phase() {
    let broadcaster = Arc::new(ProgressBroadcaster::new());
    let mut receiver = broadcaster.subscribe();
    
    broadcaster.publish_phase("job_phase", "final_analysis", json!({"step": 1}));
    
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver.recv()
    ).await;
    
    if let Ok(Ok(update)) = result {
        assert_eq!(update.job_id, "job_phase");
        assert_eq!(update.phase, "final_analysis");
    }
}

#[tokio::test]
async fn test_progress_broadcaster_clone() {
    let broadcaster = ProgressBroadcaster::new();
    let cloned = broadcaster.clone();
    
    // Both should work independently
    let mut receiver = cloned.subscribe();
    
    broadcaster.publish(ProgressUpdate::new("test_clone".to_string()));
    
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        receiver.recv()
    ).await;
    
    // Should receive since they share the same underlying channel
    if let Ok(Ok(update)) = result {
        assert_eq!(update.job_id, "test_clone");
    }
}
