variable "project_id" {
  type        = string
  description = "El ID del proyecto de Google Cloud (GCP)"
}

variable "region" {
  type        = string
  default     = "us-central1"
  description = "Región de Google Cloud para el despliegue"
}

variable "zone" {
  type        = string
  default     = "us-central1-a"
  description = "Zona específica de GCP para crear la instancia"
}

variable "instance_name" {
  type        = string
  default     = "mikrom-prod"
  description = "Nombre de la instancia de Google Compute Engine"
}

variable "machine_type" {
  type        = string
  default     = "n1-standard-2"
  description = "Tipo de máquina para la VM (debe soportar virtualización anidada, ej. N1 o N2)"
}

variable "disk_size_gb" {
  type        = number
  default     = 50
  description = "Tamaño del disco de arranque en GB"
}

variable "use_spot" {
  type        = bool
  default     = true
  description = "Si es true, se usará una Spot VM para reducir significativamente los costes (~80%)"
}

variable "git_repo" {
  type        = string
  default     = "https://github.com/mikronita/mikrom.git"
  description = "URL del repositorio Git de Mikrom a clonar en la VM"
}

variable "git_branch" {
  type        = string
  default     = "main"
  description = "Rama, tag o commit de Git que se desplegará en la VM"
}

variable "git_token" {
  type        = string
  default     = ""
  sensitive   = true
  description = "Token de GitHub (opcional) si el repositorio es privado"
}

variable "ssh_public_keys" {
  type        = string
  default     = ""
  description = "Claves SSH públicas maestras a inyectar en el base-rootfs de las microVMs (separadas por salto de línea)"
}

variable "acme_staging" {
  type        = bool
  default     = false
  description = "Define si se usa el entorno de pruebas (staging) de Let's Encrypt para certificados TLS"
}
