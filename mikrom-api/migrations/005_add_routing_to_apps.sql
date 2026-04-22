-- Add routing fields to apps
ALTER TABLE apps ADD COLUMN port INT NOT NULL DEFAULT 80;
ALTER TABLE apps ADD COLUMN hostname VARCHAR(255);

-- Create a unique index for hostname to prevent collisions
CREATE UNIQUE INDEX idx_apps_hostname ON apps(hostname) WHERE hostname IS NOT NULL;
