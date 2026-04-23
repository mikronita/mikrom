-- Add active_deployment_id to apps table for instant rollbacks
ALTER TABLE apps
ADD COLUMN active_deployment_id UUID REFERENCES deployments(id) ON DELETE SET NULL;
