package mikrom

import (
	"fmt"
	"net/http"
	"strings"

	"github.com/caddyserver/caddy/v2"
	"github.com/caddyserver/caddy/v2/caddyconfig/caddyfile"
	"github.com/caddyserver/caddy/v2/caddyconfig/httpcaddyfile"
	"github.com/caddyserver/caddy/v2/modules/caddyhttp"
	"go.uber.org/zap"
	"go.uber.org/zap/zapcore"
)

func init() {
	caddy.RegisterModule(&MikromRouter{})
	httpcaddyfile.RegisterHandlerDirective("mikrom_router", parseCaddyfile)
}

// MikromRouter is a Caddy middleware that routes requests based on the hostname.
type MikromRouter struct {
	app    *MikromApp
	logger *zap.Logger
}

// CaddyModule returns the Caddy module information.
func (*MikromRouter) CaddyModule() caddy.ModuleInfo {
	return caddy.ModuleInfo{
		ID:  "http.handlers.mikrom_router",
		New: func() caddy.Module { return new(MikromRouter) },
	}
}

// Provision sets up the middleware.
func (m *MikromRouter) Provision(ctx caddy.Context) error {
	m.logger = ctx.Logger()

	appModule, err := ctx.App("mikrom")
	if err != nil {
		return fmt.Errorf("failed to get mikrom app: %v", err)
	}
	m.app = appModule.(*MikromApp)

	// Add a hook to send logs to NATS
	if m.app.logChan != nil {
		m.logger = m.logger.WithOptions(zap.Hooks(func(entry zapcore.Entry) error {
			select {
			case m.app.logChan <- entry.Message:
			default:
				// Drop if channel is full
			}
			return nil
		}))
	}

	return nil
}

// ServeHTTP handles the routing and ACME challenges.
func (m *MikromRouter) ServeHTTP(w http.ResponseWriter, r *http.Request, next caddyhttp.Handler) error {
	if m.app == nil {
		m.logger.Error("mikrom app not provisioned")
		return next.ServeHTTP(w, r)
	}

	// Handle internal domain check for On-Demand TLS
	if r.URL.Path == "/.mikrom/check-domain" {
		domain := r.URL.Query().Get("domain")
		if domain == "" {
			w.WriteHeader(http.StatusBadRequest)
			return nil
		}
		if _, ok := m.app.routes.Load(domain); ok {
			w.WriteHeader(http.StatusOK)
			return nil
		}

		// Also check without port if present
		if strings.Contains(domain, ":") {
			hostNoPort := strings.Split(domain, ":")[0]
			if _, ok := m.app.routes.Load(hostNoPort); ok {
				w.WriteHeader(http.StatusOK)
				return nil
			}
		}

		w.WriteHeader(http.StatusNotFound)
		return nil
	}

	// Handle ACME challenges
	if strings.HasPrefix(r.URL.Path, "/.well-known/acme-challenge/") {
		token := r.URL.Path[len("/.well-known/acme-challenge/"):]
		if auth, ok := m.app.acme.Load(token); ok {
			w.Header().Set("Content-Type", "text/plain")
			w.Write([]byte(auth.(string)))
			return nil
		}
	}

	// Route regular traffic
	host := r.Host
	target, ok := m.app.routes.Load(host)
	if !ok && strings.Contains(host, ":") {
		// Try without port
		hostNoPort := strings.Split(host, ":")[0]
		target, ok = m.app.routes.Load(hostNoPort)
	}

	if ok {
		targetURL := target.(string)
		// Strip http:// or https:// if present
		targetHost := strings.TrimPrefix(targetURL, "http://")
		targetHost = strings.TrimPrefix(targetHost, "https://")

		// Set a variable that can be used by the reverse_proxy directive
		caddyhttp.SetVar(r.Context(), "mikrom_target", targetHost)

		m.logger.Debug("routing request", zap.String("host", host), zap.String("target", targetHost))
	}

	return next.ServeHTTP(w, r)
}

// parseCaddyfile unmarshals tokens from h to a new MikromRouter.
func parseCaddyfile(h httpcaddyfile.Helper) (caddyhttp.MiddlewareHandler, error) {
	m := new(MikromRouter)
	err := m.UnmarshalCaddyfile(h.Dispenser)
	return m, err
}

// UnmarshalCaddyfile sets up the middleware from the Caddyfile.
func (m *MikromRouter) UnmarshalCaddyfile(d *caddyfile.Dispenser) error {
	for d.Next() {
		// No arguments expected for now
	}
	return nil
}

// Interface guards
var (
	_ caddyhttp.MiddlewareHandler = (*MikromRouter)(nil)
	_ caddy.Provisioner           = (*MikromRouter)(nil)
	_ caddyfile.Unmarshaler       = (*MikromRouter)(nil)
)
