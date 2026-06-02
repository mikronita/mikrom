-- Allow one ACME account per email and ACME environment.
-- The worker needs separate staging and production accounts for the same contact email.
ALTER TABLE acme_accounts
    DROP CONSTRAINT IF EXISTS acme_accounts_email_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_acme_accounts_email_staging
    ON acme_accounts(email, is_staging);
