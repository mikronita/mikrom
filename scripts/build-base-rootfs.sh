#!/bin/bash
set -euo pipefail

# Configuración
DEBIAN_RELEASE="${DEBIAN_RELEASE:-trixie}"
DEBIAN_MIRROR="${DEBIAN_MIRROR:-http://deb.debian.org/debian}"
ROOTFS_SIZE_MB="${ROOTFS_SIZE_MB:-2048}"
OUTPUT_DIR="${OUTPUT_DIR:-/opt/firecracker}"
OUTPUT_FILE="${OUTPUT_DIR}/base-rootfs.ext4"
WORK_DIR=""

cleanup() {
  if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
    echo "[*] Limpiando directorio de trabajo..."
    if mountpoint -q "${WORK_DIR}/rootfs"; then
      umount -l "${WORK_DIR}/rootfs" 2>/dev/null || true
    fi
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

# Claves SSH maestras (separadas por nueva línea)
SSH_PUBLIC_KEYS="${SSH_PUBLIC_KEYS:-}"

echo "========================================="
echo "  Constructor de base rootfs para Mikrom"
echo "========================================="
echo "[+] Distribución: ${DEBIAN_RELEASE}"
echo "[+] Mirror: ${DEBIAN_MIRROR}"
echo "[+] Tamaño: ${ROOTFS_SIZE_MB} MB"
echo "[+] Salida: ${OUTPUT_FILE}"
if [ -n "$SSH_PUBLIC_KEYS" ]; then
  echo "[+] Claves SSH maestras: configuradas"
else
  echo "[+] Claves SSH maestras: no configuradas"
fi

# Verificar dependencias
for cmd in debootstrap mkfs.ext4 dd losetup mount umount; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "[!] Error: '$cmd' no está instalado. Ejecuta: apt-get install debootstrap e2fsprogs"
    exit 1
  fi
done

# Verificar que se ejecuta como root
if [ "$(id -u)" -ne 0 ]; then
  echo "[!] Error: Este script requiere privilegios de root (debootstrap)"
  exit 1
fi

# Crear directorio de salida
mkdir -p "$OUTPUT_DIR"

# Crear directorio de trabajo en la ubicación actual (evitar falta de espacio en /tmp)
WORK_DIR="./.build-rootfs-work"
mkdir -p "$WORK_DIR"
ROOTFS_DIR="${WORK_DIR}/rootfs"
mkdir -p "$ROOTFS_DIR"

# Crear archivo ext4 vacío
echo "[+] Creando archivo ext4 de ${ROOTFS_SIZE_MB}MB..."
dd if=/dev/zero of="${WORK_DIR}/base-rootfs.ext4" bs=1M count="$ROOTFS_SIZE_MB" status=progress
mkfs.ext4 -F -L mikrom-base "${WORK_DIR}/base-rootfs.ext4"

# Montar el ext4
echo "[+] Montando ext4..."
mount -o loop "${WORK_DIR}/base-rootfs.ext4" "$ROOTFS_DIR"

# Bootstrap de Debian
echo "[+] Ejecutando debootstrap (${DEBIAN_RELEASE})..."
debootstrap --variant=minbase "$DEBIAN_RELEASE" "$ROOTFS_DIR" "$DEBIAN_MIRROR"

# Montar filesystems esenciales para chroot
mount -t proc proc "${ROOTFS_DIR}/proc"
mount -t sysfs sysfs "${ROOTFS_DIR}/sys"
mount -o bind /dev "${ROOTFS_DIR}/dev"
mount -o bind /dev/pts "${ROOTFS_DIR}/dev/pts"

# Preparar claves SSH maestras para inyección
SSH_KEYS_FILE="${WORK_DIR}/ssh-master-keys.txt"
if [ -n "$SSH_PUBLIC_KEYS" ]; then
  echo "$SSH_PUBLIC_KEYS" >"$SSH_KEYS_FILE"
else
  touch "$SSH_KEYS_FILE"
fi

# Hacer las claves accesibles desde el chroot
mkdir -p "${ROOTFS_DIR}/tmp"
cp "$SSH_KEYS_FILE" "${ROOTFS_DIR}/tmp/ssh-master-keys.txt"

# Configurar el sistema base dentro del chroot
echo "[+] Configurando sistema base..."
chroot "$ROOTFS_DIR" /bin/bash -euo pipefail <<CHROOT_SCRIPT
# Configurar sources.list
cat > /etc/apt/sources.list <<EOF
deb ${DEBIAN_MIRROR} ${DEBIAN_RELEASE} main contrib non-free
deb ${DEBIAN_MIRROR} ${DEBIAN_RELEASE}-updates main contrib non-free
deb http://security.debian.org/debian-security ${DEBIAN_RELEASE}-security main contrib non-free
EOF

# Instalar paquetes esenciales
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    openssh-server \
    ca-certificates \
    curl \
    wget \
    jq \
    iproute2 \
    iputils-ping \
    dnsutils \
    net-tools \
    vim-tiny \
    less \
    procps \
    coreutils \
    util-linux \
    bash \
    locales \
    tzdata \
    sudo \
    strace

# Configurar locale
echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen
locale-gen
update-locale LANG=en_US.UTF-8

# Configurar zona horaria
ln -sf /usr/share/zoneinfo/Europe/Madrid /etc/localtime
dpkg-reconfigure -f noninteractive tzdata

# Configurar SSH
mkdir -p /run/sshd
cat > /etc/ssh/sshd_config <<'SSHD_CONFIG'
# Configuración base de sshd para Mikrom
Port 22
ListenAddress 0.0.0.0
ListenAddress ::

# Autenticación
PermitRootLogin prohibit-password
PubkeyAuthentication yes
AuthorizedKeysFile .ssh/authorized_keys
PasswordAuthentication no
PermitEmptyPasswords no
ChallengeResponseAuthentication no
UsePAM yes

# Seguridad
X11Forwarding no
PrintMotd no
AcceptEnv LANG LC_*
Subsystem sftp /usr/lib/openssh/sftp-server

# Rendimiento
UseDNS no
MaxAuthTries 3
MaxSessions 5
ClientAliveInterval 300
ClientAliveCountMax 2
SSHD_CONFIG

# Generar host keys (se regenerarán en el init si es necesario)
ssh-keygen -A

# Crear usuario mikrom con acceso sudo
useradd -m -s /bin/bash mikrom
echo "mikrom ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/mikrom
chmod 440 /etc/sudoers.d/mikrom

# Directorios esenciales
mkdir -p /etc/mikrom
mkdir -p /root/.ssh
chmod 700 /root/.ssh
mkdir -p /home/mikrom/.ssh
chmod 700 /home/mikrom/.ssh
chown mikrom:mikrom /home/mikrom/.ssh

# Inyectar claves SSH maestras si existen
if [ -s /tmp/ssh-master-keys.txt ]; then
  echo "[SSH] Inyectando claves maestras..."
  cp /tmp/ssh-master-keys.txt /root/.ssh/authorized_keys
  cp /tmp/ssh-master-keys.txt /home/mikrom/.ssh/authorized_keys
  chmod 600 /root/.ssh/authorized_keys
  chmod 600 /home/mikrom/.ssh/authorized_keys
  chown mikrom:mikrom /home/mikrom/.ssh/authorized_keys
  rm -f /tmp/ssh-master-keys.txt
fi

# Limpiar caché de apt
apt-get clean
rm -rf /var/lib/apt/lists/*
rm -rf /tmp/*
rm -rf /var/cache/debconf/*
CHROOT_SCRIPT

# Exportar a imagen local y subir al registro si está disponible
echo "[+] Exporting rootfs to Docker image..."
# Excluimos directorios de sistema para un import limpio
tar -C "$ROOTFS_DIR" --exclude=./proc/* --exclude=./sys/* --exclude=./dev/* --exclude=./tmp/* -c . | docker import - mikrom-base:latest

REGISTRY_URL="${REGISTRY_URL:-registry.mikrom.spluca.org/mikrom}"
docker tag mikrom-base:latest "$REGISTRY_URL/mikrom-base:latest"
if docker push "$REGISTRY_URL/mikrom-base:latest" 2>/dev/null; then
  echo "[+] Imagen subida a $REGISTRY_URL/mikrom-base:latest"
else
  echo "[!] No se pudo subir la imagen al registro (ignorable si es desarrollo local)"
fi

# Desmontar filesystems
echo "[+] Desmontando filesystems..."
umount "${ROOTFS_DIR}/dev/pts"
umount "${ROOTFS_DIR}/dev"
umount "${ROOTFS_DIR}/sys"
umount "${ROOTFS_DIR}/proc"
umount "$ROOTFS_DIR"

# Sincronizar y copiar al destino
echo "[+] Sincronizando buffers..."
sync

# Copiar al directorio de salida
cp "${WORK_DIR}/base-rootfs.ext4" "$OUTPUT_FILE"

# Mostrar información del rootfs
echo ""
echo "========================================="
echo "  Base rootfs generado correctamente"
echo "========================================="
echo "[+] Ubicación: ${OUTPUT_FILE}"
echo "[+] Tamaño: $(du -h "$OUTPUT_FILE" | cut -f1)"
echo "[+] Sistema: Debian ${DEBIAN_RELEASE}"
echo ""
echo "Para usarlo, configura la variable de entorno:"
echo "  export FC_BASE_ROOTFS=${OUTPUT_FILE}"
echo ""
echo "Para inyectar claves SSH maestras (acceso a TODAS las VMs):"
echo "  export SSH_PUBLIC_KEYS=\"ssh-ed25519 AAAA... tu@host\""
echo "  (separar múltiples claves con nueva línea)"
echo ""
echo "Para inyectar claves por VM:"
echo "  Añadir 'ssh_public_keys' al VmConfig del deploy"
