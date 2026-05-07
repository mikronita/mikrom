-- Add health_check_path to jobs table
ALTER TABLE jobs ADD COLUMN health_check_path TEXT DEFAULT '/';
