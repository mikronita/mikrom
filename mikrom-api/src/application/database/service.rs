use crate::AppState;
use crate::domain::{CreateDatabaseParams, Database, DatabaseDeployment, DatabaseStatus};
use crate::error::{ApiError, ApiResult};
use uuid::Uuid;

pub struct DatabaseService;

const DATABASE_ROOTFS_IMAGE: &str = "local:/opt/neon";
const NEON_TENANT_ID_KEY: &str = "NEON_TENANT_ID";
const NEON_TIMELINE_ID_KEY: &str = "NEON_TIMELINE_ID";

impl DatabaseService {
    pub async fn create_database(
        state: &AppState,
        params: CreateDatabaseParams,
    ) -> ApiResult<Database> {
        // Ensure the authenticated user still exists before creating
        // any database rows that reference it. This avoids surfacing a
        // foreign-key violation as a generic 500 when the token points to
        // a deleted or missing user.
        state
            .user_repo
            .find_by_id(params.user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

        let mut params = params;
        Self::provision_neon_database(state, &mut params).await?;

        // 1. Create database record
        let database = state
            .ctx
            .database_repo
            .create_database(params)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // 2. Initial deployment
        Self::deploy_database(state, database.id).await?;

        // 3. Reload database info
        state
            .ctx
            .database_repo
            .get_database(database.id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("Database not found after creation".to_string()))
    }

    async fn provision_neon_database(
        state: &AppState,
        params: &mut CreateDatabaseParams,
    ) -> ApiResult<()> {
        if params.engine != "neon" {
            return Ok(());
        }

        let neon_client = crate::infrastructure::neon::NeonClient::from_config(&state.ctx.config)
            .ok_or_else(|| {
            ApiError::Internal(
                "NEON_PAGESERVER_URL is required to provision database workloads".to_string(),
            )
        })?;

        let provisioning = neon_client
            .provision_database()
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        params.tenant_id = Some(provisioning.tenant_id);
        params.timeline_id = Some(provisioning.timeline_id);

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

        // 1. Create deployment record
        let deployment = state
            .ctx
            .database_repo
            .create_deployment(database.id, database.user_id)
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
            .find_by_id(database.user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound("User not found".to_string()))?;

        let vpc_ipv6_prefix = user.vpc_ipv6_prefix.ok_or_else(|| {
            ApiError::BadRequest("User does not have a VPC IPv6 prefix configured".to_string())
        })?;

        if database.engine == "neon"
            && (database.tenant_id.is_none() || database.timeline_id.is_none())
        {
            return Err(ApiError::Internal(
                "Database is missing Neon tenant/timeline identifiers".to_string(),
            ));
        }

        // 4. Send deploy request to scheduler
        let nats_req = mikrom_proto::scheduler::DeployDatabaseRequest {
            database_id: database.id.to_string(),
            database_name: database.name.clone(),
            rootfs_image: DATABASE_ROOTFS_IMAGE.to_string(),
            user_id: database.user_id.to_string(),
            deployment_id: deployment.id.to_string(),
            vpc_ipv6_prefix,
            config: Some(mikrom_proto::scheduler::AppConfig {
                vcpus: database.vcpus.value(),
                memory_mib: database.memory_mib.value(),
                disk_mib: database.disk_mib,
                port: 5432,
                env: Self::database_env(&database),
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
                state
                    .scheduler
                    .delete_database(job_id, database.user_id.to_string())
                    .await
                    .ok();
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

    fn database_env(database: &Database) -> std::collections::HashMap<String, String> {
        let mut env = database.settings.clone();

        if let Some(tenant_id) = &database.tenant_id {
            env.insert(NEON_TENANT_ID_KEY.to_string(), tenant_id.clone());
        }
        if let Some(timeline_id) = &database.timeline_id {
            env.insert(NEON_TIMELINE_ID_KEY.to_string(), timeline_id.clone());
        }

        env
    }
}

#[cfg(test)]
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
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(2).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(1024).unwrap(),
            disk_mib: 4096,
            tenant_id: Some("11111111111111111111111111111111".to_string()),
            timeline_id: Some("22222222222222222222222222222222".to_string()),
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
    async fn create_database_happy_path_deploys_and_reloads() {
        let user_id = Uuid::new_v4();
        let database_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let server = MockServer::start().await;
        let tenant_id = "11111111111111111111111111111111";
        let timeline_id = "22222222222222222222222222222222";

        Mock::given(method("POST"))
            .and(path("/v1/tenant"))
            .respond_with(ResponseTemplate::new(201).set_body_string(format!("\"{tenant_id}\"")))
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(r"^/v1/tenant/[0-9a-f]{32}/location_config$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "shards": [],
                "stripe_size": null
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path_regex(r"^/v1/tenant/[0-9a-f]{32}/timeline$"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "timeline_id": timeline_id,
                "tenant_id": tenant_id,
                "last_record_lsn": "0/0",
                "disk_consistent_lsn": "0/0",
                "state": "active",
                "min_readable_lsn": "0/0"
            })))
            .mount(&server)
            .await;

        let initial_db = database(database_id, user_id, DatabaseStatus::Pending, None);
        let deployed_db = database(
            database_id,
            user_id,
            DatabaseStatus::Running,
            Some(deployment_id),
        );
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
        let initial_db_create = initial_db.clone();
        let initial_db_reload = initial_db.clone();
        let deployment_create = deployment.clone();
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

        let mut seq = Sequence::new();
        let mut db_repo = MockDatabaseRepository::new();
        db_repo
            .expect_create_database()
            .with(function(|params: &CreateDatabaseParams| {
                params.name == "orders"
                    && params.engine == "neon"
                    && params.disk_mib == 1024
                    && params.vcpus.value() == 1
                    && params.memory_mib.value() == 512
                    && params.tenant_id.is_some()
                    && params.timeline_id.is_some()
                    && !params.settings.contains_key(NEON_TENANT_ID_KEY)
                    && !params.settings.contains_key(NEON_TIMELINE_ID_KEY)
            }))
            .times(1)
            .returning(move |_| {
                let value = initial_db_create.clone();
                Box::pin(async move { Ok(value) })
            });
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .in_sequence(&mut seq)
            .returning(move |_| {
                let value = initial_db_reload.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_create_deployment()
            .with(eq(database_id), eq(user_id))
            .times(1)
            .returning(move |_, _| {
                let value = deployment_create.clone();
                Box::pin(async move { Ok(value) })
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
            .in_sequence(&mut seq)
            .returning(move |_| {
                let value = deployment_after.clone();
                Box::pin(async move { Ok(Some(value)) })
            });
        db_repo
            .expect_get_database()
            .with(eq(database_id))
            .times(1)
            .in_sequence(&mut seq)
            .returning(move |_| {
                let value = deployed_db.clone();
                Box::pin(async move { Ok(Some(value)) })
            });

        let mut user_repo = MockUserRepository::new();
        user_repo
            .expect_find_by_id()
            .with(eq(user_id))
            .times(2)
            .returning(move |_| {
                Ok(Some(User {
                    id: user_id,
                    email: "db@example.com".to_string(),
                    password_hash: "hash".to_string(),
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
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
            crate::config::ApiConfig {
                neon_pageserver_url: Some(server.uri()),
                neon_bearer_token: None,
                ..Default::default()
            },
            Arc::new(user_repo),
            Arc::new(db_repo),
            Arc::new(scheduler),
        );
        let params = CreateDatabaseParams {
            name: "orders".to_string(),
            engine: "neon".to_string(),
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
            settings: HashMap::new(),
        };

        let created = DatabaseService::create_database(&state, params)
            .await
            .unwrap();
        assert_eq!(created.id, database_id);
        assert_eq!(created.active_deployment_id, Some(deployment_id));
        assert_eq!(created.status, DatabaseStatus::Running);
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
            user_id,
            vcpus: crate::domain::types::CpuCores::try_from(1).unwrap(),
            memory_mib: crate::domain::types::MemoryMb::try_from(512).unwrap(),
            disk_mib: 1024,
            tenant_id: None,
            timeline_id: None,
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
            .with(eq(database_id), eq(user_id))
            .times(1)
            .returning(move |_, _| {
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
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: None,
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
            .with(eq(database_id), eq(user_id))
            .times(1)
            .returning(move |_, _| {
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
                    role: UserRole::User,
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
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
}
