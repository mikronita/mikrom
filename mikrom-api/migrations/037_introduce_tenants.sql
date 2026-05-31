-- 037_introduce_tenants.sql
-- Step 1: Create tenants table
CREATE TABLE tenants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id VARCHAR(6) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Step 2: Create tenant_members table
CREATE TABLE tenant_members (
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(50) NOT NULL DEFAULT 'admin',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, user_id)
);

-- Step 3: Helper function for alphanumeric generation
CREATE OR REPLACE FUNCTION generate_tenant_id() RETURNS TEXT AS $$
DECLARE
    chars TEXT := 'abcdefghijklmnopqrstuvwxyz0123456789';
    result TEXT := '';
BEGIN
    FOR i IN 1..6 LOOP
        result := result || substr(chars, floor(random() * length(chars) + 1)::integer, 1);
    END LOOP;
    RETURN result;
END;
$$ LANGUAGE plpgsql;

-- Step 4: Migrate existing users to a default project
DO $$
DECLARE
    user_record RECORD;
    new_tenant_id UUID;
    slug TEXT;
BEGIN
    FOR user_record IN SELECT id FROM users LOOP
        -- Generate a unique slug
        LOOP
            slug := generate_tenant_id();
            EXIT WHEN NOT EXISTS (SELECT 1 FROM tenants WHERE tenant_id = slug);
        END LOOP;

        INSERT INTO tenants (tenant_id, name)
        VALUES (slug, 'Default Project')
        RETURNING id INTO new_tenant_id;

        INSERT INTO tenant_members (tenant_id, user_id, role)
        VALUES (new_tenant_id, user_record.id, 'admin');
    END LOOP;
END $$;

-- Step 5: Add tenant_id to resource tables
-- Rename existing Neon tenant_id to avoid conflict
ALTER TABLE databases RENAME COLUMN tenant_id TO neon_tenant_id;
ALTER TABLE databases RENAME COLUMN timeline_id TO neon_timeline_id;

ALTER TABLE apps ADD COLUMN tenant_id UUID REFERENCES tenants(id);
ALTER TABLE deployments ADD COLUMN tenant_id UUID REFERENCES tenants(id);
ALTER TABLE volumes ADD COLUMN tenant_id UUID REFERENCES tenants(id);
ALTER TABLE volume_snapshots ADD COLUMN tenant_id UUID REFERENCES tenants(id);
ALTER TABLE databases ADD COLUMN tenant_id UUID REFERENCES tenants(id);
ALTER TABLE database_deployments ADD COLUMN tenant_id UUID REFERENCES tenants(id);

-- Step 6: Map existing resources to their owner's default tenant
UPDATE apps a SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE a.user_id = tm.user_id;
UPDATE deployments d SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE d.user_id = tm.user_id;
UPDATE volumes v SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE v.user_id = tm.user_id;
UPDATE volume_snapshots vs SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE vs.user_id = tm.user_id;
UPDATE databases db SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE db.user_id = tm.user_id;
UPDATE database_deployments dd SET tenant_id = tm.tenant_id FROM tenant_members tm WHERE dd.user_id = tm.user_id;

-- Step 7: Clean up
DROP FUNCTION generate_tenant_id();

-- Indices for performance
CREATE INDEX idx_tenants_tenant_id ON tenants(tenant_id);
CREATE INDEX idx_apps_tenant_id ON apps(tenant_id);
CREATE INDEX idx_databases_tenant_id ON databases(tenant_id);
CREATE INDEX idx_volumes_tenant_id ON volumes(tenant_id);
