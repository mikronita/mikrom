-- Cache for application scaling configuration
CREATE TABLE IF NOT EXISTS apps (
    id VARCHAR PRIMARY KEY,
    user_id VARCHAR NOT NULL,
    min_replicas INTEGER NOT NULL DEFAULT 1,
    max_replicas INTEGER NOT NULL DEFAULT 1,
    autoscaling_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    cpu_threshold FLOAT NOT NULL DEFAULT 80.0,
    mem_threshold FLOAT NOT NULL DEFAULT 80.0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_apps_user_id ON apps(user_id);
