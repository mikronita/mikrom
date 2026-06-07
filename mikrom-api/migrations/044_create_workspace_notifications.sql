CREATE TABLE IF NOT EXISTS workspace_notifications (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    route TEXT NOT NULL,
    entity_name TEXT,
    resource_id TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    read_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_workspace_notifications_user_created_at
    ON workspace_notifications (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_workspace_notifications_user_unread
    ON workspace_notifications (user_id, read_at);

