package mikrom

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	schedulerv1 "github.com/antpard/mikrom/mikrom-router/proto/scheduler/v1"
	"github.com/caddyserver/caddy/v2"
	"github.com/caddyserver/caddy/v2/caddyconfig/caddyfile"
	"github.com/caddyserver/caddy/v2/caddyconfig/httpcaddyfile"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"go.uber.org/zap"
	"google.golang.org/protobuf/proto"
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
	wg     *WireGuardManager
	ctx    context.Context
	cancel context.CancelFunc
	logger *zap.Logger

	logChan chan string
}

func (m *MikromApp) getRouterIPv6(hostID string) string {
	// DJB2 hash algorithm (simple, stable, fast)
	var hash uint64 = 5381
	for _, b := range []byte(hostID) {
		hash = ((hash << 5) + hash) ^ uint64(b)
	}

	// Use 32 bits of the hash to create a 'normal' looking IPv6 (fd00::xxxx:xxxx)
	s1 := (hash >> 16) & 0xFFFF
	s2 := hash & 0xFFFF

	return fmt.Sprintf("fd00::%x:%x", s1, s2)
}

func (m *MikromApp) runRouterHeartbeat(hostID, hostname, pubKey string) {
	m.logger.Info("starting router heartbeat loop", zap.String("host_id", hostID))
	ticker := time.NewTicker(10 * time.Second)
	defer ticker.Stop()

	subject := "mikrom.scheduler.router.heartbeat"
	wgIPv6 := m.getRouterIPv6(hostID)

	for {
		ip := m.getOutboundIP()
		heartbeat := &schedulerv1.RouterHeartbeat{
			HostId:          hostID,
			Hostname:        hostname,
			IpAddress:       ip,
			WireguardPubkey: pubKey,
			WireguardIp:     wgIPv6,
			WireguardPort:   51822,
		}

		payload, err := proto.Marshal(heartbeat)
		if err == nil {
			if m.nc != nil && m.nc.IsConnected() {
				m.nc.Publish(subject, payload)
				m.logger.Info("sent router heartbeat", zap.String("host_id", hostID), zap.String("wg_ip", wgIPv6))
			}
		} else {
			m.logger.Error("failed to marshal router heartbeat", zap.Error(err))
		}

		select {
		case <-m.ctx.Done():
			return
		case <-ticker.C:
		}
	}
}

func (m *MikromApp) getOutboundIP() string {
	// 1. Try to find an IP in the 192.168.122.0/24 range first
	ifaces, err := net.Interfaces()
	if err == nil {
		for _, i := range ifaces {
			addrs, err := i.Addrs()
			if err != nil {
				continue
			}
			for _, addr := range addrs {
				var ip net.IP
				match := false
				switch v := addr.(type) {
				case *net.IPNet:
					ip = v.IP
				case *net.IPAddr:
					ip = v.IP
				}
				if ip != nil && !ip.IsLoopback() && ip.To4() != nil {
					ip4 := ip.To4()
					if ip4[0] == 192 && ip4[1] == 168 && ip4[2] == 122 {
						match = true
					}
				}
				if match {
					return ip.String()
				}
			}
		}
	}

	// 2. Fallback to generic outbound detection
	conn, err := net.Dial("udp", "8.8.8.8:80")
	if err != nil {
		return "127.0.0.1"
	}
	defer conn.Close()

	localAddr := conn.LocalAddr().(*net.UDPAddr)
	return localAddr.IP.String()
}

type LogEntry struct {
	VmID      string      `json:"vm_id"`
	AppID     string      `json:"app_id"`
	Source    string      `json:"source"`
	Message   interface{} `json:"message"`
	Timestamp int64       `json:"timestamp"`
}

func (m *MikromApp) runNatsLogger() {
	subject := "mikrom.logs.mikrom-router.system"
	for {
		select {
		case <-m.ctx.Done():
			return
		case msg := <-m.logChan:
			if m.nc == nil || !m.nc.IsConnected() {
				continue
			}

			trimmed := strings.TrimSpace(msg)
			var message interface{} = msg

			// If msg is JSON, use RawMessage to avoid double-encoding without expensive unmarshaling
			if strings.HasPrefix(trimmed, "{") && strings.HasSuffix(trimmed, "}") {
				message = json.RawMessage(trimmed)
			}

			entry := LogEntry{
				VmID:      "system",
				AppID:     "mikrom-router",
				Source:    "stdout",
				Message:   message,
				Timestamp: time.Now().UnixNano(),
			}

			payload, err := json.Marshal([]LogEntry{entry})
			if err != nil {
				m.logger.Error("failed to marshal log entry", zap.Error(err))
				continue
			}
			_ = m.nc.Publish(subject, payload)
		}
	}
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
	m.logChan = make(chan string, 1000)
	m.wg = NewWireGuardManager("wg-mikrom", m.logger)

	if m.NatsURL == "" {
		m.NatsURL = nats.DefaultURL
	}

	return nil
}

// Start begins the background synchronization.
func (m *MikromApp) Start() error {
	m.logger.Info("starting mikrom app module")

	go m.runNatsLogger()

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

		// WireGuard Setup
		hostname, _ := os.Hostname()
		// Use a more unique ID for the router host to allow multiple instances
		hostID := fmt.Sprintf("router-%s", hostname)
		wgIPv6 := m.getRouterIPv6(hostID)
		pubKey, err := m.wg.Init(hostID, wgIPv6)
		if err != nil {
			m.logger.Error("failed to initialize WireGuard", zap.Error(err))
		} else {
			go m.runRouterHeartbeat(hostID, hostname, pubKey)
		}
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
