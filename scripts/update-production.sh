#!/usr/bin/env bash
# Script para actualizar Mikrom en caliente en producción (Hot Update)
set -euo pipefail

# Asegurar que se ejecuta como root
if [ "$EUID" -ne 0 ]; then
  echo "[!] Este script debe ejecutarse como root (sudo)."
  exit 1
fi

REPO_DIR="/opt/mikrom"
if [ ! -d "$REPO_DIR" ]; then
  echo "[!] No se encontró el directorio del repositorio en $REPO_DIR."
  exit 1
fi

cd "$REPO_DIR"

echo "[*] 1. Descargando últimos cambios de Git..."
git fetch origin
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")
echo "[*] Rama actual detectada: $CURRENT_BRANCH"
git checkout "$CURRENT_BRANCH"
git pull origin "$CURRENT_BRANCH"
git submodule update --init --recursive

echo "[*] 2. Compilando mikrom-init..."
make build-init

echo "[*] 3. Compilando Tundra NAT64..."
cmake --build target/tundra-build --parallel

echo "[*] 4. Compilando servicios en modo release..."
export PATH="/opt/rust/bin:$PATH"
export RUSTUP_HOME=/opt/rust
export CARGO_HOME=/opt/rust
cargo build --release

echo "[*] 5. Instalando nuevos binarios..."
# Eliminar binarios actuales para evitar "Text file busy"
rm -f /usr/local/bin/mikrom-api /usr/local/bin/mikrom-scheduler /usr/local/bin/mikrom-builder /usr/local/bin/tundra-nat64
rm -f /usr/bin/mikrom-agent /usr/bin/mikrom-router /usr/bin/mikrom-dns /usr/bin/mikrom-network /usr/bin/mikrom /usr/bin/mikrom-init

cp target/release/mikrom-api /usr/local/bin/
cp target/release/mikrom-scheduler /usr/local/bin/
cp target/release/mikrom-builder /usr/local/bin/
cp target/release/mikrom-agent /usr/bin/
cp target/release/mikrom-router /usr/bin/
cp target/release/mikrom-dns /usr/bin/
cp target/release/mikrom-network /usr/bin/
cp target/release/mikrom /usr/bin/
cp target/release/mikrom-init /usr/bin/
cp target/release/tundra-nat64 /usr/local/bin/

echo "[*] 6. Compilando Dashboard de SvelteKit..."
cd "$REPO_DIR/mikrom-app"
pnpm install
pnpm build
cd "$REPO_DIR"

echo "[*] 7. Reconstruyendo Base RootFS para MicroVMs..."
export FC_BASE_ROOTFS=/opt/firecracker/base-rootfs.ext4
export REGISTRY_URL="127.0.0.1:5000/mikrom"
./scripts/build-base-rootfs.sh

# Deshabilitar DNSStubListener en systemd-resolved para liberar el puerto 53 para mikrom-dns
if systemctl is-active --quiet systemd-resolved || systemctl is-enabled --quiet systemd-resolved; then
    echo "[*] Configurando systemd-resolved para liberar el puerto 53..."
    mkdir -p /etc/systemd/resolved.conf.d
    cat > /etc/systemd/resolved.conf.d/mikrom-dns.conf <<EOF
[Resolve]
DNSStubListener=no
EOF
    ln -sf /run/systemd/resolve/resolv.conf /etc/resolv.conf
    systemctl restart systemd-resolved
fi

echo "[*] 8. Reiniciando servicios systemd..."
systemctl daemon-reload

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
    echo "[*] Reiniciando $svc..."
    systemctl restart "$svc"
done

echo "[*] ¡Actualización de producción completada con éxito!"
echo "[*] Verificando estado de los servicios:"
systemctl status mikrom-* --no-pager
