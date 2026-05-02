-- Migration to remove agent_port from workers table
ALTER TABLE workers DROP COLUMN agent_port;
