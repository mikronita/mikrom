use crate::domain::types::{CpuCores, MemoryMb};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Database {
    pub id: Uuid,
    pub name: String,
    pub engine: String,
    pub user_id: Uuid,
    pub vcpus: CpuCores,
    pub memory_mib: MemoryMb,
    pub disk_mib: u32,
    pub settings: std::collections::HashMap<String, String>,
    pub status: DatabaseStatus,
    pub active_deployment_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseStatus {
    Pending,
    Running,
    Failed,
    Deleting,
}

impl From<String> for DatabaseStatus {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "failed" => Self::Failed,
            "deleting" => Self::Deleting,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseDeployment {
    pub id: Uuid,
    pub database_id: Uuid,
    pub user_id: Uuid,
    pub job_id: Option<String>,
    pub status: String,
    pub host_id: Option<String>,
    pub vm_id: Option<String>,
    pub ipv6_address: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct CreateDatabaseParams {
    pub name: String,
    pub engine: String,
    pub user_id: Uuid,
    pub vcpus: CpuCores,
    pub memory_mib: MemoryMb,
    pub disk_mib: u32,
    pub settings: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
pub trait DatabaseRepository: Send + Sync {
    async fn create_database(
        &self,
        params: CreateDatabaseParams,
    ) -> crate::domain::DomainResult<Database>;
    async fn get_database(&self, id: Uuid) -> crate::domain::DomainResult<Option<Database>>;
    async fn get_database_by_name(
        &self,
        user_id: Uuid,
        name: &str,
    ) -> crate::domain::DomainResult<Option<Database>>;
    async fn list_databases_by_user(
        &self,
        user_id: Uuid,
    ) -> crate::domain::DomainResult<Vec<Database>>;
    async fn delete_database(&self, id: Uuid) -> crate::domain::DomainResult<()>;
    async fn update_database_status(
        &self,
        id: Uuid,
        status: DatabaseStatus,
    ) -> crate::domain::DomainResult<()>;
    async fn update_active_deployment(
        &self,
        db_id: Uuid,
        deployment_id: Uuid,
    ) -> crate::domain::DomainResult<()>;

    // Deployment operations
    async fn create_deployment(
        &self,
        db_id: Uuid,
        user_id: Uuid,
    ) -> crate::domain::DomainResult<DatabaseDeployment>;
    async fn get_deployment(
        &self,
        id: Uuid,
    ) -> crate::domain::DomainResult<Option<DatabaseDeployment>>;
    async fn update_deployment_status(
        &self,
        id: Uuid,
        status: &str,
    ) -> crate::domain::DomainResult<()>;
    async fn update_deployment_job_info(
        &self,
        id: Uuid,
        job_id: &str,
        host_id: &str,
        vm_id: &str,
    ) -> crate::domain::DomainResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn database_status_from_string_is_case_insensitive() {
        assert_eq!(
            DatabaseStatus::from("pending".to_string()),
            DatabaseStatus::Pending
        );
        assert_eq!(
            DatabaseStatus::from("RUNNING".to_string()),
            DatabaseStatus::Running
        );
        assert_eq!(
            DatabaseStatus::from("Failed".to_string()),
            DatabaseStatus::Failed
        );
        assert_eq!(
            DatabaseStatus::from("deleting".to_string()),
            DatabaseStatus::Deleting
        );
        assert_eq!(
            DatabaseStatus::from("unknown".to_string()),
            DatabaseStatus::Pending
        );
    }

    #[test]
    fn database_roundtrip_serializes_settings_and_status() {
        let db = Database {
            id: Uuid::new_v4(),
            name: "db-1".to_string(),
            engine: "neon".to_string(),
            user_id: Uuid::new_v4(),
            vcpus: CpuCores::try_from(2).unwrap(),
            memory_mib: MemoryMb::try_from(1024).unwrap(),
            disk_mib: 4096,
            settings: HashMap::from([
                ("max_connections".to_string(), "100".to_string()),
                ("shared_buffers".to_string(), "256MB".to_string()),
            ]),
            status: DatabaseStatus::Running,
            active_deployment_id: Some(Uuid::new_v4()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let json = serde_json::to_value(&db).unwrap();
        assert_eq!(json["status"], "running");
        assert_eq!(json["settings"]["max_connections"], "100");

        let decoded: Database = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, db);
    }
}
