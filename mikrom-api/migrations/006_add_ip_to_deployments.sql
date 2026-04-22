-- Add ip_address to deployments to allow routing
ALTER TABLE deployments ADD COLUMN ip_address VARCHAR(45);
