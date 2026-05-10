-- Add advertise_address to workers table
ALTER TABLE workers ADD COLUMN advertise_address VARCHAR(255) NOT NULL DEFAULT '';
