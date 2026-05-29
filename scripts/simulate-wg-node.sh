#!/bin/bash
# scripts/simulate-wg-node.sh - Script to trick mikrom-scheduler into adding an external WireGuard node.

set -e

# Default values
HOST_ID="external-node-$(hostname)"
HOSTNAME=$(hostname)
WG_PUBKEY=""
WG_IP="fd00::99"
WG_PORT=51820
ADVERTISE_ADDR="127.0.0.1"
NATS_URL="nats://localhost:4222"
INTERVAL=15

# Get the directory where the script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PROTO_DIR="$PROJECT_ROOT/mikrom-proto/proto"
PROTO_PATH="$PROTO_DIR/scheduler.proto"

usage() {
    echo "Usage: $0 [options]"
    echo ""
    echo "Options:"
    echo "  --host-id ID        Unique ID for this node (default: $HOST_ID)"
    echo "  --hostname NAME     Hostname for this node (default: $HOSTNAME)"
    echo "  --pubkey KEY        WireGuard public key (REQUIRED)"
    echo "  --wg-ip IP          WireGuard IPv6 address (default: $WG_IP)"
    echo "  --wg-port PORT      WireGuard UDP port (default: $WG_PORT)"
    echo "  --adv-addr ADDR     Public IP/Address to reach this node (default: $ADVERTISE_ADDR)"
    echo "  --nats-url URL      NATS server URL (default: $NATS_URL)"
    echo "  --interval SEC      Heartbeat interval (default: $INTERVAL)"
    echo "  -h, --help          Show this help message"
    exit 1
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --host-id) HOST_ID="$2"; shift 2 ;;
        --hostname) HOSTNAME="$2"; shift 2 ;;
        --pubkey) WG_PUBKEY="$2"; shift 2 ;;
        --wg-ip) WG_IP="$2"; shift 2 ;;
        --wg-port) WG_PORT="$2"; shift 2 ;;
        --adv-addr) ADVERTISE_ADDR="$2"; shift 2 ;;
        --nats-url) NATS_URL="$2"; shift 2 ;;
        --interval) INTERVAL="$2"; shift 2 ;;
        -h|--help) usage ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

if [ -z "$WG_PUBKEY" ]; then
    echo "Error: --pubkey is required."
    usage
fi

# Check for dependencies
if ! command -v nats &> /dev/null; then
    echo "Error: 'nats' CLI tool not found. Install it from https://github.com/nats-io/natscli"
    exit 1
fi

if ! command -v protoc &> /dev/null; then
    echo "Error: 'protoc' not found. It is required to encode the heartbeat message."
    exit 1
fi

if [ ! -f "$PROTO_PATH" ]; then
    echo "Error: Proto file not found at $PROTO_PATH"
    exit 1
fi

echo "Starting heartbeat simulation for $HOST_ID..."
echo "  WireGuard IP: $WG_IP"
echo "  WireGuard Port: $WG_PORT"
echo "  NATS Server: $NATS_URL"
echo ""

while true; do
    # Construct the text-format Protobuf message
    MESSAGE=$(cat <<EOF
host_id: "$HOST_ID"
hostname: "$HOSTNAME"
wireguard_pubkey: "$WG_PUBKEY"
wireguard_ip: "$WG_IP"
wireguard_port: $WG_PORT
advertise_address: "$ADVERTISE_ADDR"
EOF
    )

    # Encode to binary and publish to NATS
    # Using a temporary file to avoid pipe/variable issues with binary data
    BIN_FILE=$(mktemp)
    if ! echo "$MESSAGE" | protoc --encode=mikrom.scheduler.v1.WorkerHeartbeat -I "$PROTO_DIR" "$PROTO_PATH" > "$BIN_FILE" 2>/tmp/protoc_error; then
        echo "[$(date +%T)] Error encoding message: $(cat /tmp/protoc_error)"
        rm -f "$BIN_FILE"
        continue
    fi

    # Encode to binary and publish to NATS
    # Using command substitution as it was confirmed to work where pipes failed
    if ! nats pub -s "$NATS_URL" mikrom.scheduler.worker.heartbeat "$(cat "$BIN_FILE")" &> /tmp/nats_error; then
        echo "[$(date +%T)] Failed to send heartbeat: $(cat /tmp/nats_error)"
    else
        echo "[$(date +%T)] Heartbeat sent successfully to $NATS_URL ($(stat -c%s "$BIN_FILE") bytes)"
    fi
    rm -f "$BIN_FILE"

    sleep "$INTERVAL"
done
