output "public_ip" {
  value       = google_compute_instance.mikrom.network_interface[0].access_config[0].nat_ip
  description = "Dirección IP pública asignada a la VM de Mikrom"
}

output "dashboard_url" {
  value       = "https://dashboard.${google_compute_instance.mikrom.network_interface[0].access_config[0].nat_ip}.sslip.io"
  description = "URL de acceso al panel de control de Mikrom"
}

output "api_url" {
  value       = "https://api.${google_compute_instance.mikrom.network_interface[0].access_config[0].nat_ip}.sslip.io"
  description = "URL de acceso a la API REST de Mikrom"
}
