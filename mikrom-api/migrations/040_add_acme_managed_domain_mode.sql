-- Track the desired ACME environment and reissuance state for managed hostnames.
ALTER TABLE acme_managed_domains
    ADD COLUMN IF NOT EXISTS is_staging BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE acme_managed_domains
    ADD COLUMN IF NOT EXISTS needs_reissue BOOLEAN NOT NULL DEFAULT FALSE;
