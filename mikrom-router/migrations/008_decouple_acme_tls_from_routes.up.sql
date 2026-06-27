-- Temporarily decouple ACME/TLS state from routing rows so older databases
-- can accept challenge/certificate updates before permanent host routes are
-- seeded again in the next migration.

ALTER TABLE tls_certificates DROP CONSTRAINT IF EXISTS fk_tls_certificates_routes;
ALTER TABLE acme_challenges DROP CONSTRAINT IF EXISTS fk_acme_challenges_routes;
ALTER TABLE acme_challenges ALTER COLUMN hostname DROP NOT NULL;
