-- Track router activity and preserve the last scaled-to-zero timestamp
ALTER TABLE apps
ADD COLUMN IF NOT EXISTS hostname VARCHAR NOT NULL DEFAULT '',
ADD COLUMN IF NOT EXISTS last_router_traffic_at BIGINT NOT NULL DEFAULT 0,
ADD COLUMN IF NOT EXISTS last_scaled_to_zero_at BIGINT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_apps_hostname ON apps(hostname);
