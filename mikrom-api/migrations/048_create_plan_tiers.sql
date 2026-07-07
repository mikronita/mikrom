CREATE TABLE plan_tiers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    polar_product_id VARCHAR(128) UNIQUE,
    tier_slug VARCHAR(64) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    max_apps INT NOT NULL DEFAULT 1,
    max_databases INT NOT NULL DEFAULT 0,
    max_volumes INT NOT NULL DEFAULT 0,
    max_vcpus_total INT NOT NULL DEFAULT 1,
    max_memory_mb_total INT NOT NULL DEFAULT 512,
    max_storage_gb_total INT NOT NULL DEFAULT 1,
    max_deployments_per_app INT NOT NULL DEFAULT 1,
    max_team_members INT NOT NULL DEFAULT 1,
    autoscaling_allowed BOOLEAN NOT NULL DEFAULT FALSE,
    custom_domains BOOLEAN NOT NULL DEFAULT FALSE,
    trial_days INT NOT NULL DEFAULT 0,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    sort_order INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO plan_tiers (tier_slug, name, max_apps, max_databases, max_volumes, max_vcpus_total, max_memory_mb_total, max_storage_gb_total, max_deployments_per_app, max_team_members, autoscaling_allowed, custom_domains, trial_days, is_default, sort_order) VALUES
    ('free', 'Free', 1, 0, 0, 1, 512, 1, 1, 1, FALSE, FALSE, 0, FALSE, 1),
    ('hobby', 'Hobby', 3, 1, 1, 2, 1024, 5, 2, 3, TRUE, FALSE, 14, TRUE, 2),
    ('pro', 'Pro', 10, 3, 5, 4, 4096, 20, 5, 10, TRUE, TRUE, 0, FALSE, 3),
    ('enterprise', 'Enterprise', 100, 50, 50, 16, 16384, 500, 20, 100, TRUE, TRUE, 0, FALSE, 4);
