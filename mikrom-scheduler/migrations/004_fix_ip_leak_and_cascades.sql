-- Migration to fix IP leaks and add cascading deletes for IP allocations
-- We link ip_allocations to jobs so that when a job is deleted, its IP is automatically released.

-- 1. Add job_id column to ip_allocations
ALTER TABLE ip_allocations ADD COLUMN job_id VARCHAR;

-- 2. Add foreign key constraint with CASCADE
-- We use job_id as the primary link for cleanup.
ALTER TABLE ip_allocations
ADD CONSTRAINT fk_ip_allocations_jobs
FOREIGN KEY (job_id) REFERENCES jobs(job_id)
ON DELETE CASCADE;

-- 3. Add index for performance
CREATE INDEX IF NOT EXISTS idx_ip_allocations_job_id ON ip_allocations(job_id);
