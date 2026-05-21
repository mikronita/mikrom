-- Create app_volumes junction table for many-to-many relationship
CREATE TABLE app_volumes (
    app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    volume_id UUID NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    mount_point VARCHAR(255) NOT NULL DEFAULT '/data',
    access_mode INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (app_id, volume_id)
);

-- Migrate existing volume associations to the junction table
INSERT INTO app_volumes (app_id, volume_id, mount_point, access_mode)
SELECT app_id, id, mount_point, access_mode FROM volumes;

-- Decouple volumes from specific apps
-- 1. Remove the foreign key constraint
ALTER TABLE volumes DROP CONSTRAINT IF EXISTS volumes_app_id_fkey;
-- 2. Drop the app_id column
ALTER TABLE volumes DROP COLUMN app_id;
-- 3. Drop attachment-specific columns that moved to the junction table
ALTER TABLE volumes DROP COLUMN mount_point;
ALTER TABLE volumes DROP COLUMN access_mode;

-- Re-create index for the junction table if needed (primary key already covers app_id, volume_id)
CREATE INDEX idx_app_volumes_volume_id ON app_volumes(volume_id);
