CREATE TABLE tenant_usage (
    tenant_id UUID PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
    apps_count INT NOT NULL DEFAULT 0,
    databases_count INT NOT NULL DEFAULT 0,
    volumes_count INT NOT NULL DEFAULT 0,
    vcpus_total INT NOT NULL DEFAULT 0,
    memory_mb_total INT NOT NULL DEFAULT 0,
    storage_gb_total INT NOT NULL DEFAULT 0,
    deployments_count INT NOT NULL DEFAULT 0,
    bandwidth_gb_billed INT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE tenant_usage_history (
    id BIGSERIAL PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    vcpu_seconds BIGINT NOT NULL DEFAULT 0,
    ram_mb_seconds BIGINT NOT NULL DEFAULT 0,
    storage_gb_seconds BIGINT NOT NULL DEFAULT 0,
    bandwidth_gb BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX idx_tenant_usage_history_tenant_period
    ON tenant_usage_history(tenant_id, period_start DESC);
