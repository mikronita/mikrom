-- Add github_webhook_secret column to apps table
ALTER TABLE apps ADD COLUMN github_webhook_secret VARCHAR(255);
