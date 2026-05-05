package mikrom

import (
	"context"
	"encoding/json"
	"sync"
	"time"

	"github.com/caddyserver/caddy/v2"
	"github.com/caddyserver/caddy/v2/caddyconfig/caddyfile"
	"github.com/caddyserver/caddy/v2/caddyconfig/httpcaddyfile"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"go.uber.org/zap"
)

func init() {
	caddy.RegisterModule(&MikromApp{})
	httpcaddyfile.RegisterGlobalOption("mikrom_app", parseAppCaddyfile)
}

func parseAppCaddyfile(d *caddyfile.Dispenser, _ any) (any, error) {
	app := new(MikromApp)
	err := app.UnmarshalCaddyfile(d)
	if err != nil {
		return nil, err
	}
	config, err := json.Marshal(app)
	if err != nil {
		return nil, err
	}
	return httpcaddyfile.App{
		Name:  "mikrom",
		Value: config,
	}, nil
}

// UnmarshalCaddyfile sets up the app from the Caddyfile.
func (m *MikromApp) UnmarshalCaddyfile(d *caddyfile.Dispenser) error {
	for d.Next() {
		for d.NextBlock(0) {
			switch d.Val() {
			case "nats_url":
				if !d.NextArg() {
					return d.ArgErr()
				}
				m.NatsURL = d.Val()
			case "db_url":
				if !d.NextArg() {
					return d.ArgErr()
				}
				m.DBURL = d.Val()
			case "master_key":
				if !d.NextArg() {
					return d.ArgErr()
				}
				m.MasterKey = d.Val()
			default:
				return d.Errf("unrecognized subdirective: %s", d.Val())
			}
		}
	}
	return nil
}

// MikromApp is a Caddy app that synchronizes routes and certificates from Mikrom DB and NATS.
type MikromApp struct {
	NatsURL   string `json:"nats_url,omitempty"`
	DBURL     string `json:"db_url,omitempty"`
	MasterKey string `json:"master_key,omitempty"`

	routes sync.Map // hostname -> target_url (string)
	acme   sync.Map // token -> key_auth (string)

	pool   *pgxpool.Pool
	nc     *nats.Conn
	ctx    context.Context
	cancel context.CancelFunc
	logger *zap.Logger
}

// CaddyModule returns the Caddy module information.
func (*MikromApp) CaddyModule() caddy.ModuleInfo {
	return caddy.ModuleInfo{
		ID:  "mikrom",
		New: func() caddy.Module { return new(MikromApp) },
	}
}

// Provision sets up the app.
func (m *MikromApp) Provision(ctx caddy.Context) error {
	m.logger = ctx.Logger()
	m.ctx, m.cancel = context.WithCancel(ctx)

	if m.NatsURL == "" {
		m.NatsURL = nats.DefaultURL
	}

	return nil
}

// Start begins the background synchronization.
func (m *MikromApp) Start() error {
	m.logger.Info("starting mikrom app module")

	go func() {
		var err error

		// Connect to DB with retries
		m.logger.Info("connecting to database", zap.String("url", m.DBURL))
		for {
			m.pool, err = pgxpool.New(m.ctx, m.DBURL)
			if err == nil {
				if err = m.pool.Ping(m.ctx); err == nil {
					break
				}
				m.pool.Close()
			}
			m.logger.Warn("failed to connect to database, retrying in 5s", zap.Error(err))

			timer := time.NewTimer(5 * time.Second)
			select {
			case <-m.ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
		}
		m.logger.Info("connected to database")

		// Run migrations
		if err := m.runMigrations(); err != nil {
			m.logger.Fatal("failed to run migrations, failing fast", zap.Error(err))
			return
		}

		// Connect to NATS with retries
		m.logger.Info("connecting to NATS", zap.String("url", m.NatsURL))
		for {
			m.nc, err = nats.Connect(m.NatsURL,
				nats.RetryOnFailedConnect(true),
				nats.MaxReconnects(-1),
				nats.DisconnectErrHandler(func(nc *nats.Conn, err error) {
					m.logger.Warn("NATS disconnected", zap.Error(err))
				}),
				nats.ReconnectHandler(func(nc *nats.Conn) {
					m.logger.Info("NATS reconnected", zap.String("url", nc.ConnectedUrl()))
				}),
			)
			if err == nil {
				break
			}
			m.logger.Warn("failed to initiate NATS connection, retrying in 5s", zap.Error(err))

			timer := time.NewTimer(5 * time.Second)
			select {
			case <-m.ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
		}
		m.logger.Info("connected to NATS")

		// Initial sync
		if err := m.syncFromDB(); err != nil {
			m.logger.Error("initial sync from DB failed", zap.Error(err))
		}

		// Start NATS listeners
		m.listenForUpdates()
	}()

	return nil
}

// Stop cleans up the app.
func (m *MikromApp) Stop() error {
	if m.cancel != nil {
		m.cancel()
	}
	if m.nc != nil {
		m.nc.Close()
	}
	if m.pool != nil {
		m.pool.Close()
	}
	return nil
}

// Interface guards
var (
	_ caddy.App         = (*MikromApp)(nil)
	_ caddy.Provisioner = (*MikromApp)(nil)
)
