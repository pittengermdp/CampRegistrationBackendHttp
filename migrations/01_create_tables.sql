-- Migration to create tables for the WebSocket connections and payment events

-- Create extension for UUID support
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create websocket_connections table
CREATE TABLE IF NOT EXISTS websocket_connections (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    payment_intent_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    customer_id TEXT,
    customer_email TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    UNIQUE (connection_id)
);

-- Create index on payment_intent_id to speed up lookup
CREATE INDEX IF NOT EXISTS idx_websocket_connections_payment_intent_id ON websocket_connections(payment_intent_id);
CREATE INDEX IF NOT EXISTS idx_websocket_connections_status ON websocket_connections(status);

-- Create payment_events table
CREATE TABLE IF NOT EXISTS payment_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    payment_intent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    amount BIGINT,
    currency TEXT,
    customer_id TEXT,
    metadata JSONB,
    UNIQUE (payment_intent_id, status, created_at)
);

-- Create index on payment_intent_id to speed up lookup
CREATE INDEX IF NOT EXISTS idx_payment_events_payment_intent_id ON payment_events(payment_intent_id);