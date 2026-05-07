-- Add 6PN metadata to users and deployments
ALTER TABLE users ADD COLUMN vpc_ipv6_prefix VARCHAR(45);
ALTER TABLE deployments ADD COLUMN ipv6_address VARCHAR(45);

-- Create workers table to track agent nodes and their WireGuard keys
CREATE TABLE workers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    hostname VARCHAR(255) NOT NULL,
    ip_address VARCHAR(45) NOT NULL,
    wireguard_pubkey VARCHAR(255) NOT NULL,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for performance
CREATE INDEX idx_workers_wireguard_pubkey ON workers(wireguard_pubkey);
