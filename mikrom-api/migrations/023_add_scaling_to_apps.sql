-- Add scaling fields to apps table
ALTER TABLE apps
ADD COLUMN desired_replicas INTEGER NOT NULL DEFAULT 1,
ADD COLUMN min_replicas INTEGER NOT NULL DEFAULT 1,
ADD COLUMN max_replicas INTEGER NOT NULL DEFAULT 1,
ADD COLUMN autoscaling_enabled BOOLEAN NOT NULL DEFAULT FALSE,
ADD COLUMN cpu_threshold FLOAT NOT NULL DEFAULT 80.0,
ADD COLUMN mem_threshold FLOAT NOT NULL DEFAULT 80.0;

-- Ensure constraints
ALTER TABLE apps
ADD CONSTRAINT replicas_positive CHECK (desired_replicas >= 0),
ADD CONSTRAINT min_replicas_positive CHECK (min_replicas >= 0),
ADD CONSTRAINT max_replicas_positive CHECK (max_replicas >= 1),
ADD CONSTRAINT min_max_ordered CHECK (min_replicas <= max_replicas);
