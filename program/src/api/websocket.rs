//! WebSocket support for real-time progress updates

use axum::{
    extract::{ws::{Message, WebSocket}, Path, State, WebSocketUpgrade},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::time::Duration;
use tokio::time::interval;

use super::handlers::ApiState;
use crate::job_queue::JobStatus;
use crate::{log_info, log_debug, MAIN_LOGGER};

/// WebSocket handler for backtest progress
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, job_id))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: ApiState, job_id: String) {
    let (mut sender, mut receiver) = socket.split();
    
    log_info!(MAIN_LOGGER, "WebSocket connection established for job {}", job_id);
    
    // Spawn a task to send progress updates
    let job_id_clone = job_id.clone();
    let send_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(500)); // Poll every 500ms for smoother updates
        let mut last_progress: f64 = -1.0;
        
        loop {
            ticker.tick().await;
            
            // Fetch job status
            match state.job_queue.get_job(&job_id_clone).await {
                Ok(Some(job)) => {
                    // Log when progress changes
                    if (job.progress - last_progress).abs() >= 0.1 {
                        log_debug!(MAIN_LOGGER, "WebSocket sending progress update | job={} progress={:.1}% phase={}", 
                            job.id, job.progress, &job.current_phase);
                        last_progress = job.progress;
                    }
                    
                    let message = serde_json::json!({
                        "job_id": job.id,
                        "status": format!("{:?}", job.status).to_lowercase(),
                        "progress": job.progress,
                        "current_generation": job.current_generation,
                        "total_generations": job.total_generations,
                        "phase": job.current_phase,
                        "phase_details": job.phase_details,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    
                    if sender
                        .send(Message::Text(message.to_string()))
                        .await
                        .is_err()
                    {
                        log_debug!(MAIN_LOGGER, "WebSocket client disconnected for job {}", job.id);
                        break; // Client disconnected
                    }
                    
                    // Stop sending if job is completed or failed
                    if job.status == JobStatus::Completed || job.status == JobStatus::Failed {
                        log_info!(MAIN_LOGGER, "WebSocket closing for job {} - status: {:?}", job.id, job.status);
                        // Send one final message with result info
                        if job.status == JobStatus::Completed {
                            let final_msg = serde_json::json!({
                                "job_id": job.id,
                                "status": "completed",
                                "progress": 100.0,
                                "message": "Backtest completed successfully",
                            });
                            let _ = sender.send(Message::Text(final_msg.to_string())).await;
                        }
                        let _ = sender
                            .send(Message::Close(None))
                            .await;
                        break;
                    }
                }
                Ok(None) => {
                    let error = serde_json::json!({
                        "error": "Job not found",
                        "job_id": job_id,
                    });
                    let _ = sender
                        .send(Message::Text(error.to_string()))
                        .await;
                    break;
                }
                Err(e) => {
                    let error = serde_json::json!({
                        "error": format!("Failed to fetch job: {}", e),
                        "job_id": job_id,
                    });
                    let _ = sender
                        .send(Message::Text(error.to_string()))
                        .await;
                    break;
                }
            }
        }
    });
    
    // Handle incoming messages (ping/pong)
    let receive_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                Message::Ping(_data) => {
                    // Response to ping is handled automatically by axum
                }
                _ => {}
            }
        }
    });
    
    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = receive_task => {},
    }
}


