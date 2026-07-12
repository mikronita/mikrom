-- Restore the route-backed integrity model after seeding the permanent hosts
-- that are managed by mikrom-api rather than by dynamic app routing.

INSERT INTO routes (hostname, target_url, updated_at)
VALUES
    ('api.mikrom.spluca.org', '192.168.122.128:5001', NOW()),
    ('mikrom.spluca.org', '192.168.122.128:5173', NOW())
ON CONFLICT (hostname, target_url) DO UPDATE
SET updated_at = EXCLUDED.updated_at;

DELETE FROM acme_challenges
WHERE hostname IS NULL;

ALTER TABLE acme_challenges
    ALTER COLUMN hostname SET NOT NULL;
