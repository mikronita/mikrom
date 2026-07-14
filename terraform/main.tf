# Habilitar la API de Compute Engine si no está ya habilitada
resource "google_project_service" "compute" {
  service                    = "compute.googleapis.com"
  disable_on_destroy         = false
}

# Regla de firewall específica para la VM de Mikrom (usando target_tags)
resource "google_compute_firewall" "mikrom_rules" {
  name        = "mikrom-rules"
  network     = "default"
  description = "Reglas de acceso seguro para la plataforma Mikrom (HTTP, HTTPS, SSH y WireGuard)"

  allow {
    protocol = "tcp"
    ports    = ["22", "80", "443"]
  }

  allow {
    protocol = "udp"
    ports    = ["51820-51825"]
  }

  source_ranges = ["0.0.0.0/0"]
  target_tags   = ["mikrom"]

  depends_on = [google_project_service.compute]
}

# Dirección IP estática externa para mantener la IP estable entre recreaciones de la instancia Spot
resource "google_compute_address" "mikrom_ip" {
  name        = "${var.instance_name}-ip"
  region      = var.region
  description = "Dirección IP pública estática para Mikrom"

  depends_on = [google_project_service.compute]
}

# Plantilla de instancia de GCE con virtualización anidada habilitada
resource "google_compute_instance_template" "mikrom_template" {
  name_prefix  = "${var.instance_name}-template-"
  machine_type = var.machine_type
  tags         = ["mikrom"]

  disk {
    source_image = "debian-cloud/debian-13"
    auto_delete  = true
    boot         = true
    disk_size_gb = var.disk_size_gb
    disk_type    = "pd-ssd"
  }

  network_interface {
    network = "default"
    access_config {
      nat_ip = google_compute_address.mikrom_ip.address
    }
  }

  scheduling {
    preemptible        = var.use_spot
    automatic_restart  = var.use_spot ? false : true
    provisioning_model = var.use_spot ? "SPOT" : "STANDARD"
  }

  advanced_machine_features {
    enable_nested_virtualization = true
  }

  metadata = {
    git-repo          = var.git_repo
    git-branch        = var.git_branch
    git-token         = var.git_token
    ssh-public-keys   = var.ssh_public_keys
    acme-staging      = var.acme_staging
    db-host           = google_sql_database_instance.mikrom_db.public_ip_address
    db-user           = google_sql_user.mikrom_db_user.name
    db-password       = google_sql_user.mikrom_db_user.password
    db-name-api       = google_sql_database.mikrom_api_db.name
    db-name-scheduler = google_sql_database.mikrom_scheduler_db.name
    db-name-router    = google_sql_database.mikrom_router_db.name
    master-key        = random_id.master_key.hex
    jwt-secret        = random_id.jwt_secret.hex
  }

  metadata_startup_script = file("${path.module}/../scripts/gcloud-startup.sh")

  lifecycle {
    create_before_destroy = true
  }

  depends_on = [
    google_project_service.compute,
    google_sql_database_instance.mikrom_db,
    google_sql_user.mikrom_db_user,
    google_sql_database.mikrom_api_db,
    google_sql_database.mikrom_scheduler_db,
    google_sql_database.mikrom_router_db
  ]
}

# Grupo de Instancias Administrado (MIG) con tamaño objetivo de 1
resource "google_compute_instance_group_manager" "mikrom_mig" {
  name               = "${var.instance_name}-mig"
  base_instance_name = var.instance_name
  zone               = var.zone

  version {
    instance_template = google_compute_instance_template.mikrom_template.id
  }

  target_size = 1

  depends_on = [google_project_service.compute]
}

# Habilitar la API de Cloud SQL Admin
resource "google_project_service" "sqladmin" {
  service            = "sqladmin.googleapis.com"
  disable_on_destroy = false
}

# Generar un sufijo aleatorio para el nombre de la instancia de base de datos
resource "random_id" "db_name_suffix" {
  byte_length = 4
}

# Generar una contraseña aleatoria y segura para la base de datos
resource "random_password" "db_password" {
  length           = 16
  special          = true
  override_special = "!#$%&*()-_=+[]{}<>:?"
}

# Generar una clave maestra de encriptación persistente
resource "random_id" "master_key" {
  byte_length = 32
}

# Generar una clave secreta JWT persistente
resource "random_id" "jwt_secret" {
  byte_length = 32
}

# Instancia de Cloud SQL para PostgreSQL 17
resource "google_sql_database_instance" "mikrom_db" {
  name             = "${var.instance_name}-db-${random_id.db_name_suffix.hex}"
  database_version = "POSTGRES_17"
  region           = var.region

  settings {
    tier    = var.db_tier
    edition = "ENTERPRISE"

    ip_configuration {
      ipv4_enabled = true
      # Permitir el acceso únicamente a la IP pública estática de la VM de Mikrom
      authorized_networks {
        name  = "mikrom-vm-ip"
        value = "${google_compute_address.mikrom_ip.address}/32"
      }
    }
  }

  depends_on = [google_project_service.sqladmin]
}


# Usuario de base de datos principal
resource "google_sql_user" "mikrom_db_user" {
  name     = "mikrom"
  instance = google_sql_database_instance.mikrom_db.name
  password = random_password.db_password.result
}

# Base de datos para el servicio de API
resource "google_sql_database" "mikrom_api_db" {
  name     = "mikrom_api"
  instance = google_sql_database_instance.mikrom_db.name
}

# Base de datos para el servicio de Scheduler
resource "google_sql_database" "mikrom_scheduler_db" {
  name     = "mikrom_scheduler"
  instance = google_sql_database_instance.mikrom_db.name
}

# Base de datos para el servicio de Router (Pingora Ingress)
resource "google_sql_database" "mikrom_router_db" {
  name     = "mikrom_router"
  instance = google_sql_database_instance.mikrom_db.name
}


