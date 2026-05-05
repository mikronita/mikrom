package mikrom

import (
	"embed"
	"fmt"

	"github.com/golang-migrate/migrate/v4"
	_ "github.com/golang-migrate/migrate/v4/database/postgres"
	"github.com/golang-migrate/migrate/v4/source/iofs"
	"go.uber.org/zap"
)

//go:embed migrations/*.sql
var migrationsFS embed.FS

func (m *MikromApp) runMigrations() error {
	m.logger.Info("running database migrations...", zap.String("url", m.DBURL))

	d, err := iofs.New(migrationsFS, "migrations")
	if err != nil {
		return fmt.Errorf("failed to create migration source: %w", err)
	}

	migrator, err := migrate.NewWithSourceInstance("iofs", d, m.DBURL)
	if err != nil {
		return fmt.Errorf("failed to create migrator: %w", err)
	}
	defer migrator.Close()

	if err := migrator.Up(); err != nil && err != migrate.ErrNoChange {
		return fmt.Errorf("failed to run migrations: %w", err)
	}

	m.logger.Info("database migrations completed successfully")
	return nil
}

func (m *MikromApp) syncFromDB() error {
	m.logger.Info("syncing from database...")

	// Sync routes
	rows, err := m.pool.Query(m.ctx, "SELECT hostname, target_url FROM routes")
	if err != nil {
		return err
	}
	defer rows.Close()

	for rows.Next() {
		var hostname, targetURL string
		if err := rows.Scan(&hostname, &targetURL); err != nil {
			m.logger.Error("failed to scan route row", zap.Error(err))
			continue
		}
		m.routes.Store(hostname, targetURL)
	}

	// Sync ACME challenges
	acmeRows, err := m.pool.Query(m.ctx, "SELECT token, key_auth FROM acme_challenges")
	if err != nil {
		return err
	}
	defer acmeRows.Close()

	for acmeRows.Next() {
		var token, keyAuth string
		if err := acmeRows.Scan(&token, &keyAuth); err != nil {
			m.logger.Error("failed to scan acme row", zap.Error(err))
			continue
		}
		m.acme.Store(token, keyAuth)
	}

	return nil
}

func (m *MikromApp) saveRoute(hostname, targetURL string) error {
	_, err := m.pool.Exec(m.ctx, `
		INSERT INTO routes (hostname, target_url, updated_at) 
		VALUES ($1, $2, NOW()) 
		ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at`,
		hostname, targetURL)
	return err
}

func (m *MikromApp) deleteRoute(hostname string) error {
	_, err := m.pool.Exec(m.ctx, "DELETE FROM routes WHERE hostname = $1", hostname)
	return err
}

func (m *MikromApp) saveACMEChallenge(token, keyAuth, hostname string) error {
	_, err := m.pool.Exec(m.ctx, `
		INSERT INTO acme_challenges (token, key_auth, hostname) 
		VALUES ($1, $2, $3) 
		ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname`,
		token, keyAuth, hostname)
	return err
}

func (m *MikromApp) deleteACMEChallenge(token string) error {
	_, err := m.pool.Exec(m.ctx, "DELETE FROM acme_challenges WHERE token = $1", token)
	return err
}

func (m *MikromApp) saveTLSCertificate(hostname, certChain, privateKey string, expiresAt int64) error {
	_, err := m.pool.Exec(m.ctx, `
		INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) 
		VALUES ($1, $2, $3, TO_TIMESTAMP($4)) 
		ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at`,
		hostname, certChain, privateKey, expiresAt)
	return err
}
