-- Add desired_replicas to apps table for manual scaling reconciliation
ALTER TABLE apps ADD COLUMN desired_replicas INTEGER NOT NULL DEFAULT 1;
