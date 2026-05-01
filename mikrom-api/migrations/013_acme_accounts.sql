-- Table to store persistent ACME accounts (Let's Encrypt)
CREATE TABLE IF NOT EXISTS acme_accounts (
    id SERIAL PRIMARY KEY,
    email VARCHAR NOT NULL UNIQUE,
    credentials_json TEXT NOT NULL,
    is_staging BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index to quickly find account by email and staging flag
CREATE UNIQUE INDEX IF NOT EXISTS idx_acme_accounts_email_staging ON acme_accounts(email, is_staging);
