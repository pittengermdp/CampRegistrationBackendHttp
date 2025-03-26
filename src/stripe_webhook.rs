use axum::{
    body::Body,
    extract::{FromRequest, Request},
    response::{IntoResponse, Response},
};
use hyper::StatusCode;
use stripe::{Event, EventObject, EventType, Webhook};
use tracing::{error, info, trace};

use lambda_lib::structs::{AppState, PaymentIntentStatus};

/// Custom extractor for Stripe webhook events.
pub struct StripeEvent(pub Event);

impl<S> FromRequest<S> for StripeEvent
where
    S: Send + Sync + core::fmt::Debug,
{
    type Rejection = Response;

    #[tracing::instrument]
    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        trace!("Received webhook event");
        let app_state = req
            .extensions()
            .get::<AppState>()
            .ok_or_else(|| StatusCode::BAD_REQUEST.into_response())?
            .clone();
        trace!("Got App State for Stripe keys");
        let webhook_secret = app_state.stripe_keys.webhook_secret.clone();

        let signature = if let Some(sig) = req.headers().get("stripe-signature") {
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

        let payload = String::from_request(req, state).await.map_err(|e| {
            error!("Encountered error {e:?} when converting request to payload as string");
            //IntoResponse::into_response
            e.into_response()
        })?;

        trace!("Payload: {payload}");

        // Construct and verify the event.
        let event =
            Webhook::construct_event(&payload, &signature, &webhook_secret).map_err(|e| {
                error!("Error constructing event: {e:?}");
                StatusCode::BAD_REQUEST.into_response()
            })?;
        trace!("Event: {event:?}");
        Ok(Self(event))
    }
}

/// Webhook handler that processes Stripe events.
#[tracing::instrument(skip(app_state))]
pub async fn webhook_handler(
    StripeEvent(stripe_event): StripeEvent,
    app_state: axum::extract::State<AppState>,
) -> impl IntoResponse {
    trace!("Received webhook event: {stripe_event:?}");

    // Extract payment intent and update database + notify client
    match &stripe_event.type_ {
        EventType::PaymentIntentSucceeded
        | EventType::PaymentIntentCanceled
        | EventType::PaymentIntentPartiallyFunded
        | EventType::PaymentIntentPaymentFailed
        | EventType::PaymentIntentRequiresAction
        | EventType::PaymentIntentRequiresCapture
        | EventType::PaymentIntentAmountCapturableUpdated
        | EventType::PaymentIntentCreated
        | EventType::PaymentIntentProcessing => {
            if let EventObject::PaymentIntent(payment_intent) = &stripe_event.data.object {
                let payment_status = match PaymentIntentStatus::try_from(stripe_event.type_) {
                    Ok(status) => status,
                    Err(e) => {
                        error!("Encountered Error {e:?} when processing stripe event type");
                        return (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}"))
                            .into_response();
                    }
                };

                info!(
                    "PaymentIntent status update: id={}, status={:?}, amount={:?}",
                    payment_intent.id, payment_status, payment_intent.amount
                );

                // Update database with payment status (if we have a database client)
                // if let Some(db_client) = &app_state.database_client {
                //     if let Err(e) = update_payment_status(
                //         db_client,
                //         &payment_intent.id.to_string(),
                //         payment_status,
                //     )
                //     .await
                //     {
                //         error!("Failed to update payment status in database: {:?}", e);
                //     }
                // }

                // Notify connected clients via WebSocket if we have that service
                if let Some(ws_service) = &app_state.websocket_service {
                    let status_update = serde_json::json!({
                        "type": "payment_status_update",
                        "payment_intent_id": payment_intent.id.to_string(),
                        "status": payment_status.to_string(),
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    if let Err(e) = ws_service
                        .broadcast_message(
                            &payment_intent.id.to_string(),
                            &status_update.to_string(),
                        )
                        .await
                    {
                        error!("Failed to broadcast payment status update: {:?}", e);
                    }
                }
            }
        }
        EventType::PaymentMethodAttached => {
            if let EventObject::PaymentMethod(payment_method) = &stripe_event.data.object {
                info!("PaymentMethod attached: id={}", payment_method.id);
                // Update your records as needed.
            }
        }
        EventType::ChargeSucceeded | EventType::ChargeUpdated => {
            if let EventObject::Charge(charge) = &stripe_event.data.object {
                info!("Charge succeeded: id={}", charge.id);
                // Update your records as needed.
            }
        }
        _ => {
            info!("Unhandled event type: {}", stripe_event.type_);
        }
    }
    (StatusCode::OK, "Webhook received".to_string()).into_response()
}
