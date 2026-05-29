use crate::domain::{
    CreateDatabaseParams, Database, DatabaseDeployment, DatabaseRepository, DatabaseStatus,
    DomainError, DomainResult,
};
use crate::infrastructure::db::models::{DbDatabase, DbDatabaseDeployment};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresDatabaseRepository {
    pool: PgPool,
}

impl PostgresDatabaseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DatabaseRepository for PostgresDatabaseRepository {
    async fn create_database(&self, params: CreateDatabaseParams) -> DomainResult<Database> {
        let db = sqlx::query_as::<_, DbDatabase>(
            r#"
            INSERT INTO databases (name, engine, user_id, vcpus, memory_mib, disk_mib, settings)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(params.name)
        .bind(params.engine)
        .bind(params.user_id)
        .bind(params.vcpus.value() as i32)
        .bind(params.memory_mib.value() as i32)
        .bind(params.disk_mib as i32)
        .bind(serde_json::to_value(params.settings).unwrap_or_default())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.into())
    }

    async fn get_database(&self, id: Uuid) -> DomainResult<Option<Database>> {
        let db = sqlx::query_as::<_, DbDatabase>("SELECT * FROM databases WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.map(|d| d.into()))
    }

    async fn get_database_by_name(
        &self,
        user_id: Uuid,
        name: &str,
    ) -> DomainResult<Option<Database>> {
        let db = sqlx::query_as::<_, DbDatabase>(
            "SELECT * FROM databases WHERE user_id = $1 AND name = $2",
        )
        .bind(user_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.map(|d| d.into()))
    }

    async fn list_databases_by_user(&self, user_id: Uuid) -> DomainResult<Vec<Database>> {
        let dbs = sqlx::query_as::<_, DbDatabase>("SELECT * FROM databases WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(dbs.into_iter().map(|d| d.into()).collect())
    }

    async fn delete_database(&self, id: Uuid) -> DomainResult<()> {
        sqlx::query("DELETE FROM databases WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(())
    }

    async fn update_database_status(&self, id: Uuid, status: DatabaseStatus) -> DomainResult<()> {
        let status_str = match status {
            DatabaseStatus::Pending => "pending",
            DatabaseStatus::Running => "running",
            DatabaseStatus::Failed => "failed",
            DatabaseStatus::Deleting => "deleting",
        };

        sqlx::query("UPDATE databases SET status = $1, updated_at = NOW() WHERE id = $2")
            .bind(status_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(())
    }

    async fn update_active_deployment(&self, db_id: Uuid, deployment_id: Uuid) -> DomainResult<()> {
        sqlx::query(
            "UPDATE databases SET active_deployment_id = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(deployment_id)
        .bind(db_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(())
    }

    async fn create_deployment(
        &self,
        db_id: Uuid,
        user_id: Uuid,
    ) -> DomainResult<DatabaseDeployment> {
        let deployment = sqlx::query_as::<_, DbDatabaseDeployment>(
            r#"
            INSERT INTO database_deployments (database_id, user_id, status)
            VALUES ($1, $2, 'PENDING')
            RETURNING *
            "#,
        )
        .bind(db_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(deployment.into())
    }

    async fn get_deployment(&self, id: Uuid) -> DomainResult<Option<DatabaseDeployment>> {
        let deployment = sqlx::query_as::<_, DbDatabaseDeployment>(
            "SELECT * FROM database_deployments WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(deployment.map(|d| d.into()))
    }

    async fn update_deployment_status(&self, id: Uuid, status: &str) -> DomainResult<()> {
        sqlx::query(
            "UPDATE database_deployments SET status = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(status)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(())
    }

    async fn update_deployment_job_info(
        &self,
        id: Uuid,
        job_id: &str,
        host_id: &str,
        vm_id: &str,
    ) -> DomainResult<()> {
        sqlx::query(
            r#"
            UPDATE database_deployments 
            SET job_id = $1, host_id = $2, vm_id = $3, updated_at = NOW() 
            WHERE id = $4
            "#,
        )
        .bind(job_id)
        .bind(host_id)
        .bind(vm_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(())
    }
}
