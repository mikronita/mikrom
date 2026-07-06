CREATE TABLE polar_webhook_deliveries (
    webhook_id VARCHAR(255) PRIMARY KEY,
    event_type VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processed',
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
