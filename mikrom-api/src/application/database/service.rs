use crate::AppState;
use crate::application::tenant::resolve_tenant_owner_user_id;
use crate::application::vms::{VmService, VmSnapshot};
use crate::domain::{CreateDatabaseParams, Database, DatabaseDeployment, DatabaseStatus};
use crate::error::{ApiError, ApiResult};
use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub struct DatabaseService;

const DATABASE_ROOTFS_IMAGE: &str = "local:/opt/neon";
const NEON_TENANT_ID_KEY: &str = "NEON_TENANT_ID";
const NEON_TIMELINE_ID_KEY: &str = "NEON_TIMELINE_ID";
const MIKROM_DATABASE_ID_KEY: &str = "MIKROM_DATABASE_ID";
const NEON_JWKS_JSON_KEY: &str = "NEON_JWKS_JSON";
const NEON_PAGESERVER_IPV6_KEY: &str = "NEON_PAGESERVER_IPV6";
const NEON_SAFEKEEPERS_GENERATION_KEY: &str = "NEON_SAFEKEEPERS_GENERATION";
const NEON_INSTANCE_ID_KEY: &str = "NEON_INSTANCE_ID";
const NEON_SAFEKEEPER_CONNSTRS_KEY: &str = "NEON_SAFEKEEPER_CONNSTRS";
const MIKROM_NEON_DEV_MODE_KEY: &str = "MIKROM_NEON_DEV_MODE";
const MIKROM_INIT_TRACE_FILES_KEY: &str = "MIKROM_INIT_TRACE_FILES";
const NEON_CONFIGURE_TOKEN_KEY: &str = "NEON_CONFIGURE_TOKEN";
const MIKROM_DATABASE_CONFIGURE_TOKEN_KEY: &str = "MIKROM_DATABASE_CONFIGURE_TOKEN";
const NEON_CONFIGURE_TOKEN_KID: &str = "mikrom-neon-key";
const NEON_CONFIGURE_TOKEN_ISSUER: &str = "mikrom-api";
const NEON_CONFIGURE_TOKEN_SUBJECT: &str = "mikrom-api";
const NEON_CONFIGURE_TOKEN_AUDIENCE: &str = "compute";
const NEON_CONFIGURE_TOKEN_SCOPE: &str = "compute_ctl:admin";
const NEON_CONFIGURE_TOKEN_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NeonConfigureClaims {
    iss: String,
    sub: String,
    aud: Vec<String>,
    iat: i64,
    exp: i64,
    compute_id: String,
    scope: String,
}

#[cfg(test)]
mod delete_database_tests {
    use super::*;
    use crate::application::ApiContext;
    use crate::domain::{
        Database, DatabaseDeployment, DatabaseStatus, MockDatabaseRepository, MockScheduler,
        MockTenantRepository, MockUserRepository, UserRepository,
    };
    use crate::infrastructure::nats::TypedNatsClient;
    use crate::{AppState, domain::DatabaseRepository};
    use mockall::predicate::eq;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn build_state(
        config: crate::config::ApiConfig,
        user_repo: Arc<dyn UserRepository>,
        tenant_repo: Arc<dyn crate::domain::TenantRepository>,
        database_repo: Arc<dyn DatabaseRepository>,
        scheduler: Arc<dyn crate::domain::Scheduler>,
    ) -> AppState {
        let ctx = ApiContext {
            user_repo: user_repo.clone(),
            tenant_repo: tenant_repo.clone(),
            app_repo: Arc::new(crate::domain::MockAppRepository::new()),
            database_repo: database_repo.clone(),
            github_repo: Arc::new(crate::domain::MockGithubRepository::new()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            plan_tier_repo: Arc::new(crate::domain::MockPlanTierRepository::new()),
            tenant_usage_repo: Arc::new(crate::domain::MockTenantUsageRepository::new()),
            personal_access_token_repo: Arc::new(
                crate::domain::MockPersonalAccessTokenRepository::new(),
            ),
            scheduler: scheduler.clone(),
            nats: TypedNatsClient::default(),
            db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            config: Arc::new(config.clone()),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
        };

        let (deployment_events, _) = tokio::sync::broadcast::channel(4);
        let (workspace_events, _) = tokio::sync::broadcast::channel(4);
        let (mesh_status, _) =
            tokio::sync::watch::channel(crate::application::vms::MeshStatus::default());

        AppState {
            ctx,
            user_repo,
            tenant_repo,
            app_repo: Arc::new(crate::domain::MockAppRepository::new()),
            database_repo,
            github_repo: Arc::new(crate::domain::MockGithubRepository::new()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler,
            nats: TypedNatsClient::default(),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events,
            workspace_events,
            mesh_status,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
        }
    }

    fn database(
        id: Uuid,
        tenant_id: Uuid,
        status: DatabaseStatus,
        active_deployment_id: Option<Uuid>,
    ) -> Database {
        Database {
            id,
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            tenant_id,
            vcpus: crate::domain::types::CpuCores::try_from(2).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(1024).unwrap(),
            disk_mib: 4096,
            neon_tenant_id: Some("11111111111111111111111111111111".to_string()),
            neon_timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::from([("max_connections".to_string(), "200".to_string())]),
            status,
            active_deployment_id,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn delete_database_deletes_active_deployment_before_row() {
        let tenant_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let active_db = database(
            database_id,
            tenant_id,
            DatabaseStatus::Running,
            Some(deployment_id),
        );
        let deployment = DatabaseDeployment {
            id: deployment_id,
            database_id,
            tenant_id,
            job_id: Some("job-123".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning(move |_| {
                let value = active_db.clone();
                Ok(Some(value))
            });
        db_repo
            .expect_get_deployment()
            .with(eq(deployment_id))
            .times(1)
            .returning(move |_| {
                let value = deployment.clone();
                Ok(Some(value))
            });
        db_repo
            .expect_delete_database()
            .with(eq(database_id))
            .times(1)
            .returning(|_| Ok(()));

        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_delete_database()
            .with(eq("job-123".to_string()), eq(tenant_id.to_string()))
            .times(1)
            .returning(|_, _| Ok(true));

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(MockTenantRepository::new()),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        DatabaseService::delete_database(&state, database_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_database_keeps_row_when_scheduler_rejects_cleanup() {
        let tenant_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let active_db = database(
            database_id,
            tenant_id,
            DatabaseStatus::Running,
            Some(deployment_id),
        );
        let deployment = DatabaseDeployment {
            id: deployment_id,
            database_id,
            tenant_id,
            job_id: Some("job-123".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning(move |_| {
                let value = active_db.clone();
                Ok(Some(value))
            });
        db_repo
            .expect_get_deployment()
            .with(eq(deployment_id))
            .times(1)
            .returning(move |_| {
                let value = deployment.clone();
                Ok(Some(value))
            });
        db_repo.expect_delete_database().times(0);

        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_delete_database()
            .with(eq("job-123".to_string()), eq(tenant_id.to_string()))
            .times(1)
            .returning(|_, _| Ok(false));

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(MockTenantRepository::new()),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        let err = DatabaseService::delete_database(&state, database_id)
            .await
            .unwrap_err();

        match err {
            ApiError::Scheduler(message) => {
                assert_eq!(message, "Scheduler rejected database deletion")
            },
            other => panic!("expected scheduler error, got {other:?}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, rovo::schemars::JsonSchema)]
pub struct DatabaseConnectionInfo {
    pub database_id: Uuid,
    pub database_name: String,
    pub database_user: String,
    pub database_host: String,
    pub database_port: u16,
    pub ssh_host: String,
    pub ssh_user: String,
    pub ssh_port: u16,
    pub ssh_tunnel_command: String,
    pub psql_command: String,
}

impl DatabaseService {
    pub async fn get_connection_info(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
    ) -> ApiResult<DatabaseConnectionInfo> {
        let database = state
            .ctx
            .database_repo
            .get_database(database_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found".to_string()))?;

        if database.tenant_id != tenant_id {
            return Err(ApiError::Forbidden);
        }

        let deployment_id = database.active_deployment_id.ok_or_else(|| {
            ApiError::Conflict("Database has no active deployment yet".to_string())
        })?;

        let deployment = state
            .ctx
            .database_repo
            .get_deployment(deployment_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database deployment not found".to_string()))?;

        if deployment.tenant_id != tenant_id {
            return Err(ApiError::Forbidden);
        }

        let ssh_host = deployment.ipv6_address.ok_or_else(|| {
            ApiError::Conflict("Database deployment does not have an IPv6 address yet".to_string())
        })?;
        let ssh_user = "mikrom".to_string();
        let database_user = "cloud_admin".to_string();
        let database_host = "127.0.0.1".to_string();
        let database_port = 5432;
        let ssh_port = 22;
        let ssh_destination = if ssh_host.contains(':') {
            format!("[{ssh_host}]")
        } else {
            ssh_host.clone()
        };
        let ssh_tunnel_command = format!(
            "ssh -N -L {database_port}:{database_host}:{database_port} {ssh_user}@{ssh_destination}"
        );
        let psql_command = format!(
            "psql \"host={database_host} port={database_port} user={database_user} dbname={}\"",
            database.name
        );

        Ok(DatabaseConnectionInfo {
            database_id,
            database_name: database.name,
            database_user,
            database_host,
            database_port,
            ssh_host,
            ssh_user,
            ssh_port,
            ssh_tunnel_command,
            psql_command,
        })
    }

    pub async fn validate_tenant_retention(
        state: &AppState,
        tenant_id: &str,
        generation: u32,
    ) -> bool {
        match state
            .ctx
            .database_repo
            .get_database_by_neon_tenant_id(tenant_id)
            .await
        {
            Ok(Some(database)) => {
                !matches!(database.status, DatabaseStatus::Deleting)
                    && database.tenant_gen.unwrap_or(1) == generation
            },
            Ok(None) => false,
            Err(err) => {
                tracing::warn!(
                    tenant_id = %tenant_id,
                    error = %err,
                    "[mikrom-api] Falling back to retention on database lookup error"
                );
                true
            },
        }
    }

    async fn resolve_backup_context(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
    ) -> ApiResult<(Database, DatabaseDeployment, String)> {
        let database = state
            .ctx
            .database_repo
            .get_database(database_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found".to_string()))?;

        if database.tenant_id != tenant_id {
            return Err(ApiError::Forbidden);
        }

        let deployment_id = database.active_deployment_id.ok_or_else(|| {
            ApiError::Conflict("Database has no active deployment yet".to_string())
        })?;

        let deployment = state
            .ctx
            .database_repo
            .get_deployment(deployment_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database deployment not found".to_string()))?;

        if deployment.tenant_id != tenant_id {
            return Err(ApiError::Forbidden);
        }

        let job_id = deployment.job_id.clone().ok_or_else(|| {
            ApiError::Conflict("Database deployment does not have a job ID yet".to_string())
        })?;

        Ok((database, deployment, job_id))
    }

    pub async fn list_backup_snapshots(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
    ) -> ApiResult<(bool, String, Vec<VmSnapshot>)> {
        let (database, _deployment, job_id) =
            Self::resolve_backup_context(state, database_id, tenant_id).await?;
        VmService::list_snapshots(state, database.tenant_id.to_string(), job_id).await
    }

    pub async fn create_backup_snapshot(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let (database, _deployment, job_id) =
            Self::resolve_backup_context(state, database_id, tenant_id).await?;
        VmService::create_snapshot(state, database.tenant_id.to_string(), job_id, snapshot_name)
            .await
    }

    pub async fn restore_backup_snapshot(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let (database, _deployment, job_id) =
            Self::resolve_backup_context(state, database_id, tenant_id).await?;
        VmService::restore_snapshot(state, database.tenant_id.to_string(), job_id, snapshot_name)
            .await
    }

    pub async fn delete_backup_snapshot(
        state: &AppState,
        database_id: Uuid,
        tenant_id: Uuid,
        snapshot_name: String,
    ) -> ApiResult<(bool, String)> {
        let (database, _deployment, job_id) =
            Self::resolve_backup_context(state, database_id, tenant_id).await?;
        VmService::delete_snapshot(state, database.tenant_id.to_string(), job_id, snapshot_name)
            .await
    }

    pub async fn create_database(
        state: &AppState,
        params: CreateDatabaseParams,
    ) -> ApiResult<Database> {
        // Ensure the tenant exists before creating any database rows that reference it.
        state
            .ctx
            .tenant_repo
            .find_by_id(params.tenant_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Tenant not found".to_string()))?;

        let owner_user_id = resolve_tenant_owner_user_id(state, params.tenant_id).await?;
        let mut params = params;
        Self::ensure_neon_provisioning_ids(&mut params);
        params.user_id = owner_user_id;

        let database = state
            .ctx
            .database_repo
            .create_database(params)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        #[cfg(not(test))]
        {
            let state = state.clone();
            let database_id = database.id;
            tokio::spawn(async move {
                if let Err(err) =
                    Self::provision_and_deploy_database_with_retry(state.clone(), database_id).await
                {
                    tracing::error!(
                        database_id = %database_id,
                        error = %err,
                        "Database provisioning failed after retries"
                    );
                    let _ = state
                        .ctx
                        .database_repo
                        .update_database_status(database_id, DatabaseStatus::Failed)
                        .await;
                }
            });
        }

        Ok(database)
    }

    #[allow(dead_code)]
    async fn provision_and_deploy_database_with_retry(
        state: AppState,
        database_id: Uuid,
    ) -> ApiResult<()> {
        let mut delay = std::time::Duration::from_secs(1);
        let max_attempts = 5;

        for attempt in 1..=max_attempts {
            match Self::provision_and_deploy_database(state.clone(), database_id).await {
                Ok(()) => return Ok(()),
                Err(err)
                    if attempt < max_attempts && Self::is_retryable_provisioning_error(&err) =>
                {
                    tracing::warn!(
                        database_id = %database_id,
                        attempt,
                        error = %err,
                        "Database provisioning attempt failed, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay =
                        std::cmp::min(delay.saturating_mul(2), std::time::Duration::from_secs(30));
                },
                Err(err) => return Err(err),
            }
        }

        Err(ApiError::Internal(
            "Database provisioning failed after all retry attempts".to_string(),
        ))
    }

    async fn provision_and_deploy_database(state: AppState, database_id: Uuid) -> ApiResult<()> {
        let mut database = state
            .ctx
            .database_repo
            .get_database(database_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found".to_string()))?;

        if database.engine == "neon" && Self::needs_neon_provisioning(&database) {
            let neon_client =
                crate::infrastructure::neon::NeonClient::from_config(&state.ctx.config)
                    .map_err(|e| ApiError::Internal(e.to_string()))?
                    .ok_or_else(|| {
                        ApiError::Internal(
                            "NEON_PAGESERVER_URL and NEON_SAFEKEEPER_HTTP_URL are required to provision database workloads"
                                .to_string(),
                        )
                    })?;

            let (tenant_id, timeline_id) = Self::resolve_neon_provisioning_ids(&database);
            let provisioning = neon_client
                .provision_database_with_ids(tenant_id, timeline_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;

            state
                .ctx
                .database_repo
                .update_database_provisioning(
                    database_id,
                    &provisioning.tenant_id,
                    &provisioning.timeline_id,
                    provisioning.tenant_gen,
                )
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;

            database.neon_tenant_id = Some(provisioning.tenant_id);
            database.neon_timeline_id = Some(provisioning.timeline_id);
            database.tenant_gen = Some(provisioning.tenant_gen);
        }

        Self::deploy_database(&state, database_id).await?;
        Ok(())
    }

    pub async fn deploy_database(
        state: &AppState,
        database_id: Uuid,
    ) -> ApiResult<DatabaseDeployment> {
        let database = state
            .ctx
            .database_repo
            .get_database(database_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found".to_string()))?;

        let owner_user_id = resolve_tenant_owner_user_id(state, database.tenant_id).await?;

        // 1. Create deployment record
        let deployment = state
            .ctx
            .database_repo
            .create_deployment(database.id, database.tenant_id, owner_user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // 2. Update active deployment ID in database table
        state
            .ctx
            .database_repo
            .update_active_deployment(database.id, deployment.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // 3. Get user VPC info for IPv6
        let user = state
            .user_repo
            .find_by_id(owner_user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

        let vpc_ipv6_prefix = user.vpc_ipv6_prefix.ok_or_else(|| {
            ApiError::BadRequest("User does not have a VPC IPv6 prefix configured".to_string())
        })?;

        if database.engine == "neon" && Self::needs_neon_provisioning(&database) {
            return Err(ApiError::Internal(
                "Database is missing Neon tenant/timeline identifiers".to_string(),
            ));
        }

        // 4. Send deploy request to scheduler
        let nats_req = mikrom_proto::scheduler::DeployDatabaseRequest {
            database_id: database.id.to_string(),
            database_name: database.name.clone(),
            rootfs_image: DATABASE_ROOTFS_IMAGE.to_string(),
            tenant_id: database.tenant_id.to_string(),
            deployment_id: deployment.id.to_string(),
            vpc_ipv6_prefix,
            config: Some(mikrom_proto::scheduler::AppConfig {
                vcpus: database.vcpus.value(),
                memory_mib: database.memory_mib.value(),
                disk_mib: database.disk_mib,
                port: 5432,
                env: Self::database_env(&database, &state.ctx.config)?,
                volumes: vec![],
                health_check_path: "/".to_string(),
                ipv6_address: "".to_string(),
                ipv6_gateway: "".to_string(),
                hypervisor: mikrom_proto::scheduler::HypervisorType::HypertypeCloudHypervisor
                    as i32,
                workload_type: mikrom_proto::scheduler::WorkloadType::Database as i32,
            }),
        };

        let response = state
            .scheduler
            .deploy_database(nats_req)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        if response.status == mikrom_proto::scheduler::DeployStatus::Failed as i32 {
            state
                .ctx
                .database_repo
                .update_deployment_status(deployment.id, "FAILED")
                .await
                .ok();
            state
                .ctx
                .database_repo
                .update_database_status(database.id, DatabaseStatus::Failed)
                .await
                .ok();
            return Err(ApiError::Internal(format!(
                "Scheduler failed to deploy: {}",
                response.message
            )));
        }

        // 5. Update deployment with job info
        state
            .ctx
            .database_repo
            .update_deployment_job_info(
                deployment.id,
                &response.job_id,
                &response.host_id,
                &response.vm_id,
            )
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state
            .ctx
            .database_repo
            .update_deployment_status(deployment.id, "RUNNING")
            .await
            .ok();
        state
            .ctx
            .database_repo
            .update_database_status(database.id, DatabaseStatus::Running)
            .await
            .ok();

        state
            .ctx
            .database_repo
            .get_deployment(deployment.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Deployment not found".to_string()))
    }

    pub async fn delete_database(state: &AppState, database_id: Uuid) -> ApiResult<()> {
        let database = state
            .ctx
            .database_repo
            .get_database(database_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found".to_string()))?;

        // 1. Tell scheduler to delete if there is an active deployment
        if let Some(deployment_id) = database.active_deployment_id {
            let deployment = state
                .ctx
                .database_repo
                .get_deployment(deployment_id)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;

            let job_id = deployment.and_then(|deployment| deployment.job_id);
            if let Some(job_id) = job_id {
                let deleted = state
                    .scheduler
                    .delete_database(job_id, database.tenant_id.to_string())
                    .await
                    .map_err(|e| ApiError::Scheduler(e.to_string()))?;

                if !deleted {
                    return Err(ApiError::Scheduler(
                        "Scheduler rejected database deletion".to_string(),
                    ));
                }
            }
        }

        // 2. Delete from database
        state
            .ctx
            .database_repo
            .delete_database(database.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(())
    }

    fn database_env(
        database: &Database,
        config: &crate::config::ApiConfig,
    ) -> ApiResult<std::collections::HashMap<String, String>> {
        let mut env = database.settings.clone();
        env.insert(MIKROM_DATABASE_ID_KEY.to_string(), database.id.to_string());

        if let Some(tenant_id) = &database.neon_tenant_id {
            env.insert(NEON_TENANT_ID_KEY.to_string(), tenant_id.clone());
        }
        if let Some(timeline_id) = &database.neon_timeline_id {
            env.insert(NEON_TIMELINE_ID_KEY.to_string(), timeline_id.clone());
        }

        if let Some(value) = config
            .neon_jwks_json
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            env.entry(NEON_JWKS_JSON_KEY.to_string())
                .or_insert_with(|| value.clone());
        } else if let Some(path) = config
            .neon_jwks_path
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            let jwks_json = std::fs::read_to_string(path).map_err(|e| {
                ApiError::Internal(format!("Failed to read NEON JWKS from {}: {}", path, e))
            })?;
            env.entry(NEON_JWKS_JSON_KEY.to_string())
                .or_insert(jwks_json);
        }
        if let Some(value) = config
            .neon_instance_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            env.entry(NEON_INSTANCE_ID_KEY.to_string())
                .or_insert_with(|| value.clone());
        }
        env.entry(NEON_SAFEKEEPERS_GENERATION_KEY.to_string())
            .or_insert_with(|| database.tenant_gen.unwrap_or(1).to_string());
        if let Some(value) = config
            .neon_pageserver_url
            .as_ref()
            .and_then(|value| Self::extract_neon_host(value))
        {
            env.entry(NEON_PAGESERVER_IPV6_KEY.to_string())
                .or_insert(value);
        }
        if let Some(value) = config
            .neon_safekeeper_connstrs
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            env.entry(NEON_SAFEKEEPER_CONNSTRS_KEY.to_string())
                .or_insert_with(|| value.clone());
        }
        if let Some(value) = config.mikrom_neon_dev_mode {
            env.entry(MIKROM_NEON_DEV_MODE_KEY.to_string())
                .or_insert_with(|| value.to_string());
        }
        if let Some(value) = config
            .mikrom_init_trace_files
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            env.entry(MIKROM_INIT_TRACE_FILES_KEY.to_string())
                .or_insert_with(|| value.clone());
        }
        if let Some(value) = config
            .neon_configure_token
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            env.entry(NEON_CONFIGURE_TOKEN_KEY.to_string())
                .or_insert_with(|| value.clone());
            env.entry(MIKROM_DATABASE_CONFIGURE_TOKEN_KEY.to_string())
                .or_insert_with(|| value.clone());
            return Ok(env);
        }

        if let Some(token) = Self::generate_neon_configure_token(database, config)? {
            env.entry(NEON_CONFIGURE_TOKEN_KEY.to_string())
                .or_insert(token.clone());
            env.entry(MIKROM_DATABASE_CONFIGURE_TOKEN_KEY.to_string())
                .or_insert(token);
        }

        Ok(env)
    }

    fn extract_neon_host(value: &str) -> Option<String> {
        let authority = value
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(value);
        let authority = authority.split('/').next().unwrap_or(authority);
        let authority = authority.rsplit('@').next().unwrap_or(authority);

        if let Some(start) = authority.find('[') {
            let remainder = &authority[start + 1..];
            let end = remainder.find(']')?;
            return Some(remainder[..end].to_string());
        }

        if authority.chars().filter(|&c| c == ':').count() > 1 {
            return Some(authority.to_string());
        }

        authority.split(':').next().map(|host| host.to_string())
    }

    fn generate_neon_configure_token(
        database: &Database,
        config: &crate::config::ApiConfig,
    ) -> ApiResult<Option<String>> {
        let private_key = match (
            config.neon_configure_private_key_pem.as_ref(),
            config.neon_configure_private_key_path.as_ref(),
        ) {
            (Some(pem), _) if !pem.trim().is_empty() => pem.trim().replace("\\n", "\n"),
            (None, Some(path)) if !path.trim().is_empty() => std::fs::read_to_string(path)
                .map_err(|e| {
                    ApiError::Internal(format!(
                        "Failed to read NEON configure private key from {}: {}",
                        path, e
                    ))
                })?
                .trim()
                .replace("\\n", "\n"),
            _ => return Ok(None),
        };

        let now = Utc::now();
        let claims = NeonConfigureClaims {
            iss: NEON_CONFIGURE_TOKEN_ISSUER.to_string(),
            sub: NEON_CONFIGURE_TOKEN_SUBJECT.to_string(),
            aud: vec![NEON_CONFIGURE_TOKEN_AUDIENCE.to_string()],
            iat: (now - Duration::seconds(30)).timestamp(),
            exp: (now + Duration::seconds(NEON_CONFIGURE_TOKEN_TTL_SECS)).timestamp(),
            compute_id: database.id.to_string(),
            scope: NEON_CONFIGURE_TOKEN_SCOPE.to_string(),
        };

        let key = EncodingKey::from_rsa_pem(private_key.as_bytes()).map_err(|e| {
            ApiError::Internal(format!("Invalid NEON configure private key: {}", e))
        })?;

        let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some(NEON_CONFIGURE_TOKEN_KID.to_string());

        let token = jsonwebtoken::encode(&header, &claims, &key).map_err(|e| {
            ApiError::Internal(format!("Failed to encode NEON configure JWT: {}", e))
        })?;

        Ok(Some(token))
    }

    fn ensure_neon_provisioning_ids(params: &mut CreateDatabaseParams) {
        if params.engine != "neon" {
            return;
        }

        if params.neon_tenant_id.is_none() {
            params.neon_tenant_id = Some(Self::placeholder_neon_id("tenant"));
        }
        if params.neon_timeline_id.is_none() {
            params.neon_timeline_id = Some(Self::placeholder_neon_id("timeline"));
        }
    }

    fn needs_neon_provisioning(database: &Database) -> bool {
        (match database.neon_tenant_id.as_deref() {
            None => true,
            Some(value) => Self::is_placeholder_neon_id(value),
        }) || match database.neon_timeline_id.as_deref() {
            None => true,
            Some(value) => Self::is_placeholder_neon_id(value),
        }
    }

    fn placeholder_neon_id(kind: &str) -> String {
        format!("pending-{kind}-{}", uuid::Uuid::new_v4().simple())
    }

    fn resolve_neon_provisioning_ids(database: &Database) -> (String, String) {
        let tenant_id = database
            .neon_tenant_id
            .as_deref()
            .and_then(|id| id.strip_prefix("pending-tenant-"))
            .map(|id| id.to_string())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let timeline_id = database
            .neon_timeline_id
            .as_deref()
            .and_then(|id| id.strip_prefix("pending-timeline-"))
            .map(|id| id.to_string())
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());

        (tenant_id, timeline_id)
    }

    fn is_placeholder_neon_id(value: &str) -> bool {
        value.starts_with("pending-tenant-") || value.starts_with("pending-timeline-")
    }

    fn is_retryable_provisioning_error(err: &ApiError) -> bool {
        let message = err.to_string();

        if message.contains("404 Not Found") || message.contains("page not found") {
            return false;
        }

        if message.contains("NEON_PAGESERVER_URL is required") {
            return false;
        }

        true
    }
}

#[cfg(any())]
mod tests {
    use super::*;
    use crate::application::ApiContext;
    use crate::domain::{
        MockDatabaseRepository, MockScheduler, MockUserRepository, User, UserRole,
    };
    use crate::infrastructure::nats::TypedNatsClient;
    use crate::{AppState, domain::DatabaseRepository};
    use mockall::Sequence;
    use mockall::predicate::{eq, function};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn build_state(
        config: crate::config::ApiConfig,
        user_repo: Arc<dyn crate::domain::UserRepository>,
        database_repo: Arc<dyn DatabaseRepository>,
        scheduler: Arc<dyn crate::domain::Scheduler>,
    ) -> AppState {
        let ctx = ApiContext {
            user_repo: user_repo.clone(),
            app_repo: Arc::new(crate::domain::MockAppRepository::new()),
            database_repo: database_repo.clone(),
            github_repo: Arc::new(crate::domain::MockGithubRepository::new()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler: scheduler.clone(),
            nats: TypedNatsClient::default(),
            db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            config: Arc::new(config.clone()),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
        };

        let (deployment_events, _) = tokio::sync::broadcast::channel(4);
        let (workspace_events, _) = tokio::sync::broadcast::channel(4);
        let (mesh_status, _) =
            tokio::sync::watch::channel(crate::application::vms::MeshStatus::default());

        AppState {
            ctx,
            user_repo,
            app_repo: Arc::new(crate::domain::MockAppRepository::new()),
            database_repo,
            github_repo: Arc::new(crate::domain::MockGithubRepository::new()),
            volume_repo: Arc::new(crate::domain::MockVolumeRepository::new()),
            scheduler,
            nats: TypedNatsClient::default(),
            router_addr: "http://localhost:8080".to_string(),
            frontend_url: "http://localhost:3000".to_string(),
            api_db: sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap(),
            jwt_secret: "secret".to_string(),
            master_key: "key".to_string(),
            deployment_events,
            workspace_events,
            mesh_status,
            acme_email: "test@example.com".to_string(),
            acme_staging: true,
            acme_check_interval: 3600,
            github_app_id: None,
            github_private_key: None,
            github_app_slug: None,
            github_webhook_url_base: None,
            active_deployment_flows: Arc::new(dashmap::DashSet::new()),
        }
    }

    fn database(
        id: Uuid,
        user_id: Uuid,
        status: DatabaseStatus,
        active_deployment_id: Option<Uuid>,
    ) -> Database {
        Database {
            id,
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(2).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(1024).unwrap(),
            disk_mib: 4096,
            tenant_id: Some("11111111111111111111111111111111".to_string()),
            timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::from([("max_connections".to_string(), "200".to_string())]),
            status,
            active_deployment_id,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn deployment(db_id: Uuid, user_id: Uuid) -> DatabaseDeployment {
        DatabaseDeployment {
            id: Uuid::new_v4(),
            database_id: db_id,
            user_id,
            job_id: None,
            status: "PENDING".to_string(),
            host_id: None,
            vm_id: None,
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn provision_and_deploy_database_creates_running_deployment() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();

        let initial_db = Database {
            tenant_id: Some("11111111111111111111111111111111".to_string()),
            timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            ..database(database_id, user_id, DatabaseStatus::Pending, None)
        };
        let running_db = Database {
            status: DatabaseStatus::Running,
            active_deployment_id: Some(deployment_id),
            ..initial_db.clone()
        };
        let deployment = DatabaseDeployment {
            id: deployment_id,
            database_id,
            user_id,
            job_id: None,
            status: "PENDING".to_string(),
            host_id: None,
            vm_id: None,
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let deployment_after = DatabaseDeployment {
            id: deployment_id,
            database_id,
            user_id,
            job_id: Some("job-123".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: None,
            created_at: deployment.created_at,
            updated_at: chrono::Utc::now(),
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning({
                let initial_db = initial_db.clone();
                move |_| {
                    let value = initial_db.clone();
                    Box::pin(async move { Ok(Some(value)) })
                }
            });
        db_repo
            .expect_create_deployment()
            .with(eq(database_id), eq(user_id), eq(user_id))
            .times(1)
            .returning({
                let deployment = deployment.clone();
                move |_, _, _| {
                    let value = deployment.clone();
                    Box::pin(async move { Ok(value) })
                }
            });
        db_repo
            .expect_update_active_deployment()
            .with(eq(database_id), eq(deployment_id))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_update_deployment_job_info()
            .with(eq(deployment_id), eq("job-123"), eq("host-1"), eq("vm-1"))
            .times(1)
            .returning(|_, _, _, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_update_deployment_status()
            .with(eq(deployment_id), eq("RUNNING"))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_update_database_status()
            .with(eq(database_id), eq(DatabaseStatus::Running))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_get_deployment()
            .with(eq(deployment_id))
            .times(1)
            .returning({
                let deployment_after = deployment_after.clone();
                move |_| {
                    let value = deployment_after.clone();
                    Box::pin(async move { Ok(Some(value)) })
                }
            });
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning({
                let running_db = running_db.clone();
                move |_| {
                    let value = running_db.clone();
                    Box::pin(async move { Ok(Some(value)) })
                }
            });

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(User {
                    id: user_id,
                    email: "db@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    avatar_url: None,
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
                    totp_secret: None,
                    totp_enabled: false,
                    deleted_at: None,
                }))
            });

        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_deploy_database()
            .with(function(
                move |req: &mikrom_proto::scheduler::DeployDatabaseRequest| {
                    req.database_id == database_id.to_string()
                        && req.user_id == user_id.to_string()
                        && req.vpc_ipv6_prefix == "fd00:abcd::"
                        && req
                            .config
                            .as_ref()
                            .map(|cfg| {
                                cfg.workload_type
                                    == mikrom_proto::scheduler::WorkloadType::Database as i32
                                    && cfg.port == 5432
                                    && cfg
                                        .env
                                        .get(MIKROM_DATABASE_ID_KEY)
                                        .is_some_and(|v| v == &database_id.to_string())
                                    && cfg
                                        .env
                                        .get(NEON_TENANT_ID_KEY)
                                        .is_some_and(|v| v == "11111111111111111111111111111111")
                                    && cfg
                                        .env
                                        .get(NEON_TIMELINE_ID_KEY)
                                        .is_some_and(|v| v == "22222222222222222222222222222222")
                            })
                            .unwrap_or(false)
                },
            ))
            .times(1)
            .returning(|_| {
                Ok(mikrom_proto::scheduler::DeployDatabaseResponse {
                    job_id: "job-123".to_string(),
                    status: mikrom_proto::scheduler::DeployStatus::Running as i32,
                    host_id: "host-1".to_string(),
                    vm_id: "vm-1".to_string(),
                    message: "ok".to_string(),
                    hypervisor: mikrom_proto::scheduler::HypervisorType::HypertypeCloudHypervisor
                        as i32,
                })
            });
        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        DatabaseService::provision_and_deploy_database(state, database_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_database_returns_pending_database() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_create_database()
            .with(function(|params: &CreateDatabaseParams| {
                params.name == "orders"
                    && params.engine == "neon"
                    && params.postgres_version == 16
                    && params.user_id == user_id
                    && params.disk_mib == 1024
                    && params.vcpus.value() == 1
                    && params.memory_mib.value() == 512
                    && params
                        .tenant_id
                        .as_deref()
                        .is_some_and(DatabaseService::is_placeholder_neon_id)
                    && params
                        .timeline_id
                        .as_deref()
                        .is_some_and(DatabaseService::is_placeholder_neon_id)
                    && !params.settings.contains_key(NEON_TENANT_ID_KEY)
                    && !params.settings.contains_key(NEON_TIMELINE_ID_KEY)
            }))
            .times(1)
            .returning(move |_| {
                Box::pin(async move {
                    Ok(Database {
                        id: database_id,
                        name: "orders".to_string(),
                        engine: "neon".to_string(),
                        postgres_version: 16,
                        user_id,
                        vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
                        memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
                        disk_mib: 1024,
                        tenant_id: None,
                        timeline_id: None,
                        tenant_gen: None,
                        settings: HashMap::new(),
                        status: DatabaseStatus::Pending,
                        active_deployment_id: None,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    })
                })
            });

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(User {
                    id: user_id,
                    email: "db@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    avatar_url: None,
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
                    totp_secret: None,
                    totp_enabled: false,
                    deleted_at: None,
                }))
            });

        let scheduler = MockScheduler::new();
        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );
        let params = CreateDatabaseParams {
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
            tenant_gen: None,
            settings: HashMap::new(),
        };

        let created = DatabaseService::create_database(&state, params)
            .await
            .unwrap();
        assert_eq!(created.id, database_id);
        assert_eq!(created.postgres_version, 16);
        assert_eq!(created.active_deployment_id, None);
        assert_eq!(created.status, DatabaseStatus::Pending);
    }

    #[test]
    fn neon_provisioning_helpers_distinguish_real_and_placeholder_ids() {
        let mut neon_params = CreateDatabaseParams {
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
            tenant_gen: None,
            settings: HashMap::new(),
        };
        DatabaseService::ensure_neon_provisioning_ids(&mut neon_params);
        assert!(
            neon_params
                .tenant_id
                .as_deref()
                .is_some_and(DatabaseService::is_placeholder_neon_id)
        );
        assert!(
            neon_params
                .timeline_id
                .as_deref()
                .is_some_and(DatabaseService::is_placeholder_neon_id)
        );

        let neon_db = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: Some("11111111111111111111111111111111".to_string()),
            timeline_id: Some("22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::new(),
            status: DatabaseStatus::Running,
            active_deployment_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert!(!DatabaseService::needs_neon_provisioning(&neon_db));

        let placeholder_db = Database {
            tenant_id: Some("pending-tenant-123".to_string()),
            timeline_id: Some("pending-timeline-456".to_string()),
            ..neon_db.clone()
        };
        assert!(DatabaseService::needs_neon_provisioning(&placeholder_db));

        let mut postgres_params = CreateDatabaseParams {
            name: "orders".to_string(),
            engine: "postgres".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
            tenant_gen: None,
            settings: HashMap::new(),
        };
        DatabaseService::ensure_neon_provisioning_ids(&mut postgres_params);
        assert!(postgres_params.tenant_id.is_none());
        assert!(postgres_params.timeline_id.is_none());
    }

    #[test]
    fn resolve_neon_provisioning_ids_reuses_placeholder_suffixes() {
        let database = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: Some("pending-tenant-11111111111111111111111111111111".to_string()),
            timeline_id: Some("pending-timeline-22222222222222222222222222222222".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::new(),
            status: DatabaseStatus::Pending,
            active_deployment_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let (tenant_id, timeline_id) = DatabaseService::resolve_neon_provisioning_ids(&database);
        assert_eq!(tenant_id, "11111111111111111111111111111111");
        assert_eq!(timeline_id, "22222222222222222222222222222222");
    }

    #[test]
    fn database_env_includes_neon_runtime_settings_from_api_config() {
        let database = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: Some("tenant-123".to_string()),
            timeline_id: Some("timeline-456".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::from([("max_connections".to_string(), "200".to_string())]),
            status: DatabaseStatus::Pending,
            active_deployment_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let config = crate::config::ApiConfig {
            neon_jwks_json: Some("{\"keys\":[]}".to_string()),
            neon_jwks_path: Some("/etc/mikrom/jwks.json".to_string()),
            neon_instance_id: Some("compute-node-1".to_string()),
            neon_pageserver_url: Some("http://[fd40:b90d:fc5f:1ae0::1]:9898".to_string()),
            neon_safekeeper_connstrs: Some("[fd40:b90d:fc5f:1ae0::1]:5454".to_string()),
            neon_safekeeper_token: Some("token-123".to_string()),
            mikrom_neon_dev_mode: Some(false),
            neon_configure_token: Some("token-123".to_string()),
            ..Default::default()
        };

        let env = DatabaseService::database_env(&database, &config).unwrap();
        assert_eq!(
            env.get(MIKROM_DATABASE_ID_KEY),
            Some(&database.id.to_string())
        );
        assert_eq!(env.get(NEON_TENANT_ID_KEY), Some(&"tenant-123".to_string()));
        assert_eq!(
            env.get(NEON_TIMELINE_ID_KEY),
            Some(&"timeline-456".to_string())
        );
        assert_eq!(
            env.get(NEON_JWKS_JSON_KEY),
            Some(&"{\"keys\":[]}".to_string())
        );
        assert!(!env.contains_key(NEON_JWKS_PATH_KEY));
        assert_eq!(
            env.get(NEON_INSTANCE_ID_KEY),
            Some(&"compute-node-1".to_string())
        );
        assert_eq!(
            env.get(NEON_SAFEKEEPERS_GENERATION_KEY),
            Some(&"1".to_string())
        );
        assert_eq!(
            env.get(NEON_SAFEKEEPER_CONNSTRS_KEY),
            Some(&"[fd40:b90d:fc5f:1ae0::1]:5454".to_string())
        );
        assert_eq!(
            env.get(NEON_PAGESERVER_IPV6_KEY),
            Some(&"fd40:b90d:fc5f:1ae0::1".to_string())
        );
        assert_eq!(
            env.get(MIKROM_NEON_DEV_MODE_KEY),
            Some(&"false".to_string())
        );
        assert_eq!(
            env.get(NEON_CONFIGURE_TOKEN_KEY),
            Some(&"token-123".to_string())
        );
        assert_eq!(
            env.get(MIKROM_DATABASE_CONFIGURE_TOKEN_KEY),
            Some(&"token-123".to_string())
        );
        assert_eq!(env.get("max_connections"), Some(&"200".to_string()));
    }

    #[test]
    fn database_env_reads_jwks_path_and_injects_inline_json() {
        let temp_dir = tempfile::tempdir().unwrap();
        let jwks_path = temp_dir.path().join("jwks.json");
        std::fs::write(&jwks_path, r#"{"keys":[{"kid":"mikrom-neon-key"}]}"#).unwrap();

        let database = Database {
            id: Uuid::new_v4(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id: Uuid::new_v4(),
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: Some("tenant-123".to_string()),
            timeline_id: Some("timeline-456".to_string()),
            tenant_gen: Some(1),
            settings: HashMap::new(),
            status: DatabaseStatus::Pending,
            active_deployment_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let config = crate::config::ApiConfig {
            neon_jwks_path: Some(jwks_path.to_string_lossy().to_string()),
            ..Default::default()
        };

        let env = DatabaseService::database_env(&database, &config).unwrap();
        assert_eq!(
            env.get(NEON_JWKS_JSON_KEY),
            Some(&r#"{"keys":[{"kid":"mikrom-neon-key"}]}"#.to_string())
        );
        assert!(!env.contains_key(NEON_JWKS_PATH_KEY));
    }

    #[test]
    fn neon_configure_claims_use_expected_scope_literal() {
        let claims = NeonConfigureClaims {
            iss: NEON_CONFIGURE_TOKEN_ISSUER.to_string(),
            sub: NEON_CONFIGURE_TOKEN_SUBJECT.to_string(),
            aud: vec![NEON_CONFIGURE_TOKEN_AUDIENCE.to_string()],
            iat: 1,
            exp: 2,
            compute_id: "compute-1".to_string(),
            scope: NEON_CONFIGURE_TOKEN_SCOPE.to_string(),
        };

        let value = serde_json::to_value(&claims).unwrap();
        assert_eq!(value["scope"], NEON_CONFIGURE_TOKEN_SCOPE);
        assert_eq!(value["aud"], serde_json::json!(["compute"]));
    }

    #[test]
    fn extract_neon_host_preserves_unbracketed_ipv6_authority() {
        assert_eq!(
            DatabaseService::extract_neon_host("http://fd40:b90d:fc5f:1ae0::1:9898"),
            Some("fd40:b90d:fc5f:1ae0::1:9898".to_string())
        );
    }

    #[tokio::test]
    async fn validate_tenant_retention_requires_matching_generation() {
        let tenant_id = "11111111111111111111111111111111";
        let user_id = Uuid::new_v4();
        let db_id = Uuid::new_v4();
        let db = Database {
            tenant_id: Some(tenant_id.to_string()),
            tenant_gen: Some(3),
            ..database(db_id, user_id, DatabaseStatus::Running, None)
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database_by_tenant_id()
            .with(eq(tenant_id))
            .times(2)
            .returning(move |_| {
                let value = db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(MockScheduler::new()),
        );

        assert!(!DatabaseService::validate_tenant_retention(&state, tenant_id, 1).await);
        assert!(DatabaseService::validate_tenant_retention(&state, tenant_id, 3).await);
    }

    #[tokio::test]
    async fn create_database_returns_not_found_when_user_is_missing() {
        let user_id = Uuid::new_v4();

        let mut db_repo = MockDatabaseRepository::new();
        db_repo.expect_create_database().times(0);

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(|_| Ok(None));

        let scheduler = MockScheduler::new();
        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );
        let params = CreateDatabaseParams {
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
            tenant_gen: None,
            settings: HashMap::new(),
        };

        let err = DatabaseService::create_database(&state, params)
            .await
            .unwrap_err();
        match err {
            ApiError::NotFound(message) => assert_eq!(message, "User not found"),
            other => panic!("expected not found, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deploy_database_requires_vpc_prefix() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let db = database(database_id, user_id, DatabaseStatus::Pending, None);
        let deployment = deployment(database_id, user_id);

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning(move |_| {
                let value = db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_create_deployment()
            .with(eq(database_id), eq(user_id), eq(user_id))
            .times(1)
            .returning(move |_, _, _| {
                let value = deployment.clone();
                Box::pin(async move {
                    Ok(DatabaseDeployment {
                        id: deployment_id,
                        ..value
                    })
                })
            });
        db_repo
            .expect_update_active_deployment()
            .with(eq(database_id), eq(deployment_id))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(User {
                    id: user_id,
                    email: "db@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    avatar_url: None,
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: None,
                    totp_secret: None,
                    totp_enabled: false,
                    deleted_at: None,
                }))
            });

        let scheduler = MockScheduler::new();
        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        let err = DatabaseService::deploy_database(&state, database_id)
            .await
            .unwrap_err();
        match err {
            ApiError::BadRequest(message) => {
                assert!(message.contains("VPC IPv6 prefix"));
            },
            other => panic!("expected bad request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn deploy_database_marks_failed_on_scheduler_error() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let db = database(database_id, user_id, DatabaseStatus::Pending, None);
        let deployment = deployment(database_id, user_id);

        let mut seq = Sequence::new();
        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .in_sequence(&mut seq)
            .returning(move |_| {
                let value = db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_create_deployment()
            .with(eq(database_id), eq(user_id), eq(user_id))
            .times(1)
            .returning(move |_, _, _| {
                let value = deployment.clone();
                Box::pin(async move {
                    Ok(DatabaseDeployment {
                        id: deployment_id,
                        ..value
                    })
                })
            });
        db_repo
            .expect_update_active_deployment()
            .with(eq(database_id), eq(deployment_id))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_update_deployment_status()
            .with(eq(deployment_id), eq("FAILED"))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        db_repo
            .expect_update_database_status()
            .with(eq(database_id), eq(DatabaseStatus::Failed))
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(1)
            .returning(move |_| {
                Ok(Some(User {
                    id: user_id,
                    email: "db@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    avatar_url: None,
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
                    totp_secret: None,
                    totp_enabled: false,
                    deleted_at: None,
                }))
            });

        let mut scheduler = MockScheduler::new();
        scheduler.expect_deploy_database().times(1).returning(|_| {
            Ok(mikrom_proto::scheduler::DeployDatabaseResponse {
                status: mikrom_proto::scheduler::DeployStatus::Failed as i32,
                message: "scheduler rejected the request".to_string(),
                ..Default::default()
            })
        });

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );
        let err = DatabaseService::deploy_database(&state, database_id)
            .await
            .unwrap_err();
        match err {
            ApiError::Internal(message) => assert!(message.contains("Scheduler failed to deploy")),
            other => panic!("expected internal error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_database_deletes_active_deployment_before_row() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let active_db = database(
            database_id,
            user_id,
            DatabaseStatus::Running,
            Some(deployment_id),
        );
        let deployment = DatabaseDeployment {
            id: deployment_id,
            database_id,
            user_id,
            job_id: Some("job-123".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning(move |_| {
                let value = active_db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_get_deployment()
            .with(eq(deployment_id))
            .times(1)
            .returning(move |_| {
                let value = deployment.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_delete_database()
            .with(eq(database_id))
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_delete_database()
            .with(eq("job-123".to_string()), eq(user_id.to_string()))
            .times(1)
            .returning(|_, _| Ok(true));

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        DatabaseService::delete_database(&state, database_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn delete_database_keeps_row_when_scheduler_rejects_cleanup() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let active_db = database(
            database_id,
            user_id,
            DatabaseStatus::Running,
            Some(deployment_id),
        );
        let deployment = DatabaseDeployment {
            id: deployment_id,
            database_id,
            user_id,
            job_id: Some("job-123".to_string()),
            status: "RUNNING".to_string(),
            host_id: Some("host-1".to_string()),
            vm_id: Some("vm-1".to_string()),
            ipv6_address: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .returning(move |_| {
                let value = active_db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_get_deployment()
            .with(eq(deployment_id))
            .times(1)
            .returning(move |_| {
                let value = deployment.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo.expect_delete_database().times(0);

        let mut scheduler = MockScheduler::new();
        scheduler
            .expect_delete_database()
            .with(eq("job-123".to_string()), eq(user_id.to_string()))
            .times(1)
            .returning(|_, _| Ok(false));

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );

        let err = DatabaseService::delete_database(&state, database_id)
            .await
            .unwrap_err();

        match err {
            ApiError::Scheduler(message) => {
                assert_eq!(message, "Scheduler rejected database deletion")
            },
            other => panic!("expected scheduler error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn validate_tenant_retention_returns_true_for_active_database() {
        let tenant_id = "11111111111111111111111111111111";
        let user_id = Uuid::new_v4();
        let db_id = Uuid::new_v4();
        let db = Database {
            tenant_id: Some(tenant_id.to_string()),
            tenant_gen: Some(1),
            ..database(db_id, user_id, DatabaseStatus::Running, None)
        };

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database_by_tenant_id()
            .with(eq(tenant_id))
            .times(1)
            .returning(move |_| {
                let value = db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(MockScheduler::new()),
        );

        assert!(DatabaseService::validate_tenant_retention(&state, tenant_id, 1).await);
    }

    #[tokio::test]
    async fn validate_tenant_retention_returns_false_when_missing() {
        let tenant_id = "11111111111111111111111111111111";

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database_by_tenant_id()
            .with(eq(tenant_id))
            .times(1)
            .returning(|_| Box::pin(async { Ok(None) }));

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(MockScheduler::new()),
        );

        assert!(!DatabaseService::validate_tenant_retention(&state, tenant_id, 1).await);
    }

    #[tokio::test]
    async fn validate_tenant_retention_is_conservative_on_repo_error() {
        let tenant_id = "11111111111111111111111111111111";

        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_get_database_by_tenant_id()
            .with(eq(tenant_id))
            .times(1)
            .returning(|_| {
                Box::pin(async {
                    Err(crate::domain::DomainError::Infrastructure(
                        "lookup failed".to_string(),
                    ))
                })
            });

        let state = build_state(
            crate::config::ApiConfig::default(),
            Arc::new(MockUserRepository::new()),
            Arc::new(db_repo),
            Arc::new(MockScheduler::new()),
        );

        assert!(DatabaseService::validate_tenant_retention(&state, tenant_id, 1).await);
    }

    #[test]
    fn provisioning_404_errors_are_not_retryable() {
        let err = ApiError::Internal(
            "Infrastructure error: Neon create tenant failed: 404 Not Found - {\"msg\":\"page not found\"}"
                .to_string(),
        );

        assert!(!DatabaseService::is_retryable_provisioning_error(&err));
    }
}
