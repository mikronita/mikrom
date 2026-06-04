use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbTenant {
    pub id: Uuid,
    pub tenant_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbTenant> for crate::domain::tenant::Tenant {
    fn from(db: DbTenant) -> Self {
        Self {
            id: db.id,
            tenant_id: db.tenant_id,
            name: db.name,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbTenantMember {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
}

impl From<DbTenantMember> for crate::domain::tenant::TenantMember {
    fn from(db: DbTenantMember) -> Self {
        Self {
            tenant_id: db.tenant_id,
            user_id: db.user_id,
            role: db.role,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbApp {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub tenant_id: Uuid,
    pub github_webhook_secret: Option<String>,
    pub github_installation_id: Option<i64>,
    pub github_repo_id: Option<i64>,
    pub github_repo_full_name: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub health_check_path: String,
    pub drain_timeout: i32,
    pub desired_replicas: i32,
    pub min_replicas: i32,
    pub max_replicas: i32,
    pub autoscaling_enabled: bool,
    pub cpu_threshold: f64,
    pub mem_threshold: f64,
    pub last_router_traffic_at: i64,
    pub last_scaled_to_zero_at: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbApp> for crate::domain::app::App {
    fn from(db: DbApp) -> Self {
        Self {
            id: db.id,
            name: db.name,
            git_url: db.git_url,
            port: db
                .port
                .try_into()
                .expect("Database contains invalid port for App"),
            hostname: db.hostname,
            tenant_id: db.tenant_id,
            github_webhook_secret: db.github_webhook_secret,
            github_installation_id: db.github_installation_id,
            github_repo_id: db.github_repo_id,
            github_repo_full_name: db.github_repo_full_name,
            active_deployment_id: db.active_deployment_id,
            health_check_path: db.health_check_path,
            drain_timeout: db.drain_timeout,
            desired_replicas: db.desired_replicas,
            min_replicas: db.min_replicas,
            max_replicas: db.max_replicas,
            autoscaling_enabled: db.autoscaling_enabled,
            cpu_threshold: db.cpu_threshold,
            mem_threshold: db.mem_threshold,
            last_router_traffic_at: db.last_router_traffic_at,
            last_scaled_to_zero_at: db.last_scaled_to_zero_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbDeployment {
    pub id: Uuid,
    pub app_id: Uuid,
    pub tenant_id: Uuid,
    pub build_id: Option<String>,
    pub image_tag: Option<String>,
    pub job_id: Option<String>,
    pub ipv6_address: Option<String>,
    pub status: String,
    pub vcpus: i32,
    pub memory_mib: i64,
    pub disk_mib: i64,
    pub port: i32,
    pub env_vars: serde_json::Value,
    pub git_commit_hash: Option<String>,
    pub git_commit_message: Option<String>,
    pub git_branch: Option<String>,
    pub trigger_source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hypervisor: i32,
}

impl From<DbDeployment> for crate::domain::app::Deployment {
    fn from(db: DbDeployment) -> Self {
        Self {
            id: db.id,
            app_id: db.app_id,
            tenant_id: db.tenant_id,
            build_id: db.build_id,
            image_tag: db.image_tag,
            job_id: db.job_id,
            ipv6_address: db.ipv6_address,
            status: db.status,
            vcpus: db
                .vcpus
                .try_into()
                .expect("Database contains invalid vcpus for Deployment"),
            memory_mib: db
                .memory_mib
                .try_into()
                .expect("Database contains invalid memory_mib for Deployment"),
            disk_mib: db.disk_mib,
            port: db
                .port
                .try_into()
                .expect("Database contains invalid port for Deployment"),
            env_vars: db.env_vars,
            git_commit_hash: db.git_commit_hash,
            git_commit_message: db.git_commit_message,
            git_branch: db.git_branch,
            trigger_source: db.trigger_source,
            created_at: db.created_at,
            updated_at: db.updated_at,
            hypervisor: db.hypervisor,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbSecurityRule {
    pub id: Uuid,
    pub app_id: Uuid,
    pub protocol: String,
    pub port_start: i32,
    pub port_end: i32,
    pub action: String,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbSecurityRule> for crate::domain::app::SecurityRule {
    fn from(db: DbSecurityRule) -> Self {
        Self {
            id: db.id,
            app_id: db.app_id,
            protocol: db.protocol,
            port_start: db
                .port_start
                .try_into()
                .expect("Database contains invalid port_start for SecurityRule"),
            port_end: db
                .port_end
                .try_into()
                .expect("Database contains invalid port_end for SecurityRule"),
            action: db.action,
            priority: db.priority,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbUserGithubAccount {
    pub id: Uuid,
    pub user_id: Uuid,
    pub installation_id: i64,
    pub github_username: String,
    pub created_at: DateTime<Utc>,
}

impl From<DbUserGithubAccount> for crate::domain::github::UserGithubAccount {
    fn from(db: DbUserGithubAccount) -> Self {
        Self {
            id: db.id,
            user_id: db.user_id,
            installation_id: db.installation_id,
            github_username: db.github_username,
            created_at: db.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbVolume {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub size_mib: i32,
    pub pool_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbVolume> for crate::domain::volume::Volume {
    fn from(db: DbVolume) -> Self {
        Self {
            id: db.id,
            tenant_id: db.tenant_id,
            name: db.name,
            size_mib: db.size_mib,
            pool_name: db.pool_name,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbAppVolume {
    pub app_id: Uuid,
    pub volume_id: Uuid,
    pub mount_point: String,
    pub access_mode: i32,
    pub created_at: DateTime<Utc>,
}

impl From<DbAppVolume> for crate::domain::volume::AppVolume {
    fn from(db: DbAppVolume) -> Self {
        Self {
            app_id: db.app_id,
            volume_id: db.volume_id,
            mount_point: db.mount_point,
            access_mode: db.access_mode,
            created_at: db.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbVolumeSnapshot {
    pub id: Uuid,
    pub volume_id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl From<DbVolumeSnapshot> for crate::domain::volume::VolumeSnapshot {
    fn from(db: DbVolumeSnapshot) -> Self {
        Self {
            id: db.id,
            volume_id: db.volume_id,
            tenant_id: db.tenant_id,
            name: db.name,
            created_at: db.created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbDatabase {
    pub id: Uuid,
    pub name: String,
    pub engine: String,
    pub postgres_version: i32,
    pub tenant_id: Uuid,
    pub vcpus: i32,
    pub memory_mib: i32,
    pub disk_mib: i32,
    pub neon_tenant_id: Option<String>,
    pub neon_timeline_id: Option<String>,
    pub tenant_gen: Option<i32>,
    pub settings: serde_json::Value,
    pub status: String,
    pub active_deployment_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbDatabase> for crate::domain::Database {
    fn from(db: DbDatabase) -> Self {
        Self {
            id: db.id,
            name: db.name,
            engine: db.engine,
            postgres_version: db.postgres_version as u16,
            tenant_id: db.tenant_id,
            vcpus: crate::domain::types::CpuCores::try_from(db.vcpus as u32)
                .expect("Invalid vcpus"),
            memory_mib: crate::domain::types::MemoryMb::try_from(db.memory_mib as u32)
                .expect("Invalid memory_mib"),
            disk_mib: db.disk_mib as u32,
            neon_tenant_id: db.neon_tenant_id,
            neon_timeline_id: db.neon_timeline_id,
            tenant_gen: db.tenant_gen.map(|value| value as u32),
            settings: serde_json::from_value(db.settings).unwrap_or_default(),
            status: crate::domain::DatabaseStatus::from(db.status),
            active_deployment_id: db.active_deployment_id,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbDatabaseDeployment {
    pub id: Uuid,
    pub database_id: Uuid,
    pub tenant_id: Uuid,
    pub job_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub ipv6_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<DbDatabaseDeployment> for crate::domain::DatabaseDeployment {
    fn from(db: DbDatabaseDeployment) -> Self {
        Self {
            id: db.id,
            database_id: db.database_id,
            tenant_id: db.tenant_id,
            job_id: db.job_id,
            status: db.status,
            host_id: db.host_id,
            vm_id: db.vm_id,
            ipv6_address: db.ipv6_address,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[cfg(any())]
mod tests {
    use super::*;
    use crate::domain::{
        DatabaseStatus,
        types::{CpuCores, MemoryMb},
    };
    use std::collections::HashMap;

    #[test]
    fn db_database_converts_to_domain_database() {
        let db = DbDatabase {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: 2,
            memory_mib: 1024,
            disk_mib: 4096,
            tenant_id: Some("11111111111111111111111111111111".to_string()),
            timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: serde_json::json!({"max_connections": "200"}),
            status: "running".to_string(),
            active_deployment_id: Some(Uuid::new_v4()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let domain: crate::domain::Database = db.into();
        assert_eq!(domain.vcpus, CpuCores::try_from(2).unwrap());
        assert_eq!(domain.memory_mib, MemoryMb::try_from(1024).unwrap());
        assert_eq!(domain.status, DatabaseStatus::Running);
        assert_eq!(
            domain.tenant_id.as_deref(),
            Some("11111111111111111111111111111111")
        );
        assert_eq!(
            domain.timeline_id.as_deref(),
            Some("22222222222222222222222222222222")
        );
        assert_eq!(
            domain.settings,
            HashMap::from([("max_connections".to_string(), "200".to_string())])
        );
    }

    #[test]
    fn db_database_deployment_converts_to_domain_deployment() {
        let db = DbDatabaseDeployment {
            id: Uuid::new_v4(),
            database_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            job_id: Some("job-1".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: Some("fd00::1".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let domain: crate::domain::DatabaseDeployment = db.into();
        assert_eq!(domain.job_id.as_deref(), Some("job-1"));
        assert_eq!(domain.host_id.as_deref(), Some("host-1"));
        assert_eq!(domain.vm_id.as_deref(), Some("vm-1"));
        assert_eq!(domain.ipv6_address.as_deref(), Some("fd00::1"));
    }
}
