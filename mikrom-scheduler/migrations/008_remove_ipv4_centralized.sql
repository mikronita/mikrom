-- 1. Drop ip_allocations table
DROP TABLE IF EXISTS ip_allocations;

-- 2. Remove IPv4 columns from jobs
ALTER TABLE jobs DROP COLUMN IF EXISTS ip_address;
ALTER TABLE jobs DROP COLUMN IF EXISTS gateway;
ALTER TABLE jobs DROP COLUMN IF EXISTS mac_address;
ALTER TABLE jobs DROP COLUMN IF EXISTS netmask;

-- 3. Remove IPv4 columns from workers
ALTER TABLE workers DROP COLUMN IF EXISTS ip_address;
ALTER TABLE workers DROP COLUMN IF EXISTS bridge_ip;
