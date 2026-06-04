use std::sync::Arc;

use chrono::Utc;
use mockall::predicate::eq;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::application::database::DatabaseService;
use mikrom_api::domain::{
    CpuCores, Database, DatabaseDeployment, DatabaseStatus, MemoryMb, MockDatabaseRepository,
};

fn build_state(mock_repo: MockDatabaseRepository) -> AppState {
    let mut state = AppState::default();
    let repo = Arc::new(mock_repo);
    state.database_repo = repo.clone();
    state.ctx.database_repo = repo;
    state
}

fn database(id: Uuid, tenant_id: Uuid, active_deployment_id: Option<Uuid>) -> Database {
    Database {
        id,
        name: "orders".to_string(),
        engine: "neon".to_string(),
        postgres_version: 16,
        tenant_id,
        vcpus: CpuCores::new(1).unwrap(),
        memory_mib: MemoryMb::new(512).unwrap(),
        disk_mib: 1024,
        neon_tenant_id: Some("11111111111111111111111111111111".to_string()),
        neon_timeline_id: Some("22222222222222222222222222222222".to_string()),
        tenant_gen: Some(1),
        settings: Default::default(),
        status: DatabaseStatus::Running,
        active_deployment_id,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn deployment(
    id: Uuid,
    database_id: Uuid,
    tenant_id: Uuid,
    ipv6_address: Option<String>,
) -> DatabaseDeployment {
    DatabaseDeployment {
        id,
        database_id,
        tenant_id,
        job_id: Some("job-1".to_string()),
        status: "RUNNING".to_string(),
        host_id: Some("host-1".to_string()),
        vm_id: Some("vm-1".to_string()),
        ipv6_address,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn get_connection_info_builds_ssh_tunnel_command() {
    let database_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let tenant_id = Uuid::new_v4();
    let vm_ipv6 = "fdac:5111:a306:6c32::1".to_string();

    let mut mock_repo = MockDatabaseRepository::new();
    mock_repo
        .expect_get_database()
        .with(eq(database_id))
        .times(1)
        .returning({
            let db = database(database_id, tenant_id, Some(deployment_id));
            move |_| Ok(Some(db.clone()))
        });
    mock_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .times(1)
        .returning({
            let dep = deployment(deployment_id, database_id, tenant_id, Some(vm_ipv6.clone()));
            move |_| Ok(Some(dep.clone()))
        });

    let state = build_state(mock_repo);
    let connection = DatabaseService::get_connection_info(&state, database_id, tenant_id)
        .await
        .expect("connection info");

    assert_eq!(connection.database_id, database_id);
    assert_eq!(connection.database_name, "orders");
    assert_eq!(connection.database_host, "127.0.0.1");
    assert_eq!(connection.database_port, 5432);
    assert_eq!(connection.ssh_host, vm_ipv6);
    assert_eq!(connection.ssh_user, "mikrom");
    assert_eq!(connection.ssh_port, 22);
    assert_eq!(
        connection.ssh_tunnel_command,
        format!(
            "ssh -N -L 5432:127.0.0.1:5432 mikrom@[{}]",
            connection.ssh_host
        )
    );
    assert_eq!(
        connection.psql_command,
        "psql \"host=127.0.0.1 port=5432 user=cloud_admin dbname=orders\""
    );
}

#[tokio::test]
async fn get_connection_info_requires_an_active_deployment() {
    let database_id = Uuid::new_v4();
    let tenant_id = Uuid::new_v4();

    let mut mock_repo = MockDatabaseRepository::new();
    mock_repo
        .expect_get_database()
        .with(eq(database_id))
        .times(1)
        .returning({
            let db = database(database_id, tenant_id, None);
            move |_| Ok(Some(db.clone()))
        });

    let state = build_state(mock_repo);
    let err = DatabaseService::get_connection_info(&state, database_id, tenant_id)
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("Database has no active deployment yet")
    );
}
