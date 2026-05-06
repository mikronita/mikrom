package mikrom

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/caddyserver/caddy/v2/modules/caddyhttp"
	"go.uber.org/zap"
)

func TestMikromRouter_ServeHTTP(t *testing.T) {
	logger := zap.NewNop()
	app := &MikromApp{
		logger: logger,
	}
	app.routes.Store("example.com", "http://backend:8080")
	app.acme.Store("test-token", "test-auth")

	router := MikromRouter{
		app:    app,
		logger: logger,
	}

	// Test ACME Challenge
	t.Run("ACME Challenge", func(t *testing.T) {
		req := httptest.NewRequest(http.MethodGet, "/.well-known/acme-challenge/test-token", nil)
		w := httptest.NewRecorder()

		err := router.ServeHTTP(w, req, caddyhttp.HandlerFunc(func(w http.ResponseWriter, r *http.Request) error {
			t.Error("next handler should not be called for matched ACME challenge")
			return nil
		}))

		if err != nil {
			t.Fatalf("ServeHTTP failed: %v", err)
		}

		if w.Code != http.StatusOK {
			t.Errorf("expected status OK, got %d", w.Code)
		}

		if w.Body.String() != "test-auth" {
			t.Errorf("expected body %q, got %q", "test-auth", w.Body.String())
		}
	})

	// Test Routing
	t.Run("Routing", func(t *testing.T) {
		req := httptest.NewRequest(http.MethodGet, "http://example.com/foo", nil)
		req.Host = "example.com"

		// Caddy's GetVar/SetVar relies on a map in the context
		caddyVars := make(map[string]any)
		ctx := context.WithValue(req.Context(), caddyhttp.VarsCtxKey, caddyVars)
		req = req.WithContext(ctx)

		w := httptest.NewRecorder()

		nextCalled := false
		err := router.ServeHTTP(w, req, caddyhttp.HandlerFunc(func(w http.ResponseWriter, r *http.Request) error {
			nextCalled = true

			// Check if variable was set
			val := caddyhttp.GetVar(r.Context(), "mikrom_target")
			if val == nil {
				// Try to see what variables are available
				vars := r.Context().Value(caddyhttp.VarsCtxKey)
				t.Errorf("mikrom_target variable not set. Vars in context: %v", vars)
			} else if val.(string) != "backend:8080" {
				t.Errorf("expected mikrom_target %q, got %q", "backend:8080", val.(string))
			}
			return nil
		}))

		if err != nil {
			t.Fatalf("ServeHTTP failed: %v", err)
		}

		if !nextCalled {
			t.Error("next handler was not called")
		}
	})

	// Test Unmatched Host
	t.Run("Unmatched Host", func(t *testing.T) {
		req := httptest.NewRequest(http.MethodGet, "http://unknown.com/", nil)
		w := httptest.NewRecorder()

		nextCalled := false
		err := router.ServeHTTP(w, req, caddyhttp.HandlerFunc(func(w http.ResponseWriter, r *http.Request) error {
			nextCalled = true
			val := caddyhttp.GetVar(r.Context(), "mikrom_target")
			if val != nil {
				t.Error("mikrom_target variable should not be set for unknown host")
			}
			return nil
		}))

		if err != nil {
			t.Fatalf("ServeHTTP failed: %v", err)
		}

		if !nextCalled {
			t.Error("next handler was not called")
		}
	})

	// Test On-Demand TLS Check (Versioned)
	t.Run("On-Demand TLS Check Versioned", func(t *testing.T) {
		req := httptest.NewRequest(http.MethodGet, "/v1/.mikrom/check-domain?domain=example.com", nil)
		w := httptest.NewRecorder()

		err := router.ServeHTTP(w, req, caddyhttp.HandlerFunc(func(w http.ResponseWriter, r *http.Request) error {
			t.Error("next handler should not be called for internal domain check")
			return nil
		}))

		if err != nil {
			t.Fatalf("ServeHTTP failed: %v", err)
		}

		if w.Code != http.StatusOK {
			t.Errorf("expected status OK, got %d", w.Code)
		}
	})

	// Test On-Demand TLS Check Legacy (Should fall through)
	t.Run("On-Demand TLS Check Legacy", func(t *testing.T) {
		req := httptest.NewRequest(http.MethodGet, "/.mikrom/check-domain?domain=example.com", nil)
		w := httptest.NewRecorder()

		nextCalled := false
		err := router.ServeHTTP(w, req, caddyhttp.HandlerFunc(func(w http.ResponseWriter, r *http.Request) error {
			nextCalled = true
			return nil
		}))

		if err != nil {
			t.Fatalf("ServeHTTP failed: %v", err)
		}

		if !nextCalled {
			t.Error("next handler should be called for legacy path")
		}
	})
}
