-- Add missing WireGuard metadata to workers table
ALTER TABLE workers ADD COLUMN wireguard_ip TEXT;
ALTER TABLE workers ADD COLUMN wireguard_port INTEGER DEFAULT 51820;
