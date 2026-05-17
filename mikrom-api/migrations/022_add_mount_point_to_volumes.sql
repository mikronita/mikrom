-- Add mount_point to volumes table
ALTER TABLE volumes ADD COLUMN mount_point VARCHAR(255) NOT NULL DEFAULT '/data';
