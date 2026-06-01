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
            INSERT INTO databases (
                name, engine, user_id, tenant_id, vcpus, memory_mib, disk_mib, status, neon_tenant_id, neon_timeline_id, tenant_gen, settings
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9, $10, $11)
            RETURNING id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at
            "#,
        )
        .bind(params.name)
        .bind(params.engine)
        .bind(params.user_id)
        .bind(params.tenant_id)
        .bind(params.vcpus.value() as i32)
        .bind(params.memory_mib.value() as i32)
        .bind(params.disk_mib as i32)
        .bind(params.neon_tenant_id)
        .bind(params.neon_timeline_id)
        .bind(params.tenant_gen.map(|value| value as i32))
        .bind(serde_json::to_value(params.settings).unwrap_or_default())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.into())
    }

    async fn get_database(&self, id: Uuid) -> DomainResult<Option<Database>> {
        let db = sqlx::query_as::<_, DbDatabase>(
            "SELECT id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at FROM databases WHERE id = $1",
        )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.map(|d| d.into()))
    }

    async fn get_database_by_neon_tenant_id(
        &self,
        neon_tenant_id: &str,
    ) -> DomainResult<Option<Database>> {
        let db =
            sqlx::query_as::<_, DbDatabase>("SELECT id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at FROM databases WHERE neon_tenant_id = $1 LIMIT 1")
                .bind(neon_tenant_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.map(|d| d.into()))
    }

    async fn list_databases(&self) -> DomainResult<Vec<Database>> {
        let dbs = sqlx::query_as::<_, DbDatabase>("SELECT id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at FROM databases ORDER BY created_at")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(dbs.into_iter().map(|d| d.into()).collect())
    }

    async fn get_database_by_name(
        &self,
        tenant_id: Uuid,
        name: &str,
    ) -> DomainResult<Option<Database>> {
        let db = sqlx::query_as::<_, DbDatabase>(
            "SELECT id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at FROM databases WHERE tenant_id = $1 AND name = $2",
        )
        .bind(tenant_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(db.map(|d| d.into()))
    }

    async fn list_databases_by_tenant(&self, tenant_id: Uuid) -> DomainResult<Vec<Database>> {
        let dbs = sqlx::query_as::<_, DbDatabase>("SELECT id, name, engine, tenant_id, vcpus, memory_mib, disk_mib, neon_tenant_id, neon_timeline_id, tenant_gen, settings, status, active_deployment_id, created_at, updated_at FROM databases WHERE tenant_id = $1")
            .bind(tenant_id)
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

    async fn update_database_provisioning(
        &self,
        id: Uuid,
        neon_tenant_id: &str,
        neon_timeline_id: &str,
        tenant_gen: u32,
    ) -> DomainResult<()> {
        sqlx::query("UPDATE databases SET neon_tenant_id = $1, neon_timeline_id = $2, tenant_gen = $3, updated_at = NOW() WHERE id = $4")
            .bind(neon_tenant_id)
            .bind(neon_timeline_id)
            .bind(tenant_gen as i32)
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

    // Deployment operations
    async fn create_deployment(
        &self,
        db_id: Uuid,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> DomainResult<DatabaseDeployment> {
        let deployment = sqlx::query_as::<_, DbDatabaseDeployment>(
            r#"
            INSERT INTO database_deployments (database_id, user_id, tenant_id, status)
            VALUES ($1, $2, $3, 'PENDING')
            RETURNING id, database_id, tenant_id, job_id, status, host_id, vm_id, ipv6_address, created_at, updated_at
            "#,
        )
        .bind(db_id)
        .bind(user_id)
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Infrastructure(e.to_string()))?;

        Ok(deployment.into())
    }

    async fn get_deployment(&self, id: Uuid) -> DomainResult<Option<DatabaseDeployment>> {
        let deployment = sqlx::query_as::<_, DbDatabaseDeployment>(
            "SELECT id, database_id, tenant_id, job_id, status, host_id, vm_id, ipv6_address, created_at, updated_at FROM database_deployments WHERE id = $1",
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
        sqlx::query("UPDATE database_deployments SET job_id = $1, host_id = $2, vm_id = $3, updated_at = NOW() WHERE id = $4")
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
