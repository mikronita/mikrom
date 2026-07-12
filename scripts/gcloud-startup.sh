#!/usr/bin/env bash
set -ex

LOG_FILE="/var/log/mikrom-install.log"
# Asegurar que las salidas vayan a syslog y a un archivo dedicado de log
exec > >(tee -a "$LOG_FILE") 2>&1

export HOME="/root"

echo "[*] Iniciando aprovisionamiento de Mikrom..."

# Leer metadatos de configuración de GCP
echo "[*] Leyendo metadatos de configuración de GCP..."
GIT_REPO=$(curl -s -f -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/attributes/git-repo || echo "https://github.com/mikronita/mikrom.git")
GIT_BRANCH=$(curl -s -f -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/attributes/git-branch || echo "main")
GIT_TOKEN=$(curl -s -f -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/attributes/git-token || echo "")
SSH_PUBLIC_KEYS=$(curl -s -f -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/attributes/ssh-public-keys || echo "")


# 1. Instalar dependencias básicas
apt-get update
apt-get install -y \
    curl git build-essential cmake pkg-config libssl-dev libelf-dev libbpf-dev \
    protobuf-compiler debootstrap wireguard-tools iptables iproute2 jq \
    ca-certificates gnupg lsb-release

# Instalar Docker
mkdir -p /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg --yes
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian \
  $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null
apt-get update
apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin

# Instalar Node.js y pnpm
curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
apt-get install -y nodejs
npm install -g pnpm

# Instalar Rust
export RUSTUP_HOME=/opt/rust
export CARGO_HOME=/opt/rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
export PATH="/opt/rust/bin:$PATH"

# Instalar Zig (obteniendo dinámicamente la última versión dev de master)
ZIG_VERSION=$(curl -s https://ziglang.org/download/index.json | jq -r '.master.version')
echo "[*] Descargando e instalando Zig versión ${ZIG_VERSION}..."
curl -fsSL "https://ziglang.org/builds/zig-x86_64-linux-${ZIG_VERSION}.tar.xz" | tar -xJ -C /opt
ln -sf "/opt/zig-x86_64-linux-${ZIG_VERSION}/zig" /usr/local/bin/zig

# 2. Clonar repositorio
REPO_DIR="/opt/mikrom"

GIT_REPO_AUTH="$GIT_REPO"
if [ -n "$GIT_TOKEN" ]; then
    # Insertar token en URL de Github
    GIT_REPO_AUTH=$(echo "$GIT_REPO" | sed -E "s|https://|https://${GIT_TOKEN}@|")
fi

if [ -d "$REPO_DIR/.git" ]; then
    echo "[*] El repositorio ya existe en $REPO_DIR. Actualizando con git pull..."
    cd "$REPO_DIR"
    git remote set-url origin "$GIT_REPO_AUTH"
    git fetch origin
    # Intentar checkout de la rama configurada, o usar main por defecto
    git checkout "$GIT_BRANCH" || git checkout main
    git pull origin "$GIT_BRANCH" || git pull origin main
else
    echo "[*] Clonando repositorio: $GIT_REPO (rama: $GIT_BRANCH)..."
    if ! git clone -b "$GIT_BRANCH" "$GIT_REPO_AUTH" "$REPO_DIR"; then
        echo "[!] Error al clonar rama $GIT_BRANCH. Reintentando con rama main..."
        git clone -b "main" "$GIT_REPO_AUTH" "$REPO_DIR"
    fi
    cd "$REPO_DIR"
fi

# 3. Arrancar Base Infrastructure en Docker
# Modificamos docker-compose para incluir un registro local de OCI (para mikrom-builder)
cat > docker-compose.prod.yml <<'EOF'
services:
  postgres:
    image: postgres:17-alpine
    restart: always
    environment:
      POSTGRES_USER: mikrom
      POSTGRES_PASSWORD: mikrom_password
      POSTGRES_DB: mikrom_default
    ports:
      - "127.0.0.1:5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./scripts/init-db.sh:/docker-entrypoint-initdb.d/init-db.sh
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U mikrom -d mikrom_default"]
      interval: 5s
      timeout: 5s
      retries: 10

  nats:
    image: nats:2.10-alpine
    restart: always
    ports:
      - "127.0.0.1:4222:4222"
      - "127.0.0.1:8222:8222"

  buildkit:
    image: moby/buildkit
    container_name: buildkit
    restart: always
    privileged: true
    command: ["buildkitd"]
    volumes:
      - buildkit_data:/var/lib/buildkit

  registry:
    image: registry:2
    restart: always
    ports:
      - "127.0.0.1:5000:5000"

volumes:
  postgres_data:
  buildkit_data:
EOF

docker compose -f docker-compose.prod.yml up -d --wait

# 4. Compilar Mikrom
make build-init
cmake -S tundra-nat64 -B target/tundra-build -DCMAKE_BUILD_TYPE=Release
cmake --build target/tundra-build --parallel
cp target/tundra-build/tundra-nat64 target/release/tundra-nat64
cargo build --release

# Instalar binarios
cp target/release/mikrom-api /usr/local/bin/
cp target/release/mikrom-scheduler /usr/local/bin/
cp target/release/mikrom-builder /usr/local/bin/
cp target/release/mikrom-agent /usr/bin/
cp target/release/mikrom-router /usr/bin/
cp target/release/mikrom-dns /usr/bin/
cp target/release/mikrom-network /usr/bin/
cp target/release/mikrom-cli /usr/bin/
cp target/release/mikrom-init /usr/bin/
cp target/release/tundra-nat64 /usr/local/bin/

# Construir App (SvelteKit)
cd "$REPO_DIR/mikrom-app"
pnpm install
pnpm build
cd "$REPO_DIR"

# 5. Generar rootfs para MicroVMs
export FC_BASE_ROOTFS=/opt/firecracker/base-rootfs.ext4
mkdir -p /opt/firecracker
export SSH_PUBLIC_KEYS="$SSH_PUBLIC_KEYS"
export REGISTRY_URL="127.0.0.1:5000/mikrom"
./scripts/build-base-rootfs.sh

# 6. Obtener IP pública y configurar variables de entorno
PUBLIC_IP=$(curl -s -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/network-interfaces/0/access-configs/0/external-ip)
API_DOMAIN="api.${PUBLIC_IP}.sslip.io"
DASHBOARD_DOMAIN="dashboard.${PUBLIC_IP}.sslip.io"
JWT_SEC=$(openssl rand -hex 32)
MASTER_KEY_VAL=$(openssl rand -hex 32)

mkdir -p /etc/mikrom

# Generar archivos .env
cat > /etc/mikrom/api.env <<EOF
DATABASE_URL=postgres://mikrom:mikrom_password@127.0.0.1:5432/mikrom_api
ROUTER_DATABASE_URL=postgres://mikrom:mikrom_password@127.0.0.1:5432/mikrom_router
JWT_SECRET=${JWT_SEC}
MASTER_KEY=${MASTER_KEY_VAL}
NATS_URL=nats://127.0.0.1:4222
DEPLOYMENT_ENV=production
API_PORT=5001
ROUTER_ADDR=http://127.0.0.1:80
FRONTEND_URL=https://${DASHBOARD_DOMAIN}
USE_TLS=false
MIKROM_NEON_DEV_MODE=true
EOF

cat > /etc/mikrom/scheduler.env <<EOF
DATABASE_URL=postgres://mikrom:mikrom_password@127.0.0.1:5432/mikrom_scheduler
NATS_URL=nats://127.0.0.1:4222
USE_TLS=false
EOF

cat > /etc/mikrom/builder.env <<EOF
REGISTRY=127.0.0.1:5000/mikrom
BUILDKIT_HOST="docker-container://buildkit"
NATS_URL=nats://127.0.0.1:4222
LOG_LEVEL=info
EOF

cat > /etc/mikrom/agent.env <<EOF
NATS_URL=nats://127.0.0.1:4222
USE_TLS=false
BRIDGE_IP=fd00::1/64
FC_BASE_ROOTFS=/opt/firecracker/base-rootfs.ext4
MIKROM_NAT64_DIR=/var/lib/mikrom-agent/nat64
EOF

cat > /etc/mikrom/router.env <<EOF
DATABASE_URL=postgres://mikrom:mikrom_password@127.0.0.1:5432/mikrom_router
NATS_URL=nats://127.0.0.1:4222
ROUTER_ID=router-1
API_UPSTREAM_TARGETS=127.0.0.1:5001
WEB_UPSTREAM_TARGETS=127.0.0.1:3001
API_HOST=${API_DOMAIN}
DASHBOARD_HOST=${DASHBOARD_DOMAIN}
MASTER_KEY=${MASTER_KEY_VAL}
DATA_DIR=/var/lib/mikrom-router
STATE_CACHE_PATH=/var/lib/mikrom-router/state.json
WIREGUARD_PORT=51820
ACME_STAGING=true
EOF

cat > /etc/mikrom/dns.env <<EOF
NATS_URL=nats://127.0.0.1:4222
UPSTREAM_DNS=8.8.8.8,1.1.1.1
NAT64_PREFIX=64:ff9b::
EOF

cat > /etc/mikrom/network.env <<EOF
NATS_URL=nats://127.0.0.1:4222
MIKROM_HOST_ID=node-1
MIKROM_WG_PORT=51823
MIKROM_ADVERTISE_ADDRESS=fd00::2
MIKROM_DATA_DIR=/var/lib/mikrom-network
EOF

cat > /etc/mikrom/app.env <<EOF
PORT=3001
ORIGIN=https://${DASHBOARD_DOMAIN}
NODE_ENV=production
EOF

# 7. Crear los servicios systemd
# Copiar los de la codebase
cp mikrom-agent/debian/lib/systemd/system/mikrom-agent.service /etc/systemd/system/
cp mikrom-network/debian/lib/systemd/system/mikrom-network.service /etc/systemd/system/
cp mikrom-dns/debian/lib/systemd/system/mikrom-dns.service /etc/systemd/system/
cp mikrom-router/debian/lib/systemd/system/mikrom-router.service /etc/systemd/system/

# Crear plantillas para los restantes
cat > /etc/systemd/system/mikrom-api.service <<'EOF'
[Unit]
Description=Mikrom API Service
After=network.target docker.service
Wants=network.target

[Service]
Type=simple
User=root
EnvironmentFile=/etc/mikrom/api.env
ExecStart=/usr/local/bin/mikrom-api
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/mikrom-scheduler.service <<'EOF'
[Unit]
Description=Mikrom Scheduler Service
After=network.target docker.service
Wants=network.target

[Service]
Type=simple
User=root
EnvironmentFile=/etc/mikrom/scheduler.env
ExecStart=/usr/local/bin/mikrom-scheduler
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/mikrom-builder.service <<'EOF'
[Unit]
Description=Mikrom Builder Service
After=network.target docker.service
Wants=network.target

[Service]
Type=simple
User=root
EnvironmentFile=/etc/mikrom/builder.env
ExecStart=/usr/local/bin/mikrom-builder
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/mikrom-app.service <<'EOF'
[Unit]
Description=Mikrom Web Dashboard
After=network.target
Wants=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/mikrom/mikrom-app
EnvironmentFile=/etc/mikrom/app.env
ExecStart=/usr/bin/node build
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Recargar systemd
systemctl daemon-reload

# Habilitar e iniciar todos los servicios
SERVICES=(
    mikrom-api
    mikrom-scheduler
    mikrom-builder
    mikrom-app
    mikrom-network
    mikrom-dns
    mikrom-router
    mikrom-agent
)

for svc in "${SERVICES[@]}"; do
    systemctl enable "$svc"
    systemctl start "$svc"
done

echo "[*] ¡Aprovisionamiento completado con éxito!"
echo "[*] Tu plataforma está disponible en:"
echo "    Dashboard: https://${DASHBOARD_DOMAIN}"
echo "    API: https://${API_DOMAIN}"
