#!/usr/bin/env bash
set -euo pipefail

# Configuración por defecto (Optimizado para bajo costo)
PROJECT_ID=$(gcloud config get-value project 2>/dev/null || echo "")
ZONE="us-central1-a"
INSTANCE_NAME="mikrom-prod"
MACHINE_TYPE="n1-standard-2"  # N1 soporta virtualización anidada y es muy económica
DISK_SIZE="50GB"
USE_SPOT="yes"                 # Habilita Spot VM para reducir costes ~80%

echo "================================================="
echo "  Despliegue automatizado de Mikrom en GCE"
echo "================================================="

if [ -z "$PROJECT_ID" ]; then
    read -p "Introduce el ID de tu proyecto de GCP: " PROJECT_ID
fi

read -p "Zona de GCE [$ZONE]: " input_zone
ZONE=${input_zone:-$ZONE}

read -p "Nombre de la instancia [$INSTANCE_NAME]: " input_name
INSTANCE_NAME=${input_name:-$INSTANCE_NAME}

read -p "Tipo de máquina [$MACHINE_TYPE]: " input_machine
MACHINE_TYPE=${input_machine:-$MACHINE_TYPE}

read -p "Tamaño del disco [$DISK_SIZE]: " input_disk
DISK_SIZE=${input_disk:-$DISK_SIZE}

read -p "¿Usar Spot VM (preemptive) para reducir costos significativamente? (yes/no) [$USE_SPOT]: " input_spot
USE_SPOT=${input_spot:-$USE_SPOT}

SPOT_FLAGS=""
if [ "$USE_SPOT" = "yes" ]; then
    SPOT_FLAGS="--provisioning-model=SPOT --instance-termination-action=TERMINATE"
fi

echo "[*] Habilitando API de Compute Engine..."
gcloud services enable compute.googleapis.com --project="$PROJECT_ID"

echo "[*] Creando reglas de Firewall para Mikrom..."
# Crear regla para HTTP, HTTPS, NATS, SSH y Wireguard
gcloud compute firewall-rules create mikrom-rules \
    --project="$PROJECT_ID" \
    --allow=tcp:22,tcp:80,tcp:443,tcp:3001,tcp:5001,tcp:4222,udp:51820-51825 \
    --description="Reglas de acceso para la plataforma Mikrom" \
    --direction=INGRESS \
    --priority=1000 \
    --network=default \
    --action=ALLOW 2>/dev/null || echo "La regla de firewall ya existe."

echo "[*] Creando instancia de GCE con virtualización anidada..."
gcloud compute instances create "$INSTANCE_NAME" \
    --project="$PROJECT_ID" \
    --zone="$ZONE" \
    --machine-type="$MACHINE_TYPE" \
    --boot-disk-size="$DISK_SIZE" \
    --boot-disk-type="pd-ssd" \
    --image-family="debian-12" \
    --image-project="debian-cloud" \
    --enable-nested-virtualization \
    $SPOT_FLAGS \
    --metadata-from-file=startup-script=scripts/gcloud-startup.sh

echo "================================================="
echo "¡VM creada con éxito!"
echo "El script de aprovisionamiento interno tardará de 5 a 10 minutos."
echo "Puedes monitorizar el log de instalación conectándote por SSH y ejecutando:"
echo "  sudo tail -f /var/log/syslog | grep startup-script"
echo "================================================="
