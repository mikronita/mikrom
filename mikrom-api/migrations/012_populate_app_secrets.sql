-- Populate NULL github_webhook_secret with random values
UPDATE apps 
SET github_webhook_secret = MD5(RANDOM()::TEXT) || MD5(RANDOM()::TEXT)
WHERE github_webhook_secret IS NULL;
