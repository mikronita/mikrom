use mikrom_api::AppState;
use mikrom_api::application::auth::{AuthService, RegisterParams};
use mikrom_api::domain::github::MockGithubRepository;
use mikrom_api::domain::user::{MockUserRepository, User, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository, NewUser,
};
use mikrom_api::workspace::WorkspaceEventKind;
use std::sync::Arc;
use totp_rs::{Secret, TOTP};
use uuid::Uuid;

fn build_state(
    user_repo: Arc<MockUserRepository>,
    tenant_repo: Arc<MockTenantRepository>,
) -> AppState {
    AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo,
        tenant_repo,
        app_repo: Arc::new(MockAppRepository::new()),
        database_repo: Arc::new(MockDatabaseRepository::new()),
        github_repo: Arc::new(MockGithubRepository::default()),
        volume_repo: Arc::new(MockVolumeRepository::new()),
        scheduler: Arc::new(MockScheduler::new()),
        nats: mikrom_api::nats::TypedNatsClient::new_custom(Arc::new(
            mikrom_api::nats::MockNatsClient::new(),
        )),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: "integration-test-secret".to_string(),
        master_key: "integration-master-key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(1).0,
        workspace_events: tokio::sync::broadcast::channel(1).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        acme_email: "admin@mikrom.spluca.org".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    }
}

fn user(id: Uuid, email: &str) -> User {
    User {
        id,
        email: email.to_string(),
        password_hash: "hash".to_string(),
        role: UserRole::User,
        first_name: None,
        last_name: None,
        avatar_url: None,
        vpc_ipv6_prefix: Some("fd00::".to_string()),
        totp_secret: None,
        totp_enabled: false,
        deleted_at: None,
    }
}

#[tokio::test]
async fn register_creates_tenant_and_emits_profile_flow_inputs() {
    let user_id = Uuid::new_v4();
    let tenant_id = Uuid::new_v4();
    let email = "register@example.com".to_string();
    let email_for_create = email.clone();
    let email_for_find = email.clone();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_count_by_email().returning(|_| Ok(0));
    user_repo
        .expect_create()
        .returning(move |new_user: NewUser| {
            assert_eq!(new_user.email, email_for_create);
            Ok(user_id)
        });
    user_repo
        .expect_find_by_id()
        .returning(move |_| Ok(Some(user(user_id, &email_for_find))));

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo.expect_create().returning(move |name, _slug| {
        assert_eq!(name, "Default Project");
        Ok(mikrom_api::domain::Tenant {
            id: tenant_id,
            tenant_id: "abc123".to_string(),
            name,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    });
    tenant_repo
        .expect_add_member()
        .returning(move |tid, uid, role| {
            assert_eq!(tid, tenant_id);
            assert_eq!(uid, user_id);
            assert_eq!(role, "admin");
            Ok(())
        });

    let state = build_state(Arc::new(user_repo), Arc::new(tenant_repo));
    let result = AuthService::register(
        &state,
        RegisterParams {
            email: email.clone(),
            password: "password123".to_string(),
            first_name: Some("Ada".to_string()),
            last_name: Some("Lovelace".to_string()),
            avatar_url: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.user.id, user_id);
    assert!(result.token.as_ref().unwrap().len() > 10);
}

#[tokio::test]
async fn login_returns_token_for_valid_credentials() {
    let user_id = Uuid::new_v4();
    let email = "login@example.com";
    let password = "password123";
    let password_hash = mikrom_api::crypto::hash_password(password).unwrap();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(move |_| {
        Ok(Some(User {
            id: user_id,
            email: email.to_string(),
            password_hash: password_hash.clone(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::login(&state, email.to_string(), password.to_string(), None)
        .await
        .unwrap();

    assert_eq!(result.user.id, user_id);
    assert!(result.token.as_ref().unwrap().len() > 10);
}

#[tokio::test]
async fn login_rejects_invalid_password() {
    let user_id = Uuid::new_v4();
    let email = "login@example.com";

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_email().returning(move |_| {
        Ok(Some(User {
            id: user_id,
            email: email.to_string(),
            password_hash: mikrom_api::crypto::hash_password("correct-password").unwrap(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::login(
        &state,
        email.to_string(),
        "wrong-password".to_string(),
        None,
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn update_profile_emits_workspace_event() {
    let user_id = Uuid::new_v4();
    let email = "profile@example.com";

    let mut user_repo = MockUserRepository::new();
    user_repo
        .expect_update_profile()
        .returning(move |id, first_name, last_name, avatar_url| {
            assert_eq!(id, user_id);
            assert_eq!(first_name.as_deref(), Some("Ada"));
            assert_eq!(last_name.as_deref(), Some("Lovelace"));
            assert!(avatar_url.is_none());
            Ok(User {
                id,
                email: email.to_string(),
                password_hash: "hash".to_string(),
                role: UserRole::User,
                first_name,
                last_name,
                avatar_url: None,
                vpc_ipv6_prefix: Some("fd00::".to_string()),
                totp_secret: None,
                totp_enabled: false,
                deleted_at: None,
            })
        });
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: email.to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: Some("Ada".to_string()),
            last_name: Some("Lovelace".to_string()),
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo.expect_get_members().returning(|_| Ok(vec![]));

    let mut state = build_state(Arc::new(user_repo), Arc::new(tenant_repo));
    let (tx, mut rx) = tokio::sync::broadcast::channel(1);
    state.workspace_events = tx.clone();

    let result = AuthService::update_profile_by_auth(
        &state,
        &user_id.to_string(),
        Some("Ada".to_string()),
        Some("Lovelace".to_string()),
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.first_name.as_deref(), Some("Ada"));
    assert_eq!(result.last_name.as_deref(), Some("Lovelace"));
    assert!(matches!(
        rx.recv().await.unwrap().kind,
        WorkspaceEventKind::ProfileUpdated
    ));
}

#[tokio::test]
async fn change_password_success() {
    let user_id = Uuid::new_v4();
    let current_password = "current-password";
    let password_hash = mikrom_api::crypto::hash_password(current_password).unwrap();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "test@example.com".to_string(),
            password_hash: password_hash.clone(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });
    user_repo.expect_update_password().returning(|_, _| Ok(()));

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::change_password(
        &state,
        &user_id.to_string(),
        current_password.to_string(),
        "new-strong-password".to_string(),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn change_password_rejects_wrong_current() {
    let user_id = Uuid::new_v4();
    let password_hash = mikrom_api::crypto::hash_password("actual-password").unwrap();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "test@example.com".to_string(),
            password_hash: password_hash.clone(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::change_password(
        &state,
        &user_id.to_string(),
        "wrong-password".to_string(),
        "new-strong-password".to_string(),
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn setup_totp_returns_secret_and_url() {
    let user_id = Uuid::new_v4();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "totp@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });
    user_repo
        .expect_update_totp_secret()
        .returning(|_, _| Ok(()));

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::setup_totp(&state, &user_id.to_string())
        .await
        .unwrap();

    assert!(!result.secret.is_empty());
    assert!(
        result.otpauth_url.starts_with("otpauth://"),
        "URL: {}",
        result.otpauth_url
    );
    assert!(
        result.otpauth_url.contains("issuer=Mikrom"),
        "URL: {}",
        result.otpauth_url
    );
}

#[tokio::test]
async fn setup_totp_fails_when_already_enabled() {
    let user_id = Uuid::new_v4();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "totp@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: Some("secret".to_string()),
            totp_enabled: true,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::setup_totp(&state, &user_id.to_string()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn disable_totp_clears_secret_and_flag() {
    let user_id = Uuid::new_v4();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_disable_totp().returning(|_| Ok(()));

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::disable_totp(&state, &user_id.to_string()).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn verify_totp_succeeds_with_valid_code() {
    let user_id = Uuid::new_v4();
    let raw_secret = Secret::generate_secret();
    let secret_encoded = raw_secret.to_encoded().to_string();
    let secret_bytes = raw_secret.to_bytes().unwrap().to_vec();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "totp@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: Some(secret_encoded.clone()),
            totp_enabled: false,
            deleted_at: None,
        }))
    });
    user_repo.expect_enable_totp().returning(|_| Ok(()));

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let totp = TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        None,
        String::new(),
    )
    .unwrap();
    let code = totp.generate_current().unwrap();

    let result = AuthService::verify_totp(&state, &user_id.to_string(), code).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn verify_totp_fails_with_invalid_code() {
    let user_id = Uuid::new_v4();
    let raw_secret = Secret::generate_secret();
    let secret_encoded = raw_secret.to_encoded().to_string();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "totp@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: Some(secret_encoded.clone()),
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::verify_totp(&state, &user_id.to_string(), "000000".to_string()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn verify_totp_fails_when_no_secret_stored() {
    let user_id = Uuid::new_v4();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |id| {
        Ok(Some(User {
            id,
            email: "totp@example.com".to_string(),
            password_hash: "hash".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            avatar_url: None,
            vpc_ipv6_prefix: Some("fd00::".to_string()),
            totp_secret: None,
            totp_enabled: false,
            deleted_at: None,
        }))
    });

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::verify_totp(&state, &user_id.to_string(), "000000".to_string()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn delete_account_marks_deleted_at() {
    let user_id = Uuid::new_v4();

    let mut user_repo = MockUserRepository::new();
    user_repo.expect_soft_delete().returning(|_| Ok(()));

    let state = build_state(Arc::new(user_repo), Arc::new(MockTenantRepository::new()));
    let result = AuthService::delete_account(&state, &user_id.to_string()).await;

    assert!(result.is_ok());
}
