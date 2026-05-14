-- Create volumes table
CREATE TABLE volumes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    size_mib INTEGER NOT NULL,
    pool_name VARCHAR(255) NOT NULL DEFAULT 'mikrom_volumes',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create volume_snapshots table
CREATE TABLE volume_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    volume_id UUID NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add indices
CREATE INDEX idx_volumes_app_id ON volumes(app_id);
CREATE INDEX idx_volumes_user_id ON volumes(user_id);
CREATE INDEX idx_volume_snapshots_volume_id ON volume_snapshots(volume_id);
CREATE INDEX idx_volume_snapshots_user_id ON volume_snapshots(user_id);
