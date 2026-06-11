-- Normalize ACME challenge persistence so it no longer depends on route FK state.
-- This keeps older databases working and matches the current router handler.

ALTER TABLE acme_challenges DROP CONSTRAINT IF EXISTS fk_acme_challenges_routes;
ALTER TABLE tls_certificates DROP CONSTRAINT IF EXISTS fk_tls_certificates_routes;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_name = 'acme_challenges'
          AND column_name = 'hostname'
    ) THEN
        ALTER TABLE acme_challenges ADD COLUMN hostname VARCHAR;
    END IF;
END;
$$;

ALTER TABLE acme_challenges ALTER COLUMN hostname DROP NOT NULL;

ALTER TABLE acme_challenges
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
