-- Persist Neon tenant generation so /re-attach can rebuild the runtime map.
ALTER TABLE databases
    ADD COLUMN tenant_gen INTEGER;

-- Backfill existing provisioned Neon databases with the initial generation.
UPDATE databases
SET tenant_gen = 1
WHERE engine = 'neon'
  AND tenant_id IS NOT NULL
  AND timeline_id IS NOT NULL
  AND tenant_gen IS NULL;

-- Keep the provisioning check aligned with the new field. The tenant generation
-- is only required once provisioning is complete.
ALTER TABLE databases
    DROP CONSTRAINT IF EXISTS databases_neon_provisioning_ids_present;

ALTER TABLE databases
    ADD CONSTRAINT databases_neon_provisioning_ids_present
    CHECK (
        engine <> 'neon'
        OR status = 'pending'
        OR (tenant_id IS NOT NULL AND timeline_id IS NOT NULL AND tenant_gen IS NOT NULL)
    )
    NOT VALID;
