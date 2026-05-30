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
WG_INTERFACE="wg0"

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

if ! command -v wg &> /dev/null; then
    echo "Error: 'wg' not found. It is required to apply mesh routes."
    exit 1
fi

if [ ! -f "$PROTO_PATH" ]; then
    echo "Error: Proto file not found at $PROTO_PATH"
    exit 1
fi

MESH_SUBJECT="mikrom.scheduler.network.mesh.${HOST_ID}"
if [ "$(id -u)" -eq 0 ]; then
    WG_CMD=(wg)
    IP_CMD=(ip)
else
    echo "Error: This script must be run as root or with sudo."
    exit 1
fi

wg_exec() {
    "${WG_CMD[@]}" "$@"
}

ip_exec() {
    "${IP_CMD[@]}" "$@"
}

format_endpoint() {
    local endpoint="$1"
    local port="$2"
    if [[ "$endpoint" == *:* && "$endpoint" != \[*\] ]]; then
        echo "[$endpoint]:$port"
    else
        echo "${endpoint}:${port}"
    fi
}

install_route_for_allowed_ip() {
    local allowed_ip="$1"
    if [[ "$allowed_ip" == *:* ]]; then
        if [[ "$allowed_ip" != */* ]]; then
            allowed_ip="${allowed_ip}/128"
        fi
        if ! ip_exec -6 route replace "$allowed_ip" dev "$WG_INTERFACE"; then
            echo "[$(date +%T)] Failed to install IPv6 route for $allowed_ip"
        fi
    else
        if [[ "$allowed_ip" != */* ]]; then
            allowed_ip="${allowed_ip}/32"
        fi
        if ! ip_exec -4 route replace "$allowed_ip" dev "$WG_INTERFACE"; then
            echo "[$(date +%T)] Failed to install IPv4 route for $allowed_ip"
        fi
    fi
}

log_payload_preview() {
    local file="$1"
    local preview_hex
    preview_hex="$(od -An -tx1 -N 32 "$file" 2>/dev/null | tr -s ' ' | sed 's/^ //; s/ $//')"
    echo "[$(date +%T)] Payload preview (hex, first 32 bytes): ${preview_hex:-<unavailable>}"
}

parse_mesh_update() {
    local decoded_file="$1"
    awk '
        function flush_peer() {
            if (in_peer && pubkey != "") {
                print host_id "\t" endpoint "\t" pubkey "\t" port "\t" allowed_ips
            }
            host_id = ""
            endpoint = ""
            pubkey = ""
            port = ""
            allowed_ips = ""
        }
        /^peers \{/ {
            in_peer = 1
            host_id = ""
            endpoint = ""
            pubkey = ""
            port = ""
            allowed_ips = ""
            next
        }
        in_peer && /^  host_id: "/ {
            sub(/^  host_id: "/, "")
            sub(/"$/, "")
            host_id = $0
            next
        }
        in_peer && /^  endpoint: "/ {
            sub(/^  endpoint: "/, "")
            sub(/"$/, "")
            endpoint = $0
            next
        }
        in_peer && /^  wireguard_pubkey: "/ {
            sub(/^  wireguard_pubkey: "/, "")
            sub(/"$/, "")
            pubkey = $0
            next
        }
        in_peer && /^  wireguard_port: / {
            sub(/^  wireguard_port: /, "")
            port = $0
            next
        }
        in_peer && /^  allowed_ips: "/ {
            sub(/^  allowed_ips: "/, "")
            sub(/"$/, "")
            if (allowed_ips == "") {
                allowed_ips = $0
            } else {
                allowed_ips = allowed_ips "," $0
            }
            next
        }
        in_peer && /^}/ {
            flush_peer()
            in_peer = 0
        }
    ' "$decoded_file"
}

parse_mesh_update_raw() {
    local decoded_file="$1"
    awk '
        function flush_peer() {
            if (in_peer && pubkey != "") {
                print host_id "\t" endpoint "\t" pubkey "\t" port "\t" allowed_ips
            }
            host_id = ""
            endpoint = ""
            pubkey = ""
            port = ""
            allowed_ips = ""
        }
        /^[[:space:]]*1[[:space:]]*\{$/ {
            in_peer = 1
            host_id = ""
            endpoint = ""
            pubkey = ""
            port = ""
            allowed_ips = ""
            next
        }
        in_peer && /^[[:space:]]*1: "/ {
            sub(/^[[:space:]]*1: "/, "")
            sub(/"$/, "")
            host_id = $0
            next
        }
        in_peer && /^[[:space:]]*2: "/ {
            sub(/^[[:space:]]*2: "/, "")
            sub(/"$/, "")
            endpoint = $0
            next
        }
        in_peer && /^[[:space:]]*3: "/ {
            sub(/^[[:space:]]*3: "/, "")
            sub(/"$/, "")
            pubkey = $0
            next
        }
        in_peer && /^[[:space:]]*4: "/ {
            sub(/^[[:space:]]*4: "/, "")
            sub(/"$/, "")
            if (allowed_ips == "") {
                allowed_ips = $0
            } else {
                allowed_ips = allowed_ips "," $0
            }
            next
        }
        in_peer && /^[[:space:]]*5: / {
            sub(/^[[:space:]]*5: /, "")
            port = $0
            next
        }
        in_peer && /^[[:space:]]*}/ {
            flush_peer()
            in_peer = 0
        }
    ' "$decoded_file"
}

parse_mesh_update_binary() {
    local raw_file="$1"
    python3 - "$raw_file" <<'PY'
import sys

path = sys.argv[1]
data = open(path, "rb").read()

def read_varint(buf, idx):
    shift = 0
    value = 0
    while True:
        if idx >= len(buf):
            raise ValueError("truncated varint")
        b = buf[idx]
        idx += 1
        value |= (b & 0x7F) << shift
        if not (b & 0x80):
            return value, idx
        shift += 7
        if shift >= 64:
            raise ValueError("varint too long")

def skip_field(buf, idx, wire_type):
    if wire_type == 0:
        _, idx = read_varint(buf, idx)
        return idx
    if wire_type == 1:
        return idx + 8
    if wire_type == 2:
        length, idx = read_varint(buf, idx)
        return idx + length
    if wire_type == 5:
        return idx + 4
    raise ValueError(f"unsupported wire type {wire_type}")

def parse_peer(buf):
    idx = 0
    end = len(buf)
    host_id = ""
    endpoint = ""
    pubkey = ""
    allowed_ips = []
    port = ""

    while idx < end:
        key, idx = read_varint(buf, idx)
        field = key >> 3
        wire_type = key & 7
        if wire_type == 2:
            length, idx = read_varint(buf, idx)
            if idx + length > end:
                raise ValueError("truncated length-delimited field")
            value = buf[idx:idx + length].decode("utf-8", errors="replace")
            idx += length
            if field == 1:
                host_id = value
            elif field == 2:
                endpoint = value
            elif field == 3:
                pubkey = value
            elif field == 4:
                allowed_ips.append(value)
        elif wire_type == 0:
            value, idx = read_varint(buf, idx)
            if field == 5:
                port = str(value)
        else:
            idx = skip_field(buf, idx, wire_type)

    if pubkey:
        print(f"{host_id}\t{endpoint}\t{pubkey}\t{port}\t{','.join(allowed_ips)}")

def parse_message(buf):
    idx = 0
    while idx < len(buf):
        key, idx = read_varint(buf, idx)
        field = key >> 3
        wire_type = key & 7
        if field == 1 and wire_type == 2:
            length, idx = read_varint(buf, idx)
            if idx + length > len(buf):
                raise ValueError("truncated peer message")
            parse_peer(buf[idx:idx + length])
            idx += length
        else:
            idx = skip_field(buf, idx, wire_type)

def try_parse(buf):
    parse_message(buf)

try:
    try_parse(data)
except Exception:
    stripped = None
    if data.endswith(b"\r\n"):
        stripped = data[:-2]
    elif data.endswith(b"\n") or data.endswith(b"\r"):
        stripped = data[:-1]
    if stripped is None:
        raise
    try:
        try_parse(stripped)
    except Exception:
        raise
PY
}

apply_mesh_update_from_file() {
    local decoded_file="$1"
    local parser="$2"
    local desired_pubkeys_file
    desired_pubkeys_file=$(mktemp)

    while IFS=$'\t' read -r host_id endpoint pubkey port allowed_ips; do
        [ -z "$pubkey" ] && continue

        echo "$pubkey" >> "$desired_pubkeys_file"

        if [ -z "$endpoint" ] || [ -z "$port" ]; then
            continue
        fi

        local formatted_endpoint
        formatted_endpoint="$(format_endpoint "$endpoint" "$port")"

        if [ -n "$allowed_ips" ]; then
            if ! wg_exec set "$WG_INTERFACE" peer "$pubkey" endpoint "$formatted_endpoint" allowed-ips "$allowed_ips" persistent-keepalive 25; then
                echo "[$(date +%T)] Failed to update peer $pubkey for host ${host_id:-unknown}"
            else
                IFS=',' read -ra route_ips <<< "$allowed_ips"
                for allowed_ip in "${route_ips[@]}"; do
                    install_route_for_allowed_ip "$allowed_ip"
                done
            fi
        else
            if ! wg_exec set "$WG_INTERFACE" peer "$pubkey" endpoint "$formatted_endpoint"; then
                echo "[$(date +%T)] Failed to update peer endpoint for $pubkey"
            fi
        fi
    done < <("$parser" "$decoded_file")

    local current_peers
    current_peers="$(wg_exec show "$WG_INTERFACE" peers 2>/tmp/wg_show_error || true)"
    for peer in $current_peers; do
        if ! grep -Fxq "$peer" "$desired_pubkeys_file"; then
            if ! wg_exec set "$WG_INTERFACE" peer "$peer" remove; then
                echo "[$(date +%T)] Failed to remove stale peer $peer"
            fi
        fi
    done

    rm -f "$desired_pubkeys_file"
}

listen_for_mesh_updates() {
    local raw_file decoded_file
    decoded_file=$(mktemp)

    echo "Listening for mesh updates on $MESH_SUBJECT..."

    while true; do
        local nats_error_file protoc_error_file python_error_file raw_decode_error_file wg_error_file
        nats_error_file=$(mktemp)
        protoc_error_file=$(mktemp)
        python_error_file=$(mktemp)
        raw_decode_error_file=$(mktemp)
        wg_error_file=$(mktemp)
        raw_file=$(mktemp)
        if ! nats sub -s "$NATS_URL" --raw --count 1 "$MESH_SUBJECT" >"$raw_file" 2>"$nats_error_file"; then
            echo "[$(date +%T)] Failed to receive mesh update: $(cat "$nats_error_file")"
            rm -f "$raw_file"
            rm -f "$nats_error_file" "$protoc_error_file" "$python_error_file" "$raw_decode_error_file" "$wg_error_file"
            sleep 2
            continue
        fi

        if protoc --decode=mikrom.scheduler.v1.NetworkMeshUpdate -I "$PROTO_DIR" "$PROTO_PATH" < "$raw_file" > "$decoded_file" 2>"$protoc_error_file"; then
            apply_mesh_update_from_file "$decoded_file" parse_mesh_update
            rm -f "$raw_file"
            rm -f "$nats_error_file" "$protoc_error_file" "$python_error_file" "$raw_decode_error_file" "$wg_error_file"
            continue
        fi

        if parse_mesh_update_binary "$raw_file" > "$decoded_file.raw" 2>"$python_error_file"; then
            apply_mesh_update_from_file "$decoded_file.raw" cat
            rm -f "$raw_file" "$decoded_file.raw"
            rm -f "$nats_error_file" "$protoc_error_file" "$python_error_file" "$raw_decode_error_file" "$wg_error_file"
            continue
        fi

        if ! protoc --decode_raw < "$raw_file" > "$decoded_file.raw" 2>"$raw_decode_error_file"; then
            echo "[$(date +%T)] Raw mesh payload size: $(stat -c%s "$raw_file" 2>/dev/null || echo 0) bytes"
            log_payload_preview "$raw_file"
            echo "[$(date +%T)] Failed to decode mesh update: $(cat "$protoc_error_file")"
            echo "[$(date +%T)] Python binary fallback failed: $(cat "$python_error_file")"
            echo "[$(date +%T)] Raw decode fallback failed: $(cat "$raw_decode_error_file")"
            rm -f "$raw_file" "$decoded_file.raw"
            rm -f "$nats_error_file" "$protoc_error_file" "$python_error_file" "$raw_decode_error_file" "$wg_error_file"
            continue
        fi

        apply_mesh_update_from_file "$decoded_file.raw" parse_mesh_update_raw
        rm -f "$raw_file"
        rm -f "$decoded_file.raw"
        rm -f "$nats_error_file" "$protoc_error_file" "$python_error_file" "$raw_decode_error_file" "$wg_error_file"
    done
}

echo "Starting heartbeat simulation for $HOST_ID..."
echo "  WireGuard IP: $WG_IP"
echo "  WireGuard Port: $WG_PORT"
echo "  NATS Server: $NATS_URL"
echo ""

listen_for_mesh_updates &
MESH_LISTENER_PID=$!
trap 'kill "$MESH_LISTENER_PID" 2>/dev/null || true' EXIT

while true; do
    # Construct the text-format Protobuf message
    printf -v MESSAGE '%s\n' \
        "host_id: \"$HOST_ID\"" \
        "hostname: \"$HOSTNAME\"" \
        "wireguard_pubkey: \"$WG_PUBKEY\"" \
        "wireguard_ip: \"$WG_IP\"" \
        "wireguard_port: $WG_PORT" \
        "advertise_address: \"$ADVERTISE_ADDR\""

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
