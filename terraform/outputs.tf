output "public_ip" {
  value       = google_compute_address.mikrom_ip.address
  description = "Dirección IP pública asignada a la VM de Mikrom"
}

output "dashboard_url" {
  value       = "https://mikrom.spluca.org"
  description = "URL de acceso al panel de control de Mikrom"
}

output "api_url" {
  value       = "https://api.mikrom.spluca.org"
  description = "URL de acceso a la API REST de Mikrom"
}

output "db_instance_ip" {
  value       = google_sql_database_instance.mikrom_db.public_ip_address
  description = "Dirección IP pública de la instancia de Cloud SQL"
}

output "db_instance_connection_name" {
  value       = google_sql_database_instance.mikrom_db.connection_name
  description = "Nombre de conexión de la instancia de Cloud SQL"
}

