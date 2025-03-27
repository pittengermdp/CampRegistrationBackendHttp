use crate::database::get_conn;
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket},
        WebSocketUpgrade,
    },
    response::IntoResponse,
    Extension,
};
use diesel::prelude::*;
use futures::{SinkExt, StreamExt};
use lambda_lib::AppState;
use lambda_lib::PgPool;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
/// WebSocket handler for payment status updates
pub async fn payment_status_ws_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<Mutex<AppState>>>,
    Extension(db_pool): Extension<Arc<PgPool>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state, db_pool))
}

/// Handles an individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<Mutex<AppState>>, db_pool: Arc<PgPool>) {
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

    // Generate a unique connection ID
    let connection_id = uuid::Uuid::new_v4().to_string();

    // Process incoming messages from the WebSocket
    let state_clone = state.clone();
    let db_pool_clone = db_pool.clone();
    let connection_id_clone = connection_id.clone();

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

                                // Get access to websocket service from state
                                let state = state_clone.lock().await;
                                if let Some(ws_service) = &state.websocket_service {
                                    ws_service
                                        .register_client(payment_intent_id.to_string(), tx.clone())
                                        .await;
                                }
                                drop(state);

                                // Store connection in database
                                let customer_id = json
                                    .get("customer_id")
                                    .and_then(|id| id.as_str())
                                    .map(String::from);
                                let customer_email = json
                                    .get("customer_email")
                                    .and_then(|email| email.as_str())
                                    .map(String::from);

                                // Create a new WebSocketConnection record
                                let ws_conn = crate::database::models::WebSocketConnection::new(
                                    payment_intent_id.to_string(),
                                    connection_id_clone.clone(),
                                    customer_id.clone(),
                                    customer_email.clone(),
                                );

                                // Save to database
                                if let Ok(mut conn) = get_conn(&db_pool_clone) {
                                    match diesel::insert_into(
                                        crate::database::schema::websocket_connections::table,
                                    )
                                    .values(&ws_conn)
                                    .execute(&mut conn)
                                    {
                                        Ok(_) => info!("Saved WebSocket connection to database"),
                                        Err(e) => error!(
                                            "Failed to save WebSocket connection to database: {}",
                                            e
                                        ),
                                    }
                                } else {
                                    error!("Failed to get database connection from pool");
                                }

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

    // Clean up when connection is closed
    info!("WebSocket connection closed: {}", connection_id);

    // Update connection status in database to inactive
    if let Ok(mut conn) = get_conn(&db_pool) {
        use crate::database::schema::websocket_connections::dsl::*;

        match diesel::update(websocket_connections.filter(connection_id.eq(connection_id.clone())))
            .set(status.eq("inactive"))
            .execute(&mut conn)
        {
            Ok(_) => info!("Updated WebSocket connection status to inactive"),
            Err(e) => error!("Failed to update WebSocket connection status: {}", e),
        }
    } else {
        error!("Failed to get database connection from pool for cleanup");
    }
}
