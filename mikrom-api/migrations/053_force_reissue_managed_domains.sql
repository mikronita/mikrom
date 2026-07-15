-- Force ACME reissuance for all managed platform domains to ensure they use Let's Encrypt Production
UPDATE acme_managed_domains
SET needs_reissue = TRUE,
    cert_expires_at = NULL;
