use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct DbApp {
    pub id: Uuid,
    pub name: String,
    pub git_url: String,
    pub port: i32,
    pub hostname: Option<String>,
    pub user_id: Uuid,
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
            user_id: db.user_id,
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
    pub user_id: Uuid,
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
            user_id: db.user_id,
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
    pub user_id: Uuid,
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
            user_id: db.user_id,
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
    pub user_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl From<DbVolumeSnapshot> for crate::domain::volume::VolumeSnapshot {
    fn from(db: DbVolumeSnapshot) -> Self {
        Self {
            id: db.id,
            volume_id: db.volume_id,
            user_id: db.user_id,
            name: db.name,
            created_at: db.created_at,
        }
    }
}
