use mikrom_api::domain::user::{NewUser, UserRepository, UserRole};
use mikrom_api::domain::{AppRepository, NewDeployment};
use mikrom_api::infrastructure::db::PostgresAppRepository;
use mikrom_api::infrastructure::db::PostgresUserRepository;
use mikrom_api::test_utils::TestDb;
use std::collections::HashMap;
use uuid::Uuid;

#[path = "common/mod.rs"]
mod common;

#[tokio::test]
#[ignore = "requires a PostgreSQL test database with the migrated apps schema"]
async fn test_encryption_at_rest() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping encryption test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let master_key = "test-master-key-123";
    let app_repo = PostgresAppRepository::new(pool.clone(), master_key.to_string());
    let user_repo = PostgresUserRepository::new(pool.clone());

    // 1. Create a user
    let user_id = user_repo
        .create(NewUser {
            email: format!("encrypt_test_{}@example.com", Uuid::new_v4()),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await
        .expect("failed to create user");

    // 2. Create an app with a secret webhook
    let webhook_secret = "super-secret-webhook-key";
    let app = app_repo
        .create_app(mikrom_api::domain::CreateAppParams {
            name: format!("test-app-{}", Uuid::new_v4()),
            git_url: "https://github.com/test/repo".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            tenant_id: user_id,
            github_webhook_secret: Some(webhook_secret.to_string()),
            ..Default::default()
        })
        .await
        .expect("failed to create app");

    // Verify it's decrypted in the response
    assert_eq!(app.github_webhook_secret.as_deref(), Some(webhook_secret));

    // Verify it's encrypted in the database
    let raw_secret: String =
        sqlx::query_scalar("SELECT github_webhook_secret FROM apps WHERE id = $1")
            .bind(app.id)
            .fetch_one(&pool)
            .await
            .expect("failed to fetch raw secret");

    assert_ne!(raw_secret, webhook_secret);
    // It should be a base64 string (from crypto::encrypt)
    assert!(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &raw_secret).is_ok()
    );

    // 3. Create a deployment with env vars
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "DATABASE_URL".to_string(),
        "postgres://user:pass@host/db".to_string(),
    );
    env_vars.insert("API_KEY".to_string(), "sk_live_123456".to_string());

    let deployment = app_repo
        .create_deployment(NewDeployment {
            app_id: app.id,
            user_id,
            tenant_id: user_id.to_string(),
            vcpus: mikrom_api::domain::types::CpuCores::new(1).unwrap(),
            memory_mib: mikrom_api::domain::types::MemoryMb::new(256).unwrap(),
            disk_mib: 1024,
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            env_vars: env_vars.clone(),
            trigger_source: "manual".to_string(),
            git_commit_hash: None,
            git_commit_message: None,
            git_branch: None,
            hypervisor: 0,
        })
        .await
        .expect("failed to create deployment");

    // Verify it's decrypted in the response
    assert_eq!(
        deployment.env_vars,
        serde_json::to_value(&env_vars).unwrap()
    );

    // Verify it's encrypted in the database
    let raw_env: serde_json::Value =
        sqlx::query_scalar("SELECT env_vars FROM deployments WHERE id = $1")
            .bind(deployment.id)
            .fetch_one(&pool)
            .await
            .expect("failed to fetch raw env");

    let encrypted_string = raw_env
        .as_str()
        .expect("env_vars should be stored as a JSON string containing the encrypted data");
    assert_ne!(encrypted_string, serde_json::to_string(&env_vars).unwrap());
    assert!(
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encrypted_string)
            .is_ok()
    );

    // 4. Test decryption with wrong key (simulated by creating a new repo instance)
    let app_repo_wrong_key = PostgresAppRepository::new(pool.clone(), "wrong-key".to_string());
    let result = app_repo_wrong_key.get_app(app.id).await;

    // Now it should return an error as per code review feedback
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Failed to decrypt") || err_msg.contains("Decryption failed"));

    let deployment_result = app_repo_wrong_key.get_deployment(deployment.id).await;
    assert!(deployment_result.is_err());
    let deployment_err_msg = deployment_result.unwrap_err().to_string();
    assert!(
        deployment_err_msg.contains("Failed to decrypt")
            || deployment_err_msg.contains("Decryption failed")
    );
}
