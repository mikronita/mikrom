CREATE TABLE polar_billing_products (
    product_id VARCHAR(128) PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    price_amount_cents INTEGER,
    currency VARCHAR(16),
    recurring_interval VARCHAR(32),
    is_archived BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
