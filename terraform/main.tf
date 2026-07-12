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

# Instancia de GCE con virtualización anidada habilitada
resource "google_compute_instance" "mikrom" {
  name         = var.instance_name
  machine_type = var.machine_type
  zone         = var.zone
  tags         = ["mikrom"]

  boot_disk {
    initialize_params {
      image = "debian-cloud/debian-12"
      size  = var.disk_size_gb
      type  = "pd-ssd"
    }
  }

  network_interface {
    network = "default"
    access_config {
      # Asigna una dirección IP pública efímera
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
  }

  metadata_startup_script = file("${path.module}/../scripts/gcloud-startup.sh")

  depends_on = [google_project_service.compute]
}
