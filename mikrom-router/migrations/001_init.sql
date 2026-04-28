-- Routing table for persistence
CREATE TABLE IF NOT EXISTS routes (
    hostname VARCHAR PRIMARY KEY,
    target_url VARCHAR NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
