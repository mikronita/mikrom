package mikrom

import (
	"encoding/json"
	"testing"
	"time"
)

func TestNatsLogger(t *testing.T) {
	app := &MikromApp{
		logChan: make(chan string, 10),
	}

	// Create a dummy message
	msg := "test log message"
	app.logChan <- msg

	// Verify we can read it from the channel
	select {
	case received := <-app.logChan:
		if received != msg {
			t.Errorf("Expected %s, got %s", msg, received)
		}
	case <-time.After(1 * time.Second):
		t.Error("Timed out waiting for log message")
	}
}

func TestLogEntryJSON(t *testing.T) {
	entry := LogEntry{
		VmID:      "system",
		AppID:     "test-app",
		Source:    "stdout",
		Message:   "hello",
		Timestamp: 123456789,
	}

	payload, err := json.Marshal([]LogEntry{entry})
	if err != nil {
		t.Fatalf("Failed to marshal LogEntry: %v", err)
	}

	var entries []LogEntry
	if err := json.Unmarshal(payload, &entries); err != nil {
		t.Fatalf("Failed to unmarshal LogEntry: %v", err)
	}

	if len(entries) != 1 || entries[0].Message != "hello" {
		t.Errorf("Unexpected unmarshaled entries: %+v", entries)
	}
}
