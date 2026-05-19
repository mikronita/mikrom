-- Add vpc_ipv6_prefix to apps table for scaling IPAM
ALTER TABLE apps ADD COLUMN vpc_ipv6_prefix VARCHAR NOT NULL DEFAULT '';
