-- Persist the PostgreSQL major version for user-facing database views.
ALTER TABLE databases
    ADD COLUMN IF NOT EXISTS postgres_version INTEGER NOT NULL DEFAULT 16;
