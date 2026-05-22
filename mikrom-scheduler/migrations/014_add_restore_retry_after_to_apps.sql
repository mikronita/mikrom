-- Persist a backoff after VM startup failures so router traffic does not retrigger endless restores
ALTER TABLE apps
ADD COLUMN IF NOT EXISTS restore_retry_after_at BIGINT NOT NULL DEFAULT 0;
