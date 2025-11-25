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
    
    // Spawn a task to send progress updates
    let send_task = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        
        loop {
            ticker.tick().await;
            
            // Fetch job status
            match state.job_queue.get_job(&job_id).await {
                Ok(Some(job)) => {
                    let message = serde_json::json!({
                        "job_id": job.id,
                        "status": format!("{:?}", job.status).to_lowercase(),
                        "progress": job.progress,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    
                    if sender
                        .send(Message::Text(message.to_string()))
                        .await
                        .is_err()
                    {
                        break; // Client disconnected
                    }
                    
                    // Stop sending if job is completed or failed
                    if job.status == JobStatus::Completed || job.status == JobStatus::Failed {
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
