-- Add health check path and drain timeout to apps table
ALTER TABLE apps ADD COLUMN health_check_path TEXT DEFAULT '/';
ALTER TABLE apps ADD COLUMN drain_timeout INTEGER DEFAULT 10;
