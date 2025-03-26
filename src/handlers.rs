use axum::response::IntoResponse;
use axum::{http::StatusCode, Extension};
use lambda_lib::{AppState, PaymentSheetRequest};
use serde_json::{json, Value};
use stripe::{
    Client, CreateCustomer, CreateEphemeralKey, CreatePaymentIntent,
    CreatePaymentIntentAutomaticPaymentMethods, Currency, Customer, EphemeralKey, PaymentIntent,
};
use tracing::{error, info};

/// POST /payment_sheet endpoint creates a Customer, an Ephemeral Key, and a PaymentIntent with automatic payment methods enabled.
#[tracing::instrument(skip(state))]
pub async fn create_payment_sheet_handler(
    axum::extract::Extension(state): axum::extract::Extension<AppState>,
    axum::extract::Json(payload): axum::extract::Json<PaymentSheetRequest>,
) -> Result<axum::Json<Value>, (StatusCode, String)> {
    info!("Received payment sheet request: {:?}", payload);
    let client = Client::new(state.stripe_keys.secret_key.clone());

    // 1. Create a Customer.
    let customer = Customer::create(
        &client,
        CreateCustomer {
            name: Some(&payload.customer_name),
            email: Some(&payload.customer_email),
            description: payload.customer_description.as_deref(),
            metadata: Some(std::collections::HashMap::from([(
                "async-stripe".to_string(),
                "true".to_string(),
            )])),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| {
        error!("Error creating customer: {e:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Error creating customer: {e:?}"),
        )
    })?;
    info!("Created customer with id: {}", customer.id);

    // 2. Create an Ephemeral Key.
    let ephemeral_key = EphemeralKey::create(
        &client,
        CreateEphemeralKey {
            customer: Some(customer.id.clone()),
            ..Default::default()
        },
    )
    .await
    .map_err(|e| {
        error!("Error creating ephemeral key: {e:?}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Error creating ephemeral key: {e:?}"),
        )
    })?;
    info!("Created ephemeral key");

    // 3. Create a PaymentIntent with automatic payment methods enabled.
    let currency = match payload.currency.to_lowercase().as_str() {
        "usd" => Currency::USD,
        "eur" => Currency::EUR,
        other => {
            error!("Unsupported currency: {other}");
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Unsupported currency: {other}"),
            ));
        }
    };

    let mut create_intent = CreatePaymentIntent::new(payload.amount, currency);
    create_intent.customer = Some(customer.id.clone());
    create_intent.automatic_payment_methods = Some(CreatePaymentIntentAutomaticPaymentMethods {
        allow_redirects: None,
        enabled: true,
    });
    if let Some(meta_obj) = payload.metadata.as_object() {
        let meta_map = meta_obj
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect();
        create_intent.metadata = Some(meta_map);
    }

    let payment_intent = PaymentIntent::create(&client, create_intent)
        .await
        .map_err(|e| {
            error!("Error creating payment intent: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error creating payment intent: {e:?}"),
            )
        })?;
    info!("Created PaymentIntent with id: {}", payment_intent.id);

    let body = json!({
        "customer": customer.id,
        "ephemeralKey": ephemeral_key.secret,
        "paymentIntent": payment_intent.client_secret,
        "publishableKey": state.stripe_keys.publishable_key,
    });

    Ok(axum::Json(body))
}

/// GET /hello endpoint returns a simple text message.
#[tracing::instrument]
pub async fn hello_handler() -> impl IntoResponse {
    info!("Handling hello request");
    "Hello, world!"
}

/// GET /stripe endpoint retrieves the Stripe publishable key.
#[tracing::instrument(skip(state))]
pub async fn stripe_handler(
    Extension(state): Extension<AppState>,
) -> Result<axum::Json<Value>, (StatusCode, String)> {
    info!("Handling stripe endpoint request");
    let body = json!({ "publishable_key": state.stripe_keys.publishable_key });
    Ok(axum::Json(body))
}
