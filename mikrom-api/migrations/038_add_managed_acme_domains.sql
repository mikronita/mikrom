-- Track non-app hostnames that are managed by the ACME worker.
CREATE TABLE IF NOT EXISTS acme_managed_domains (
    hostname VARCHAR PRIMARY KEY,
    cert_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_acme_managed_domains_expires_at
    ON acme_managed_domains(cert_expires_at);
