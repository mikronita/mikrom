-- Drop unused workers table that was accidentally included in previous migrations.
-- This table is not used by the API, as worker data is managed by the scheduler.
DROP TABLE IF EXISTS workers;
