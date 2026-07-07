use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mockall::predicate::eq;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::Tenant;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockTenantRepository,
    MockVolumeRepository, TenantMember,
};

#[allow(clippy::field_reassign_with_default)]
fn build_state(tenant_id: Uuid, owner_user_id: Uuid) -> AppState {
    let mut user_repo = MockUserRepository::new();
    user_repo.expect_find_by_id().returning(move |_| {
        Ok(Some(mikrom_api::domain::user::User {
            id: owner_user_id,
            email: "owner@example.com".to_string(),
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

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });

    let mut scheduler = MockScheduler::new();
    scheduler
        .expect_update_app_scaling_config()
        .returning(|_| Ok(true));
    scheduler
        .expect_list_apps()
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse::default()));

    let mut app_repo = MockAppRepository::new();
    app_repo
        .expect_list_apps_by_tenant()
        .returning(|_| Ok(vec![]));

    let mut database_repo = MockDatabaseRepository::new();
    database_repo
        .expect_list_databases_by_tenant()
        .returning(|_| Ok(vec![]));

    let mut volume_repo = MockVolumeRepository::new();
    volume_repo
        .expect_list_volumes_by_tenant()
        .returning(|_| Ok(vec![]));

    let mut state = AppState::default();
    state.jwt_secret = "test-secret".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.user_repo = Arc::new(user_repo);
    state.ctx.user_repo = state.user_repo.clone();
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();
    state.app_repo = Arc::new(app_repo);
    state.ctx.app_repo = state.app_repo.clone();
    state.database_repo = Arc::new(database_repo);
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(volume_repo);
    state.ctx.volume_repo = state.volume_repo.clone();
    state.scheduler = Arc::new(scheduler);
    state.ctx.scheduler = state.scheduler.clone();
    state
}

#[tokio::test]
async fn list_projects_returns_current_tenant_projects() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "abc123".to_string(),
        name: "Default Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_list_by_user()
        .with(eq(owner_user_id))
        .returning(move |_| Ok(vec![tenant.clone()]));
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/projects")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let projects: Value = serde_json::from_slice(&body).unwrap();
    let tenant_id_str = tenant_id.to_string();
    let projects = projects.as_array().expect("projects should be an array");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["id"].as_str(), Some(tenant_id_str.as_str()));
    assert_eq!(projects[0]["tenant_id"], "abc123");
    assert_eq!(projects[0]["name"], "Default Project");
}

#[tokio::test]
async fn get_project_returns_a_single_tenant_for_a_member() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "abc123".to_string(),
        name: "Default Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_find_by_slug()
        .with(eq("abc123"))
        .returning({
            let tenant = tenant.clone();
            move |_| Ok(Some(tenant.clone()))
        });
    tenant_repo
        .expect_is_member()
        .with(eq(tenant_id), eq(owner_user_id))
        .returning(|_, _| Ok(true));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/projects/abc123")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let project: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(project["tenant_id"], "abc123");
    assert_eq!(project["name"], "Default Project");
}

#[tokio::test]
async fn create_project_requires_authentication() {
    let router = create_app(AppState::default());

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"New Project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn update_project_renames_tenant_for_admins() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "abc123".to_string(),
        name: "Default Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let updated_tenant = Tenant {
        name: "Renamed Project".to_string(),
        ..tenant.clone()
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_find_by_slug()
        .with(eq("abc123"))
        .returning({
            let tenant = tenant.clone();
            move |_| Ok(Some(tenant.clone()))
        });
    tenant_repo
        .expect_is_member()
        .with(eq(tenant_id), eq(owner_user_id))
        .returning(|_, _| Ok(true));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });
    tenant_repo
        .expect_update()
        .with(eq(tenant_id), eq("Renamed Project".to_string()))
        .returning(move |_, _| Ok(updated_tenant.clone()));
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/v1/projects/abc123")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Renamed Project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let project: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(project["name"], "Renamed Project");
    assert_eq!(project["tenant_id"], "abc123");
}

#[tokio::test]
async fn delete_project_is_blocked_when_resources_exist() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "abc123".to_string(),
        name: "Default Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_find_by_slug()
        .with(eq("abc123"))
        .returning({
            let tenant = tenant.clone();
            move |_| Ok(Some(tenant.clone()))
        });
    tenant_repo
        .expect_is_member()
        .with(eq(tenant_id), eq(owner_user_id))
        .returning(|_, _| Ok(true));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let mut app_repo = MockAppRepository::new();
    app_repo.expect_list_apps_by_tenant().returning(move |_| {
        Ok(vec![mikrom_api::domain::App {
            tenant_id,
            ..Default::default()
        }])
    });
    state.app_repo = Arc::new(app_repo);
    state.ctx.app_repo = state.app_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/projects/abc123")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_project_removes_empty_tenant() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let tenant = Tenant {
        id: tenant_id,
        tenant_id: "abc123".to_string(),
        name: "Default Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_find_by_slug()
        .with(eq("abc123"))
        .returning({
            let tenant = tenant.clone();
            move |_| Ok(Some(tenant.clone()))
        });
    tenant_repo
        .expect_is_member()
        .with(eq(tenant_id), eq(owner_user_id))
        .returning(|_, _| Ok(true));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });
    tenant_repo
        .expect_delete()
        .with(eq(tenant_id))
        .returning(|_| Ok(true));
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/projects/abc123")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn create_project_creates_tenant_for_user() {
    let tenant_id = Uuid::new_v4();
    let owner_user_id = Uuid::new_v4();
    let mut state = build_state(tenant_id, owner_user_id);

    let created_tenant = Tenant {
        id: Uuid::new_v4(),
        tenant_id: "xyz789".to_string(),
        name: "New Project".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut tenant_repo = MockTenantRepository::new();
    tenant_repo
        .expect_create()
        .withf(|name, slug| name == "New Project" && slug.len() == 6)
        .returning(move |name, _slug| {
            Ok(Tenant {
                id: created_tenant.id,
                tenant_id: created_tenant.tenant_id.clone(),
                name,
                created_at: created_tenant.created_at,
                updated_at: created_tenant.updated_at,
            })
        });
    tenant_repo
        .expect_add_member()
        .with(eq(created_tenant.id), eq(owner_user_id), eq("admin"))
        .returning(|_, _, _| Ok(()));
    tenant_repo.expect_get_members().returning(move |_| {
        Ok(vec![TenantMember {
            tenant_id,
            user_id: owner_user_id,
            role: "admin".to_string(),
        }])
    });
    tenant_repo.expect_list_by_user().returning(|_| Ok(vec![]));
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();

    let token = create_token(
        &owner_user_id.to_string(),
        "test@example.com",
        &UserRole::User,
        "test-secret",
    )
    .unwrap();

    let router = create_app(state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header("Authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"New Project"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    let project: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(project["name"], "New Project");
}
