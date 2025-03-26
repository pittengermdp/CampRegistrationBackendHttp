use axum::{
    routing::{get, post},
    Extension, Router,
};
use lambda_http::run;
use lambda_lib::{get_stripe_keys, structs::WebSocketService, AppState};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod handlers;
use handlers::{create_payment_sheet_handler, hello_handler, stripe_handler};
mod stripe_webhook;
use stripe_webhook::webhook_handler;
mod websocket_handler;
use websocket_handler::payment_status_ws_handler;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    let filter = EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into());
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

    // Initialize the WebSocket service
    let websocket_service = WebSocketService::new();

    // Create AppState with optional WebSocket service
    let state = AppState {
        stripe_keys,
        websocket_service: Some(WebSocketService::new()),
        database_client: None, // Initialize if needed
    };
    let state_arc = Arc::new(state);

    // Configure HTTP routes
    let stripe_routes = Router::new()
        .route("/stripe_key", get(stripe_handler))
        .route("/payment_sheet", post(create_payment_sheet_handler))
        .route("/webhook", post(webhook_handler))
        .layer(Extension(state_arc.clone()));

    // WebSocket route for payment status updates
    let ws_routes = Router::new()
        .route("/payment_status", get(payment_status_ws_handler))
        .layer(Extension(state_arc.clone()));

    // Build the Axum router with all endpoints
    let app = Router::new()
        .route("/hello", get(hello_handler))
        .merge(stripe_routes)
        .merge(ws_routes)
        .layer(TraceLayer::new_for_http());

    // Start the lambda handler
    run(app).await?;
    Ok(())
}
