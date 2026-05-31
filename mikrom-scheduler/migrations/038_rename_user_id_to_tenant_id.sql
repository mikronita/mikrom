-- Rename user_id to tenant_id in jobs and apps tables
ALTER TABLE jobs RENAME COLUMN user_id TO tenant_id;
DROP INDEX IF EXISTS idx_jobs_user_id;
CREATE INDEX IF NOT EXISTS idx_jobs_tenant_id ON jobs(tenant_id);

ALTER TABLE apps RENAME COLUMN user_id TO tenant_id;
DROP INDEX IF EXISTS idx_apps_user_id;
CREATE INDEX IF NOT EXISTS idx_apps_tenant_id ON apps(tenant_id);
