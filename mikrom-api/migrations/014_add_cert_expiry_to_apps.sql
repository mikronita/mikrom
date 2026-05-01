-- Add certificate expiration tracking to apps
ALTER TABLE apps ADD COLUMN cert_expires_at TIMESTAMPTZ;

-- Add index to speed up ACME worker queries
CREATE INDEX idx_apps_cert_expires_at ON apps(cert_expires_at);
