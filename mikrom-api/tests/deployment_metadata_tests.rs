use mikrom_api::domain::user::{NewUser, UserRepository, UserRole};
use mikrom_api::domain::{
    AppRepository, CreateAppParams, NewDeployment, TenantRepository, UpdateDeploymentParams,
};
use mikrom_api::infrastructure::db::{
    PostgresAppRepository, PostgresTenantRepository, PostgresUserRepository,
};
use mikrom_api::test_utils::TestDb;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a PostgreSQL test database with the migrated apps schema"]
async fn deployment_metadata_roundtrip_persists_git_fields() {
    let Ok(_db) = TestDb::try_new().await else {
        eprintln!("Skipping deployment metadata test: database unavailable");
        return;
    };
    let pool = _db.pool().clone();

    let user_repo = PostgresUserRepository::new(pool.clone());
    let tenant_repo = PostgresTenantRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".to_string());

    let user_id = user_repo
        .create(NewUser {
            email: format!("metadata_test_{}@example.com", Uuid::new_v4()),
            password_hash: "pass".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
        })
        .await
        .expect("failed to create user");

    let tenant = tenant_repo
        .create(
            "Metadata Project".into(),
            mikrom_api::domain::Tenant::generate_slug(),
        )
        .await
        .expect("failed to create tenant");
    tenant_repo
        .add_member(tenant.id, user_id, "admin")
        .await
        .expect("failed to add tenant member");

    let app = app_repo
        .create_app(CreateAppParams {
            name: "metadata-app".to_string(),
            git_url: "https://github.com/test/repo".to_string(),
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            user_id,
            tenant_id: tenant.id,
            ..Default::default()
        })
        .await
        .expect("failed to create app");

    let deployment = app_repo
        .create_deployment(NewDeployment {
            app_id: app.id,
            user_id,
            tenant_id: tenant.id.to_string(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            env_vars: std::collections::HashMap::new(),
            trigger_source: "github_webhook".to_string(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            hypervisor: 0,
        })
        .await
        .expect("failed to create deployment");

    app_repo
        .update_deployment(
            deployment.id,
            UpdateDeploymentParams {
                status: Some("SUCCESS".to_string()),
                job_id: Some("job-abc".to_string()),
                image_tag: Some("img:v2".to_string()),
                build_id: Some("build-xyz".to_string()),
                ipv6_address: Some("fd00::1".to_string()),
                git_commit_hash: Some("a1b2c3".to_string()),
                git_commit_message: Some("feat: metadata".to_string()),
                git_branch: Some("feature/metadata".to_string()),
                hypervisor: Some(1),
            },
        )
        .await
        .expect("failed to update deployment");

    app_repo
        .update_deployment(
            deployment.id,
            UpdateDeploymentParams {
                status: Some("RUNNING".to_string()),
                job_id: Some("job-def".to_string()),
                image_tag: Some("img:v3".to_string()),
                build_id: Some("build-uvw".to_string()),
                ipv6_address: Some("fd00::2".to_string()),
                git_commit_hash: None,
                git_commit_message: None,
                git_branch: None,
                hypervisor: Some(1),
            },
        )
        .await
        .expect("failed to apply partial deployment update");

    let updated = app_repo
        .get_deployment(deployment.id)
        .await
        .expect("failed to get deployment")
        .expect("deployment not found");

    assert_eq!(updated.status, "RUNNING");
    assert_eq!(updated.trigger_source, "github_webhook");
    assert_eq!(updated.git_commit_hash.as_deref(), Some("a1b2c3"));
    assert_eq!(
        updated.git_commit_message.as_deref(),
        Some("feat: metadata")
    );
    assert_eq!(updated.git_branch.as_deref(), Some("feature/metadata"));
    assert_eq!(updated.job_id.as_deref(), Some("job-def"));
    assert_eq!(updated.image_tag.as_deref(), Some("img:v3"));
    assert_eq!(updated.build_id.as_deref(), Some("build-uvw"));
    assert_eq!(updated.ipv6_address.as_deref(), Some("fd00::2"));
}

#[tokio::test]
#[ignore = "requires a PostgreSQL test database with the migrated apps schema"]
async fn deployment_hypervisor_roundtrip_persists_integer_value() {
    let Ok(_db) = TestDb::try_new().await else {
        eprintln!("Skipping deployment metadata test: database unavailable");
        return;
    };
    let pool = _db.pool().clone();

    let user_repo = PostgresUserRepository::new(pool.clone());
    let tenant_repo = PostgresTenantRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".to_string());

    let user_id = user_repo
        .create(NewUser {
            email: format!("hypervisor_test_{}@example.com", Uuid::new_v4()),
            password_hash: "pass".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
        })
        .await
        .expect("failed to create user");

    let tenant = tenant_repo
        .create(
            "Hypervisor Project".into(),
            mikrom_api::domain::Tenant::generate_slug(),
        )
        .await
        .expect("failed to create tenant");
    tenant_repo
        .add_member(tenant.id, user_id, "admin")
        .await
        .expect("failed to add tenant member");

    let app = app_repo
        .create_app(CreateAppParams {
            name: "hypervisor-app".to_string(),
            git_url: "https://github.com/test/repo".to_string(),
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            tenant_id: tenant.id,
            ..Default::default()
        })
        .await
        .expect("failed to create app");

    let deployment = app_repo
        .create_deployment(NewDeployment {
            app_id: app.id,
            user_id,
            tenant_id: tenant.id.to_string(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            env_vars: std::collections::HashMap::new(),
            trigger_source: "manual".to_string(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            hypervisor: 2,
        })
        .await
        .expect("failed to create deployment");

    let raw_hypervisor: i16 =
        sqlx::query_scalar("SELECT hypervisor FROM deployments WHERE id = $1")
            .bind(deployment.id)
            .fetch_one(&pool)
            .await
            .expect("failed to fetch raw hypervisor");

    assert_eq!(raw_hypervisor, 2);

    let fetched = app_repo
        .get_deployment(deployment.id)
        .await
        .expect("failed to fetch deployment")
        .expect("deployment missing");

    assert_eq!(fetched.hypervisor, 2);
}
