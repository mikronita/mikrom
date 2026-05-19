-- Allow multiple targets per hostname
ALTER TABLE routes DROP CONSTRAINT routes_pkey;
ALTER TABLE routes ADD PRIMARY KEY (hostname, target_url);
