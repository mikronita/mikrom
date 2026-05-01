-- Add foreign key constraints with CASCADE to clean up certificates and challenges
-- when a route is deleted.

-- 1. For tls_certificates: hostname is the primary key and corresponds to routes.hostname
ALTER TABLE tls_certificates
ADD CONSTRAINT fk_tls_certificates_routes
FOREIGN KEY (hostname) REFERENCES routes(hostname)
ON DELETE CASCADE;

-- 2. For acme_challenges: We need to associate them with a hostname to allow cascading.
-- First, let's add a hostname column to acme_challenges.
ALTER TABLE acme_challenges
ADD COLUMN hostname VARCHAR;

-- Update existing challenges if any (though unlikely to be many)
-- This is a bit tricky as we don't have the hostname in acme_challenges yet.
-- Since they are transient, we can just clear them or leave them if they are few.

ALTER TABLE acme_challenges
ADD CONSTRAINT fk_acme_challenges_routes
FOREIGN KEY (hostname) REFERENCES routes(hostname)
ON DELETE CASCADE;
