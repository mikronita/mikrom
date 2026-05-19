-- Add access_mode to volumes table
-- 0: ReadWriteOnce, 1: ReadWriteMany, 2: ReadOnlyMany
ALTER TABLE volumes ADD COLUMN access_mode INTEGER NOT NULL DEFAULT 0;
