-- Align deployment defaults with the supported resource presets
ALTER TABLE deployments ALTER COLUMN memory_mib SET DEFAULT 512;
