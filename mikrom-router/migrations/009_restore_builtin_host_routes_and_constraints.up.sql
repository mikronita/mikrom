-- Restore the route-backed integrity model after seeding the permanent hosts
-- that are managed by mikrom-api rather than by dynamic app routing.

INSERT INTO routes (hostname, target_url, updated_at)
VALUES
    ('api.mikrom.spluca.org', '192.168.122.128:5001', NOW()),
    ('mikrom.spluca.org', '192.168.122.128:5173', NOW())
ON CONFLICT (hostname) DO UPDATE
SET target_url = EXCLUDED.target_url,
    updated_at = EXCLUDED.updated_at;

DELETE FROM acme_challenges
WHERE hostname IS NULL;

ALTER TABLE acme_challenges
    ALTER COLUMN hostname SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'fk_tls_certificates_routes'
    ) THEN
        ALTER TABLE tls_certificates
            ADD CONSTRAINT fk_tls_certificates_routes
            FOREIGN KEY (hostname) REFERENCES routes(hostname)
            ON DELETE CASCADE;
    END IF;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'fk_acme_challenges_routes'
    ) THEN
        ALTER TABLE acme_challenges
            ADD CONSTRAINT fk_acme_challenges_routes
            FOREIGN KEY (hostname) REFERENCES routes(hostname)
            ON DELETE CASCADE;
    END IF;
END;
$$;
