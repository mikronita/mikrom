-- Remove IPv4 address from deployments table
ALTER TABLE deployments DROP COLUMN IF EXISTS ip_address;
