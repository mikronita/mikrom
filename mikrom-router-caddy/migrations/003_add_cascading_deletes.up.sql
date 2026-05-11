-- Add foreign key constraints with CASCADE to clean up certificates and challenges
-- when a route is deleted.

-- 1. For tls_certificates: hostname is the primary key and corresponds to routes.hostname
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_tls_certificates_routes') THEN
        ALTER TABLE tls_certificates
        ADD CONSTRAINT fk_tls_certificates_routes
        FOREIGN KEY (hostname) REFERENCES routes(hostname)
        ON DELETE CASCADE;
    END IF;
END;
$$;

-- 2. For acme_challenges: We need to associate them with a hostname to allow cascading.
-- First, let's add a hostname column to acme_challenges if it doesn't exist.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = 'acme_challenges' AND COLUMN_NAME = 'hostname') THEN
        ALTER TABLE acme_challenges
        ADD COLUMN hostname VARCHAR NOT NULL;
    END IF;
END;
$$;

-- Add constraint for acme_challenges
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_acme_challenges_routes') THEN
        ALTER TABLE acme_challenges
        ADD CONSTRAINT fk_acme_challenges_routes
        FOREIGN KEY (hostname) REFERENCES routes(hostname)
        ON DELETE CASCADE;
    END IF;
END;
$$;
