-- Allow Neon databases to be created in a pending state before tenant/timeline provisioning finishes.
ALTER TABLE databases
    DROP CONSTRAINT IF EXISTS databases_neon_provisioning_ids_present;

ALTER TABLE databases
    ADD CONSTRAINT databases_neon_provisioning_ids_present
    CHECK (
        engine <> 'neon'
        OR status = 'pending'
        OR (tenant_id IS NOT NULL AND timeline_id IS NOT NULL)
    )
    NOT VALID;
