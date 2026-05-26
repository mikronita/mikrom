ALTER TABLE deployments
ALTER COLUMN hypervisor TYPE INTEGER
USING hypervisor::INTEGER;

ALTER TABLE deployments
ALTER COLUMN hypervisor SET DEFAULT 0;

ALTER TABLE deployments
ALTER COLUMN hypervisor SET NOT NULL;
