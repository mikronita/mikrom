DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'deployments' AND column_name = 'hypervisor'
    ) THEN
        ALTER TABLE deployments
        ADD COLUMN hypervisor INTEGER NOT NULL DEFAULT 0;
    END IF;
END $$;
