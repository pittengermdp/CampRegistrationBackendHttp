use crate::database::{get_conn, models::PaymentEvent};
use axum::{
    body::Body,
    extract::{Extension, FromRequest, FromRequestParts, Request},
    http::request::Parts,
    response::{IntoResponse, Response},
};
use diesel::prelude::*;
use hyper::StatusCode;
use lambda_lib::structs::{AppState, PaymentIntentStatus};
use serde_json::json;
use std::sync::Arc;
use stripe::{Event, EventObject, EventType, Webhook};
use tokio::sync::Mutex;
use tracing::{error, info, trace};

/// Custom extractor for Stripe webhook events.
pub struct StripeEvent(pub Event);

impl<S> FromRequestParts<S> for StripeEvent
where
    S: Send + Sync + core::fmt::Debug,
{
    type Rejection = Response;

    #[tracing::instrument]
    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        trace!("Received webhook event");

        let app_state = parts
            .extensions
            .get::<Arc<Mutex<AppState>>>()
            .ok_or_else(|| StatusCode::BAD_REQUEST.into_response())?
            .clone();

        let state_guard = app_state.lock().await;
        trace!("Got App State for Stripe keys");

        let webhook_secret = state_guard.stripe_keys.webhook_secret.clone();
        drop(state_guard);

        let signature = if let Some(sig) = parts.headers.get("stripe-signature") {
            sig.to_owned()
        } else {
            return Err(StatusCode::BAD_REQUEST.into_response());
        }
        .to_str()
        .map_err(|e| {
            error!("Error converting payload to string: {e}");
            StatusCode::BAD_REQUEST.into_response()
        })?
        .to_string();

        trace!("Signature: {signature}");

        // Extract the payload from the request
        let payload = axum::body::Bytes::from_request(
            Request::from_parts(parts.clone(), Body::empty()),
            state,
        )
        .await
        .map_err(|e| {
            error!("Error reading payload: {e:?}");
            StatusCode::BAD_REQUEST.into_response()
        })?;

        let payload_str = String::from_utf8(payload.to_vec()).map_err(|e| {
            error!("Error converting payload bytes to string: {e}");
            StatusCode::BAD_REQUEST.into_response()
        })?;

        trace!("Payload: {payload_str}");

        // Construct and verify the event.
        let event =
            Webhook::construct_event(&payload_str, &signature, &webhook_secret).map_err(|e| {
                error!("Error constructing event: {e:?}");
                StatusCode::BAD_REQUEST.into_response()
            })?;

        trace!("Event: {event:?}");
        Ok(Self(event))
    }
}

// Also maintain the FromRequest impl for backward compatibility
impl<S> FromRequest<S> for StripeEvent
where
    S: Send + Sync + core::fmt::Debug,
{
    type Rejection = Response;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        // We need to buffer the request body for signature verification
        let (parts, body) = req.into_parts();

        // Collect body bytes
        let bytes = match axum::body::to_bytes(body, 65_536).await {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error reading request body: {e}");
                return Err(StatusCode::BAD_REQUEST.into_response());
            }
        };

        // Create a new request with the body in the request extensions
        let mut parts = parts;
        parts.extensions.insert(axum::body::Bytes::from(bytes));

        // Use the FromRequestParts implementation
        Self::from_request_parts(&mut parts, state).await
    }
}

/// Webhook handler that processes Stripe events.
#[tracing::instrument(skip(state))]
#[axum::debug_handler]
pub async fn webhook_handler(
    StripeEvent(stripe_event): StripeEvent,
    Extension(state): Extension<Arc<Mutex<AppState>>>,
) -> impl IntoResponse {
    trace!("Processing webhook event: {stripe_event:?}");

    // Extract payment intent status from event type
    let status = match PaymentIntentStatus::try_from(stripe_event.type_) {
        Ok(status) => status.to_string(),
        Err(_) => {
            info!("Non-payment-intent event type: {}", stripe_event.type_);
            return (StatusCode::OK, "Webhook received".to_string());
        }
    };

    match stripe_event.type_ {
        EventType::PaymentIntentSucceeded
        | EventType::PaymentIntentCanceled
        | EventType::PaymentIntentPartiallyFunded
        | EventType::PaymentIntentPaymentFailed
        | EventType::PaymentIntentRequiresAction
        | EventType::PaymentIntentRequiresCapture
        | EventType::PaymentIntentAmountCapturableUpdated
        | EventType::PaymentIntentCreated
        | EventType::PaymentIntentProcessing => {
            if let EventObject::PaymentIntent(payment_intent) = stripe_event.data.object {
                info!(
                    "Payment intent event: id={}, status={}",
                    payment_intent.id, status
                );

                // Get currency as string if available
                let currency = payment_intent.currency.to_string();

                // Get customer ID if available
                let customer_id = payment_intent.customer.as_ref().map(|c| c.id().to_string());

                // Extract metadata to identify specific frontends that initiated this payment
                let frontend_id = payment_intent
                    .metadata
                    .get("frontend_id")
                    .and_then(|v| Some(v.as_str()))
                    .map(|s| s.to_string());

                // Save payment event to database
                let payment_event = PaymentEvent::new(
                    payment_intent.id.to_string(),
                    status.clone(),
                    Some(payment_intent.amount),
                    Some(currency.clone()),
                    customer_id.clone(),
                    Some(json!(payment_intent.metadata)),
                );

                let db_client = state.lock().await.database_client.clone();
                if let Some(db_client) = db_client {
                    if let Ok(mut conn) = get_conn(&db_client.pool) {
                        match diesel::insert_into(crate::database::schema::payment_events::table)
                            .values(&payment_event)
                            .execute(&mut conn)
                        {
                            Ok(_) => info!("Saved payment event to database"),
                            Err(e) => error!("Failed to save payment event to database: {}", e),
                        }
                    } else {
                        error!("Failed to get database connection from pool");
                    }
                }

                // Create the notification message
                let message = json!({
                    "type": "payment_update",
                    "payment_intent_id": payment_intent.id.to_string(),
                    "status": status,
                    "amount": payment_intent.amount,
                    "currency": currency,
                    "transaction_id": payment_intent.id.to_string(),
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "customer_id": customer_id,
                    "frontend_id": frontend_id,
                })
                .to_string();

                // Find and notify relevant WebSocket connections
                let db_client = &state.lock().await.database_client;
                if let Some(db_client) = db_client {
                    if let Ok(mut conn) = get_conn(&db_client.pool) {
                        use crate::database::schema::websocket_connections::dsl::*;

                        // Build a query that filters by payment_intent_id and active status
                        let mut query = websocket_connections
                            .filter(payment_intent_id.eq(payment_intent.id.to_string()))
                            .filter(status.eq("active"))
                            .into_boxed();

                        // If we have a frontend_id in metadata, only send to connections from that frontend
                        if let Some(frontend_identifier) = &frontend_id {
                            info!(
                                "Targeting WebSocket connections for frontend_id: {}",
                                frontend_identifier
                            );
                            // This assumes you store the frontend_id in the customer_id or metadata field
                            // You might need to adjust this based on your actual data model
                            query = query.filter(customer_id.eq(frontend_identifier));
                        }

                        match query
                            .select(crate::database::schema::websocket_connections::all_columns)
                            .load::<crate::database::models::WebSocketConnection>(&mut conn)
                        {
                            Ok(connections) => {
                                info!(
                                    "Found {} active connection(s) for payment intent {}",
                                    connections.len(),
                                    payment_intent.id
                                );

                                // Send message to specific connections
                                if !connections.is_empty() {
                                    info!(
                                        "Sending payment update to {} connection(s) for payment intent {}",
                                        connections.len(),
                                        payment_intent.id
                                    );

                                    // Extract connection IDs for targeting
                                    let connection_ids: Vec<String> = connections
                                        .iter()
                                        .map(|conn| conn.connection_id.clone())
                                        .collect();

                                    // Use the WebSocketService to send to specific clients
                                    if let Some(ws_service) = &state.lock().await.websocket_service
                                    {
                                        if let Err(e) = ws_service
                                            .send_message_to_clients(
                                                &payment_intent.id.to_string(),
                                                &message,
                                                &connection_ids,
                                            )
                                            .await
                                        {
                                            error!("Failed to send message to connections: {}", e);
                                        }
                                    } else {
                                        error!("WebSocket service not available in AppState");
                                    }
                                } else {
                                    info!(
                                        "No active connections found for payment intent {}",
                                        payment_intent.id
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Failed to fetch active connections: {}", e);
                            }
                        }
                    }
                }
            }
        }
        EventType::PaymentMethodAttached => {
            if let EventObject::PaymentMethod(payment_method) = stripe_event.data.object {
                info!("PaymentMethod attached: id={}", payment_method.id);
            }
        }
        EventType::ChargeSucceeded | EventType::ChargeUpdated => {
            if let EventObject::Charge(charge) = stripe_event.data.object {
                info!("Charge event: id={}, status={}", charge.id, charge.status);
            }
        }
        _ => {
            info!("Unhandled event type: {}", stripe_event.type_);
        }
    }

    (StatusCode::OK, "Webhook received".to_string())
}
