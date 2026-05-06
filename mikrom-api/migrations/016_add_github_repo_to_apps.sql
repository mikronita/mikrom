-- Add GitHub repository metadata to apps table
ALTER TABLE apps ADD COLUMN github_installation_id BIGINT;
ALTER TABLE apps ADD COLUMN github_repo_id BIGINT;
ALTER TABLE apps ADD COLUMN github_repo_full_name VARCHAR(255);
