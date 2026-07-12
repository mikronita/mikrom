#!/usr/bin/env bash
set -euo pipefail

# Comprobación de dependencias
if ! command -v terraform &>/dev/null; then
    echo "[!] Error: 'terraform' no está instalado."
    echo "    Por favor, instálalo desde: https://learn.hashicorp.com/tutorials/terraform/install-cli"
    exit 1
fi

if ! command -v gcloud &>/dev/null; then
    echo "[!] Error: 'gcloud' no está instalado."
    echo "    Por favor, instala Google Cloud CLI antes de continuar."
    exit 1
fi

# Verificar si las credenciales de aplicación (ADC) están configuradas para Terraform
if ! gcloud auth application-default print-access-token &>/dev/null; then
    echo "================================================="
    echo "  Configuración de Credenciales de GCP"
    echo "================================================="
    echo "[*] Terraform requiere las Application Default Credentials (ADC) de GCP."
    echo "[*] Se abrirá el navegador para autenticarte..."
    echo ""
    gcloud auth application-default login
    echo "================================================="
fi

# Detectar configuración por defecto de GCP
PROJECT_ID=$(gcloud config get-value project 2>/dev/null || echo "")
ZONE="us-central1-a"
REGION="us-central1"
INSTANCE_NAME="mikrom-prod"
MACHINE_TYPE="n1-standard-2"  # N1 soporta virtualización anidada y es económica
DISK_SIZE=50
USE_SPOT="yes"                 # Habilita Spot VM para reducir costes ~80%

# Detectar repositorio y rama Git actual
DEFAULT_REPO="https://github.com/mikronita/mikrom.git"
CURRENT_REPO=$(git remote get-url origin 2>/dev/null || echo "$DEFAULT_REPO")
if [[ "$CURRENT_REPO" =~ ^git@github\.com:(.+)\.git$ ]]; then
    CURRENT_REPO="https://github.com/${BASH_REMATCH[1]}.git"
elif [[ "$CURRENT_REPO" =~ ^git@github\.com:(.+)$ ]]; then
    CURRENT_REPO="https://github.com/${BASH_REMATCH[1]}"
fi
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")

# Detectar clave SSH pública local
SSH_KEY_CONTENT=""
if [ -f "$HOME/.ssh/id_ed25519.pub" ]; then
    SSH_KEY_CONTENT=$(cat "$HOME/.ssh/id_ed25519.pub")
elif [ -f "$HOME/.ssh/id_rsa.pub" ]; then
    SSH_KEY_CONTENT=$(cat "$HOME/.ssh/id_rsa.pub")
fi

echo "================================================="
echo "  Despliegue de Mikrom en GCP (con Terraform)"
echo "================================================="

if [ -z "$PROJECT_ID" ]; then
    read -p "Introduce el ID de tu proyecto de GCP: " PROJECT_ID
fi

read -p "Región de GCE [$REGION]: " input_region
REGION=${input_region:-$REGION}

read -p "Zona de GCE [$ZONE]: " input_zone
ZONE=${input_zone:-$ZONE}

read -p "Nombre de la instancia [$INSTANCE_NAME]: " input_name
INSTANCE_NAME=${input_name:-$INSTANCE_NAME}

read -p "Tipo de máquina [$MACHINE_TYPE]: " input_machine
MACHINE_TYPE=${input_machine:-$MACHINE_TYPE}

read -p "Tamaño del disco (GB) [$DISK_SIZE]: " input_disk
DISK_SIZE=${input_disk:-$DISK_SIZE}

read -p "¿Usar Spot VM? (yes/no) [$USE_SPOT]: " input_spot
USE_SPOT=${input_spot:-$USE_SPOT}

read -p "Repositorio Git [$CURRENT_REPO]: " input_repo
GIT_REPO=${input_repo:-$CURRENT_REPO}

read -p "Rama/Commit Git [$CURRENT_BRANCH]: " input_branch
GIT_BRANCH=${input_branch:-$CURRENT_BRANCH}

GIT_TOKEN=""
# Preguntar por token si no es el repo por defecto público
if [[ "$GIT_REPO" != *"github.com/mikrom-platform/mikrom.git"* && "$GIT_REPO" != *"github.com/mikronita/mikrom.git"* ]]; then
    read -sp "Token de Github (opcional para repos privados, Enter para omitir): " GIT_TOKEN
    echo ""
fi

# Preguntar por clave SSH pública
if [ -n "$SSH_KEY_CONTENT" ]; then
    read -p "¿Deseas inyectar tu clave SSH pública en las MicroVMs? (yes/no) [yes]: " inject_ssh
    inject_ssh=${inject_ssh:-"yes"}
    if [ "$inject_ssh" != "yes" ]; then
        SSH_KEY_CONTENT=""
    fi
else
    read -p "Clave SSH pública para inyectar en las MicroVMs (opcional, Enter para omitir): " input_ssh
    SSH_KEY_CONTENT=${input_ssh:-""}
fi

# Convertir Spot VM a booleano de Terraform
if [ "$USE_SPOT" = "yes" ]; then
    T_USE_SPOT="true"
else
    T_USE_SPOT="false"
fi

# Obtener directorio de Terraform
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="$SCRIPT_DIR/../terraform"

echo "[*] Generando archivo de variables terraform.tfvars..."
cat > "$TF_DIR/terraform.tfvars" <<EOF
project_id      = "${PROJECT_ID}"
region          = "${REGION}"
zone            = "${ZONE}"
instance_name   = "${INSTANCE_NAME}"
machine_type    = "${MACHINE_TYPE}"
disk_size_gb    = ${DISK_SIZE}
use_spot        = ${T_USE_SPOT}
git_repo        = "${GIT_REPO}"
git_branch      = "${GIT_BRANCH}"
git_token       = "${GIT_TOKEN}"
ssh_public_keys = <<EOT
${SSH_KEY_CONTENT}
EOT
EOF

echo "[*] Inicializando Terraform..."
terraform -chdir="$TF_DIR" init

echo "================================================="
echo "  Ejecutando terraform apply"
echo "  (Revisa el plan de cambios y escribe 'yes' para aplicar)"
echo "================================================="
terraform -chdir="$TF_DIR" apply
