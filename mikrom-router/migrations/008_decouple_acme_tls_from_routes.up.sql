-- ACME/TLS state should not depend on routing rows.
-- This makes certificate and challenge persistence work even when a hostname
-- has not yet been seeded into routes.

ALTER TABLE tls_certificates DROP CONSTRAINT IF EXISTS fk_tls_certificates_routes;
ALTER TABLE acme_challenges DROP CONSTRAINT IF EXISTS fk_acme_challenges_routes;
ALTER TABLE acme_challenges ALTER COLUMN hostname DROP NOT NULL;
