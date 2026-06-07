CREATE TABLE tenant_billing (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    polar_customer_id VARCHAR(128),
    polar_subscription_id VARCHAR(128),
    polar_product_id VARCHAR(128),
    plan_name VARCHAR(255),
    status VARCHAR(32) NOT NULL DEFAULT 'none',
    amount_cents INTEGER,
    currency VARCHAR(16),
    current_period_start TIMESTAMPTZ,
    current_period_end TIMESTAMPTZ,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_tenant_billing_polar_customer_id
    ON tenant_billing(polar_customer_id)
    WHERE polar_customer_id IS NOT NULL;

CREATE UNIQUE INDEX idx_tenant_billing_polar_subscription_id
    ON tenant_billing(polar_subscription_id)
    WHERE polar_subscription_id IS NOT NULL;
