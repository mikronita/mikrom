-- Populate NULL github_webhook_secret with random values
UPDATE apps 
SET github_webhook_secret = replace(gen_random_uuid()::text, '-', '')
WHERE github_webhook_secret IS NULL;
