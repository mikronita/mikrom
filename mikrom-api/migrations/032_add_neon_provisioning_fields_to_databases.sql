-- Store Neon provisioning identifiers explicitly on databases.
ALTER TABLE databases
    ADD COLUMN tenant_id VARCHAR(255),
    ADD COLUMN timeline_id VARCHAR(255);
