-- Create databases table
CREATE TABLE databases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    engine VARCHAR(50) NOT NULL DEFAULT 'neon',
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    vcpus INTEGER NOT NULL DEFAULT 1,
    memory_mib INTEGER NOT NULL DEFAULT 512,
    disk_mib INTEGER NOT NULL DEFAULT 1024,
    settings JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(50) NOT NULL DEFAULT 'PENDING',
    active_deployment_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT unique_db_name_per_user UNIQUE (user_id, name)
);

-- Create database_deployments table
CREATE TABLE database_deployments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    database_id UUID NOT NULL REFERENCES databases(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    job_id VARCHAR(255), -- Reference to scheduler job
    status VARCHAR(50) NOT NULL DEFAULT 'PENDING',
    host_id VARCHAR(255),
    vm_id VARCHAR(255),
    ipv6_address VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add index for performance
CREATE INDEX idx_databases_user_id ON databases(user_id);
CREATE INDEX idx_database_deployments_db_id ON database_deployments(database_id);
CREATE INDEX idx_database_deployments_user_id ON database_deployments(user_id);

-- Add foreign key to databases table for active_deployment_id
ALTER TABLE databases ADD CONSTRAINT fk_databases_active_deployment 
    FOREIGN KEY (active_deployment_id) REFERENCES database_deployments(id) ON DELETE SET NULL;
