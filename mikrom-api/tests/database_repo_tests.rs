use std::collections::HashMap;

use mikrom_api::domain::{CreateDatabaseParams, DatabaseRepository, DatabaseStatus};
use mikrom_api::infrastructure::db::PostgresDatabaseRepository;
use mikrom_api::test_utils::TestDb;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a PostgreSQL test database with the migrated schema"]
async fn test_postgres_database_repository_crud_and_deployments() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping database repository test: database unavailable");
        return;
    };

    let pool = db.pool().clone();
    let tenant_id = Uuid::new_v4();

    let repo = PostgresDatabaseRepository::new(pool.clone());
    let params = CreateDatabaseParams {
        name: "orders".to_string(),
        engine: "neon".to_string(),
        tenant_id,
        vcpus: mikrom_api::domain::types::CpuCores::try_from(2).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::try_from(1024).unwrap(),
        disk_mib: 4096,
        neon_tenant_id: Some("11111111111111111111111111111111".to_string()),
        neon_timeline_id: Some("22222222222222222222222222222222".to_string()),
        tenant_gen: Some(1),
        settings: HashMap::from([
            ("max_connections".to_string(), "200".to_string()),
            ("shared_buffers".to_string(), "256MB".to_string()),
        ]),
    };

    let created = repo.create_database(params).await.unwrap();
    assert_eq!(created.name, "orders");
    assert_eq!(created.engine, "neon");
    assert_eq!(created.tenant_id, tenant_id);
    assert_eq!(created.status, DatabaseStatus::Pending);
    assert_eq!(
        created.neon_tenant_id.as_deref(),
        Some("11111111111111111111111111111111")
    );
    assert_eq!(
        created.neon_timeline_id.as_deref(),
        Some("22222222222222222222222222222222")
    );
    assert_eq!(created.tenant_gen, Some(1));
    assert_eq!(created.settings.get("max_connections").unwrap(), "200");

    let other_tenant = Uuid::new_v4();

    let by_name = repo
        .get_database_by_name(tenant_id, "orders")
        .await
        .unwrap()
        .expect("database by name");
    assert_eq!(by_name.id, created.id);
    assert!(
        repo.get_database_by_name(other_tenant, "orders")
            .await
            .unwrap()
            .is_none()
    );

    let by_neon_tenant = repo
        .get_database_by_neon_tenant_id("11111111111111111111111111111111")
        .await
        .unwrap()
        .expect("database by neon tenant id");
    assert_eq!(by_neon_tenant.id, created.id);

    let all_databases = repo.list_databases().await.unwrap();
    assert_eq!(all_databases.len(), 1);
    assert_eq!(all_databases[0].id, created.id);

    let listed = repo.list_databases_by_tenant(tenant_id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(
        repo.list_databases_by_tenant(other_tenant)
            .await
            .unwrap()
            .is_empty()
    );

    let deployment = repo.create_deployment(created.id, tenant_id).await.unwrap();
    repo.update_active_deployment(created.id, deployment.id)
        .await
        .unwrap();
    repo.update_deployment_job_info(deployment.id, "job-1", "host-1", "vm-1")
        .await
        .unwrap();
    repo.update_deployment_status(deployment.id, "RUNNING")
        .await
        .unwrap();
    repo.update_database_status(created.id, DatabaseStatus::Running)
        .await
        .unwrap();

    let updated_db = repo.get_database(created.id).await.unwrap().unwrap();
    assert_eq!(updated_db.status, DatabaseStatus::Running);
    assert_eq!(updated_db.active_deployment_id, Some(deployment.id));

    let fetched_deployment = repo.get_deployment(deployment.id).await.unwrap().unwrap();
    assert_eq!(fetched_deployment.job_id.as_deref(), Some("job-1"));
    assert_eq!(fetched_deployment.host_id.as_deref(), Some("host-1"));
    assert_eq!(fetched_deployment.vm_id.as_deref(), Some("vm-1"));
    assert_eq!(fetched_deployment.status, "RUNNING");

    repo.delete_database(created.id).await.unwrap();
    assert!(repo.get_database(created.id).await.unwrap().is_none());
    assert!(
        repo.list_databases_by_tenant(tenant_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repo.list_databases_by_tenant(other_tenant)
            .await
            .unwrap()
            .is_empty()
    );
}
