#![feature(trivial_bounds)]
use axum::{
    routing::{get, post},
    Extension, Router,
};
use lambda_http::run;
use lambda_lib::{get_stripe_keys, structs::WebSocketService, AppState, DatabaseClient};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod handlers;
use handlers::{create_payment_sheet_handler, hello_handler, stripe_handler};
mod stripe_webhook;
use stripe_webhook::webhook_handler;
mod websocket_handler;
use websocket_handler::payment_status_ws_handler;
mod database;
use database::create_db_pool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    let filter = EnvFilter::from_default_env().add_directive(tracing::Level::TRACE.into());
    let stdout_layer = fmt::layer()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .with_writer(std::io::stdout);
    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .init();

    info!("Starting HTTP Lambda");

    let stripe_keys = match get_stripe_keys().await {
        Ok(keys) => keys,
        Err((status, msg)) => {
            error!("Error retrieving Stripe keys: {msg}");
            error!("Status code: {status}");
            return Err(msg.into());
        }
    };

    // Initialize database connection
    let db_pool = match create_db_pool() {
        Ok(pool) => {
            info!("Database connection pool created successfully");
            DatabaseClient { pool }
        }
        Err(e) => {
            error!("Failed to create database connection pool: {}", e);
            return Err(e);
        }
    };

    // Initialize the WebSocket service
    let websocket_service = WebSocketService::new();

    // Create AppState with WebSocket service
    let state = AppState {
        stripe_keys,
        websocket_service: Some(websocket_service),
        database_client: Some(db_pool),
    };
    let state_arc = Arc::new(Mutex::new(state));

    // Configure HTTP routes
    let app = Router::new()
        .route("/hello", get(hello_handler))
        .route("/stripe_key", get(stripe_handler))
        .route("/payment_sheet", post(create_payment_sheet_handler))
        .route("/webhook", post(webhook_handler))
        .route("/payment_status", get(payment_status_ws_handler))
        .layer(Extension(state_arc));

    match run(app).await {
        Ok(()) => info!("Lambda executed successfully"),
        Err(e) => error!("Lambda execution error: {e}"),
    }
    Ok(())
}
