-- Make application names globally unique
ALTER TABLE apps ADD CONSTRAINT apps_name_key UNIQUE (name);
