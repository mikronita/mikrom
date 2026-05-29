use mikrom_api::domain::{CreateDatabaseParams, DatabaseRepository, DatabaseStatus};
use mikrom_api::infrastructure::db::PostgresDatabaseRepository;
use mikrom_api::test_utils::TestDb;
use std::collections::HashMap;
use uuid::Uuid;

async fn insert_user(pool: &sqlx::PgPool, user_id: Uuid) {
    sqlx::query(
        r#"
        INSERT INTO users (id, email, password_hash)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(user_id)
    .bind(format!("user-{}@example.com", user_id.simple()))
    .bind("hash")
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_postgres_database_repository_crud_and_deployments() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping database repository test: database unavailable");
        return;
    };

    let pool = db.pool().clone();
    let user_id = Uuid::new_v4();
    insert_user(&pool, user_id).await;

    let repo = PostgresDatabaseRepository::new(pool.clone());
    let params = CreateDatabaseParams {
        name: "orders".to_string(),
        engine: "neon".to_string(),
        user_id,
        vcpus: mikrom_api::domain::types::CpuCores::try_from(2).unwrap(),
        memory_mib: mikrom_api::domain::types::MemoryMb::try_from(1024).unwrap(),
        disk_mib: 4096,
        tenant_id: Some("11111111111111111111111111111111".to_string()),
        timeline_id: Some("22222222222222222222222222222222".to_string()),
        settings: HashMap::from([
            ("max_connections".to_string(), "200".to_string()),
            ("shared_buffers".to_string(), "256MB".to_string()),
        ]),
    };

    let created = repo.create_database(params).await.unwrap();
    assert_eq!(created.name, "orders");
    assert_eq!(created.engine, "neon");
    assert_eq!(created.user_id, user_id);
    assert_eq!(created.status, DatabaseStatus::Pending);
    assert_eq!(
        created.tenant_id.as_deref(),
        Some("11111111111111111111111111111111")
    );
    assert_eq!(
        created.timeline_id.as_deref(),
        Some("22222222222222222222222222222222")
    );
    assert_eq!(created.settings.get("max_connections").unwrap(), "200");

    let by_name = repo
        .get_database_by_name(user_id, "orders")
        .await
        .unwrap()
        .expect("database by name");
    assert_eq!(by_name.id, created.id);

    let listed = repo.list_databases_by_user(user_id).await.unwrap();
    assert_eq!(listed.len(), 1);

    let deployment = repo.create_deployment(created.id, user_id).await.unwrap();
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
        repo.list_databases_by_user(user_id)
            .await
            .unwrap()
            .is_empty()
    );
}
