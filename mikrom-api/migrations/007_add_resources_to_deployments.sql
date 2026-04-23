-- Add resource configuration to deployments for persistence and background resume
ALTER TABLE deployments ADD COLUMN vcpus INT NOT NULL DEFAULT 1;
ALTER TABLE deployments ADD COLUMN memory_mib BIGINT NOT NULL DEFAULT 256;
ALTER TABLE deployments ADD COLUMN disk_mib BIGINT NOT NULL DEFAULT 1024;
ALTER TABLE deployments ADD COLUMN env_vars JSONB NOT NULL DEFAULT '{}'::jsonb;
