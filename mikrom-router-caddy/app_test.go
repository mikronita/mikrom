package mikrom

import (
	"context"
	"testing"
	"time"

	"github.com/caddyserver/caddy/v2"
)

func TestMikromApp_StartNonBlocking(t *testing.T) {
	ctx, cancel := caddy.NewContext(caddy.Context{Context: context.Background()})
	defer cancel()

	app := &MikromApp{
		NatsURL: "nats://127.0.0.1:4223",                        // Non-existent port
		DBURL:   "postgres://mikrom:pass@127.0.0.1:5433/mikrom", // Non-existent port
	}

	err := app.Provision(ctx)
	if err != nil {
		t.Fatalf("Provision failed: %v", err)
	}

	// Start should return immediately without error even if NATS/DB are down
	startChan := make(chan error, 1)
	go func() {
		startChan <- app.Start()
	}()

	select {
	case err := <-startChan:
		if err != nil {
			t.Errorf("Start() returned error: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Error("Start() blocked for more than 2 seconds")
	}

	// Cleanup
	app.Stop()
}
