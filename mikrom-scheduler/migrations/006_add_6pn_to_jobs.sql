-- Add 6PN support to jobs and workers
ALTER TABLE jobs ADD COLUMN ipv6_address VARCHAR(45);
ALTER TABLE jobs ADD COLUMN ipv6_gateway VARCHAR(45);
ALTER TABLE workers ADD COLUMN wireguard_pubkey VARCHAR(255);
