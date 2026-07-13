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
    git-repo        = var.git_repo
    git-branch      = var.git_branch
    git-token       = var.git_token
    ssh-public-keys = var.ssh_public_keys
    acme-staging    = var.acme_staging
  }

  metadata_startup_script = file("${path.module}/../scripts/gcloud-startup.sh")

  lifecycle {
    create_before_destroy = true
  }

  depends_on = [google_project_service.compute]
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

