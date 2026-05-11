-- TLS Certificates and ACME state for stateless routing
CREATE TABLE IF NOT EXISTS tls_certificates (
    hostname VARCHAR PRIMARY KEY,
    cert_chain TEXT NOT NULL,
    private_key TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS acme_challenges (
    token VARCHAR PRIMARY KEY,
    key_auth TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for expiration monitoring
CREATE INDEX IF NOT EXISTS idx_tls_certificates_expires_at ON tls_certificates(expires_at);
