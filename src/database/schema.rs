use diesel::table;

// Defines database schema for diesel to use
table! {
    websocket_connections (id) {
        id -> Uuid,
        payment_intent_id -> Text,
        connection_id -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        customer_id -> Nullable<Text>,
        customer_email -> Nullable<Text>,
        status -> Text,
    }
}

table! {
    payment_events (id) {
        id -> Uuid,
        payment_intent_id -> Text,
        status -> Text,
        created_at -> Timestamp,
        amount -> Nullable<Int8>,
        currency -> Nullable<Text>,
        customer_id -> Nullable<Text>,
        metadata -> Nullable<Json>,
    }
}
