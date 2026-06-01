-- Add vpc_ipv6_prefix to apps table for tenant-scoped IPv6 routing metadata
ALTER TABLE apps ADD COLUMN vpc_ipv6_prefix VARCHAR NOT NULL DEFAULT '';
