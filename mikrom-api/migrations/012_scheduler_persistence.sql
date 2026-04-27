-- Migration: 012_scheduler_persistence.sql
-- Description: Tables for persistent scheduler state (Workers, IPAM, and Jobs)

CREATE TABLE IF NOT EXISTS workers (
    id VARCHAR PRIMARY KEY,
    hostname VARCHAR NOT NULL,
    ip_address VARCHAR NOT NULL,
    agent_port INTEGER NOT NULL,
    bridge_ip VARCHAR NOT NULL,
    status VARCHAR NOT NULL DEFAULT 'Online',
    metrics JSONB,
    last_heartbeat BIGINT NOT NULL,
    registered_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS ip_allocations (
    ip_address VARCHAR PRIMARY KEY,
    worker_id VARCHAR NOT NULL REFERENCES workers(id) ON DELETE CASCADE,
    mac_address VARCHAR NOT NULL,
    allocated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS jobs (
    job_id VARCHAR PRIMARY KEY,
    app_id VARCHAR NOT NULL,
    app_name VARCHAR NOT NULL,
    image VARCHAR NOT NULL,
    user_id VARCHAR NOT NULL,
    status VARCHAR NOT NULL,
    host_id VARCHAR REFERENCES workers(id) ON DELETE SET NULL,
    vm_id VARCHAR,
    vcpus INTEGER NOT NULL,
    memory_mib BIGINT NOT NULL,
    disk_mib BIGINT NOT NULL,
    port INTEGER NOT NULL,
    env_vars JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip_address VARCHAR,
    gateway VARCHAR,
    mac_address VARCHAR,
    netmask VARCHAR,
    error_message TEXT,
    scheduled_at BIGINT,
    started_at BIGINT,
    stopped_at BIGINT,
    created_at BIGINT NOT NULL
);

-- Index for heartbeat cleanup and job lookups
CREATE INDEX IF NOT EXISTS idx_workers_last_heartbeat ON workers(last_heartbeat);
CREATE INDEX IF NOT EXISTS idx_ip_allocations_worker_id ON ip_allocations(worker_id);
CREATE INDEX IF NOT EXISTS idx_jobs_app_id ON jobs(app_id);
CREATE INDEX IF NOT EXISTS idx_jobs_user_id ON jobs(user_id);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
