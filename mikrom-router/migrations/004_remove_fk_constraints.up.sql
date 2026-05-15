-- Remove foreign key constraints that cause race conditions between route and certificate updates
ALTER TABLE tls_certificates DROP CONSTRAINT IF EXISTS fk_tls_certificates_routes;
ALTER TABLE acme_challenges DROP CONSTRAINT IF EXISTS fk_acme_challenges_routes;
