use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        Path, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{error, info};

use lambda_lib::structs::AppState;

/// WebSocket handler for payment status updates
pub async fn payment_status_ws_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(app_state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, app_state))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Task that forwards messages from the channel to the WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sender
                .send(Message::Text(Utf8Bytes::try_from(message).unwrap()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Process incoming messages from the WebSocket
    let ws_service = app_state.websocket_service.clone();
    let mut receive_task = tokio::spawn(async move {
        while let Some(Ok(message)) = receiver.next().await {
            if let Message::Text(text) = message {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Handle subscription request
                    if let Some(msg_type) = json.get("type").and_then(|t| t.as_str()) {
                        if msg_type == "subscribe" {
                            if let Some(payment_intent_id) =
                                json.get("payment_intent_id").and_then(|id| id.as_str())
                            {
                                info!(
                                    "Client subscribed to payment updates for: {}",
                                    payment_intent_id
                                );

                                ws_service
                                    .as_ref()
                                    .unwrap()
                                    .register_client(payment_intent_id.to_string(), tx.clone())
                                    .await;

                                // Send confirmation to client
                                let confirmation = json!({
                                    "type": "subscription_confirmed",
                                    "payment_intent_id": payment_intent_id
                                })
                                .to_string();

                                if tx.send(confirmation).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    error!("Failed to parse WebSocket message as JSON: {}", text);
                }
            }
        }
    });

    // If any of the tasks complete, abort the other one
    tokio::select! {
        _ = (&mut send_task) => receive_task.abort(),
        _ = (&mut receive_task) => send_task.abort(),
    }

    info!("WebSocket connection closed");
}
