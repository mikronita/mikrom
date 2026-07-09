-- Add email notifications and marketing emails preferences to users table
ALTER TABLE users ADD COLUMN IF NOT EXISTS email_notifications BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS marketing_emails BOOLEAN NOT NULL DEFAULT FALSE;
