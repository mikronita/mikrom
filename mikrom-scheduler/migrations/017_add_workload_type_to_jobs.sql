-- Add workload_type to jobs table
ALTER TABLE jobs ADD COLUMN workload_type INTEGER NOT NULL DEFAULT 0;
