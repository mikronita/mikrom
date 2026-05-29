-- Ensure Neon databases always have provisioning identifiers.
-- `NOT VALID` avoids failing the migration on any legacy rows; new
-- writes are still checked.
ALTER TABLE databases
    ADD CONSTRAINT databases_neon_provisioning_ids_present
    CHECK (
        engine <> 'neon'
        OR (tenant_id IS NOT NULL AND timeline_id IS NOT NULL)
    )
    NOT VALID;
