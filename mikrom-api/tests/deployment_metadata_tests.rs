use mikrom_api::domain::user::{NewUser, UserRepository, UserRole};
use mikrom_api::domain::{AppRepository, NewDeployment, UpdateDeploymentParams};
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::infrastructure::db::PostgresUserRepository;
use mikrom_api::test_utils::TestDb;
use uuid::Uuid;

#[path = "common/mod.rs"]
mod common;

#[tokio::test]
async fn test_deployment_metadata_roundtrip() {
    let db = TestDb::new().await;
    let pool = db.pool().clone();

    let user_repo = PostgresUserRepository::new(pool.clone());
    let app_repo = PostgresAppRepository::new(pool.clone(), "test-key".to_string());

    // 1. Create a user
    let email = format!("metadata_test_{}@example.com", Uuid::new_v4());
    let user_id = user_repo
        .create(NewUser {
            email,
            password_hash: "pass".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await
        .expect("failed to create user");

    // 2. Create an app
    let app = app_repo
        .create_app(mikrom_api::domain::CreateAppParams {
            name: "metadata-app".to_string(),
            git_url: "https://github.com/test/repo".to_string(),
            port: mikrom_api::domain::types::Port::new(80).unwrap(),
            user_id,
            ..Default::default()
        })
        .await
        .expect("failed to create app");

    // 3. Create a deployment with trigger_source
    let deployment = app_repo
        .create_deployment(NewDeployment {
            app_id: app.id,
            user_id: user_id.to_string(),
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

    assert_eq!(deployment.trigger_source, "github_webhook");
    assert!(deployment.git_commit_hash.is_none());

    // 4. Update with git metadata
    let commit_hash = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0";
    let commit_msg = "feat: add exhaustive metadata tests";
    let branch = "feature/metadata";

    app_repo
        .update_deployment(
            deployment.id,
            UpdateDeploymentParams {
                status: Some("SUCCESS".to_string()),
                job_id: Some("job-abc".to_string()),
                image_tag: Some("img:v2".to_string()),
                build_id: Some("build-xyz".to_string()),
                ipv6_address: None,
                git_commit_hash: Some(commit_hash.to_string()),
                git_commit_message: Some(commit_msg.to_string()),
                git_branch: Some(branch.to_string()),
            },
        )
        .await
        .expect("failed to update deployment status with metadata");

    // 5. Verify persistence
    let updated = app_repo
        .get_deployment(deployment.id)
        .await
        .expect("failed to get deployment")
        .expect("deployment not found");

    assert_eq!(updated.status, "SUCCESS");
    assert_eq!(updated.trigger_source, "github_webhook");
    assert_eq!(updated.git_commit_hash.as_deref(), Some(commit_hash));
    assert_eq!(updated.git_commit_message.as_deref(), Some(commit_msg));
    assert_eq!(updated.git_branch.as_deref(), Some(branch));
}
