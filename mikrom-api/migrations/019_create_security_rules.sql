-- Create security_rules table
CREATE TABLE security_rules (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_id UUID NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    protocol VARCHAR(10) NOT NULL, -- 'tcp', 'udp', 'any'
    port_start INTEGER NOT NULL,
    port_end INTEGER NOT NULL,
    action VARCHAR(10) NOT NULL, -- 'allow', 'deny'
    priority INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for fast lookup by app
CREATE INDEX idx_security_rules_app_id ON security_rules(app_id);
