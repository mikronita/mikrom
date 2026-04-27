-- Migration: 013_add_deployment_id_to_jobs.sql
-- Description: Add deployment_id column to jobs table for frontend synchronization

ALTER TABLE jobs ADD COLUMN deployment_id VARCHAR;
CREATE INDEX IF NOT EXISTS idx_jobs_deployment_id ON jobs(deployment_id);
