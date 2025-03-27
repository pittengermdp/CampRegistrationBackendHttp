use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Queryable, Debug, Serialize, Deserialize)]
#[diesel(table_name = crate::database::schema::websocket_connections)]
pub struct WebSocketConnection {
    pub id: Uuid,
    pub payment_intent_id: String,
    pub connection_id: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub customer_id: Option<String>,
    pub customer_email: Option<String>,
    pub status: String,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = crate::database::schema::websocket_connections)]
pub struct NewWebSocketConnection {
    pub id: Uuid,
    pub payment_intent_id: String,
    pub connection_id: String,
    pub customer_id: Option<String>,
    pub customer_email: Option<String>,
    pub status: String,
}

impl WebSocketConnection {
    pub fn new(
        payment_intent_id: String,
        connection_id: String,
        customer_id: Option<String>,
        customer_email: Option<String>,
    ) -> NewWebSocketConnection {
        NewWebSocketConnection {
            id: Uuid::new_v4(),
            payment_intent_id,
            connection_id,
            customer_id,
            customer_email,
            status: "active".to_string(),
        }
    }
}

#[derive(Queryable, Debug, Serialize, Deserialize)]
#[diesel(table_name = crate::database::schema::payment_events)]
pub struct PaymentEvent {
    pub id: Uuid,
    pub payment_intent_id: String,
    pub status: String,
    pub created_at: NaiveDateTime,
    pub amount: Option<i64>,
    pub currency: Option<String>,
    pub customer_id: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = crate::database::schema::payment_events)]
pub struct NewPaymentEvent {
    pub id: Uuid,
    pub payment_intent_id: String,
    pub status: String,
    pub amount: Option<i64>,
    pub currency: Option<String>,
    pub customer_id: Option<String>,
    pub metadata: Option<Value>,
}

impl PaymentEvent {
    pub fn new(
        payment_intent_id: String,
        status: String,
        amount: Option<i64>,
        currency: Option<String>,
        customer_id: Option<String>,
        metadata: Option<Value>,
    ) -> NewPaymentEvent {
        NewPaymentEvent {
            id: Uuid::new_v4(),
            payment_intent_id,
            status,
            amount,
            currency,
            customer_id,
            metadata,
        }
    }
}
