-- Add Git metadata and trigger source to deployments table
ALTER TABLE deployments
ADD COLUMN git_commit_hash VARCHAR(40),
ADD COLUMN git_commit_message TEXT,
ADD COLUMN git_branch VARCHAR(255),
ADD COLUMN trigger_source VARCHAR(50) DEFAULT 'manual';
