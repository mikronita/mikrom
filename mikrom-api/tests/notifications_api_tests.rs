use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::Utc;
use mikrom_api::application::notifications::project_workspace_event;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::UserRole;
use mikrom_api::test_utils::{TestDb, create_test_app_state};
use mikrom_api::workspace::{WorkspaceEvent, WorkspaceEventKind};
use serial_test::serial;
use sqlx::Row;
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &str = "test-secret";

async fn insert_user_and_tenant(
    db: &sqlx::PgPool,
    user_id: Uuid,
    tenant_id: Uuid,
    tenant_slug: &str,
) {
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(format!("user-{}@example.com", user_id))
        .bind("hash")
        .execute(db)
        .await
        .unwrap();

    sqlx::query("INSERT INTO tenants (id, tenant_id, name) VALUES ($1, $2, $3)")
        .bind(tenant_id)
        .bind(tenant_slug)
        .bind("Acme")
        .execute(db)
        .await
        .unwrap();

    sqlx::query("INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(tenant_id)
        .bind(user_id)
        .bind("admin")
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn project_workspace_event_creates_notifications_for_each_tenant_member() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping notifications test: database unavailable");
        return;
    };
    let mut state = create_test_app_state(db.pool().clone());
    state.jwt_secret = JWT_SECRET.to_string();
    state.master_key = "test-master-key".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.ctx.master_key = state.master_key.clone();
    state.api_db = db.pool().clone();
    state.ctx.db = db.pool().clone();

    let tenant_id = Uuid::new_v4();
    let first_user = Uuid::new_v4();
    let second_user = Uuid::new_v4();
    let tenant_slug = Uuid::new_v4().simple().to_string();
    insert_user_and_tenant(&state.api_db, first_user, tenant_id, &tenant_slug[..6]).await;
    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(second_user)
        .bind(format!("user-{}@example.com", second_user))
        .bind("hash")
        .execute(&state.api_db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ($1, $2, $3)")
        .bind(tenant_id)
        .bind(second_user)
        .bind("member")
        .execute(&state.api_db)
        .await
        .unwrap();

    project_workspace_event(
        &state,
        WorkspaceEvent {
            kind: WorkspaceEventKind::AppCreated,
            user_id: None,
            tenant_id: Some(tenant_id),
            app_id: Some(Uuid::new_v4()),
            app_name: Some("starter".to_string()),
            deployment_id: None,
            volume_id: None,
            resource_id: None,
        },
    )
    .await
    .unwrap();

    let rows = sqlx::query(
        "SELECT user_id, kind, title, route, entity_name, resource_id FROM workspace_notifications WHERE tenant_id = $1 ORDER BY created_at ASC",
    )
    .bind(tenant_id)
    .fetch_all(&state.api_db)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2);
    let mut actual_user_ids = rows
        .iter()
        .map(|row| row.try_get::<Uuid, _>("user_id").unwrap())
        .collect::<Vec<_>>();
    actual_user_ids.sort_unstable();

    let mut expected_user_ids = vec![first_user, second_user];
    expected_user_ids.sort_unstable();

    assert_eq!(actual_user_ids, expected_user_ids);
    for row in &rows {
        assert_eq!(row.try_get::<String, _>("kind").unwrap(), "app_created");
        assert_eq!(
            row.try_get::<String, _>("title").unwrap(),
            "Application created"
        );
        assert_eq!(row.try_get::<String, _>("route").unwrap(), "/apps/starter");
        assert_eq!(
            row.try_get::<Option<String>, _>("entity_name").unwrap(),
            Some("starter".to_string())
        );
    }

    project_workspace_event(
        &state,
        WorkspaceEvent {
            kind: WorkspaceEventKind::Refresh,
            user_id: Some(first_user),
            tenant_id: Some(tenant_id),
            app_id: None,
            app_name: None,
            deployment_id: None,
            volume_id: None,
            resource_id: Some("refresh".to_string()),
        },
    )
    .await
    .unwrap();

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workspace_notifications WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_one(&state.api_db)
            .await
            .unwrap();

    assert_eq!(count, 2);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn notification_endpoints_list_and_mark_read() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping notifications test: database unavailable");
        return;
    };
    let user_id = Uuid::new_v4();
    let mut state = create_test_app_state(db.pool().clone());
    state.jwt_secret = JWT_SECRET.to_string();
    state.master_key = "test-master-key".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.ctx.master_key = state.master_key.clone();
    state.api_db = db.pool().clone();
    state.ctx.db = db.pool().clone();
    let token = create_token(
        &user_id.to_string(),
        "notifier@example.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind("notifier@example.com")
        .bind("hash")
        .execute(&state.api_db)
        .await
        .unwrap();

    let notification_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspace_notifications (id, user_id, tenant_id, kind, title, body, route, entity_name, resource_id, metadata, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, '{}'::jsonb, $10)",
    )
    .bind(notification_id)
    .bind(user_id)
    .bind(None::<Uuid>)
    .bind("billing_updated")
    .bind("Billing updated")
    .bind("Your billing status changed.")
    .bind("/settings")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(Utc::now())
    .execute(&state.api_db)
    .await
    .unwrap();

    let app = create_app(state.clone());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/notifications?limit=10")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["unread_count"], 1);
    assert_eq!(payload["notifications"].as_array().unwrap().len(), 1);
    assert_eq!(
        payload["notifications"][0]["id"],
        notification_id.to_string()
    );
    assert_eq!(payload["notifications"][0]["is_read"], false);

    let response = create_app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/notifications/{notification_id}/read"))
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let body = axum::body::to_bytes(
        create_app(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/notifications")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body(),
        1024 * 1024,
    )
    .await
    .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["unread_count"], 0);
    assert_eq!(payload["notifications"][0]["is_read"], true);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn notification_endpoints_support_pagination_and_unread_filter() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping notifications test: database unavailable");
        return;
    };
    let user_id = Uuid::new_v4();
    let mut state = create_test_app_state(db.pool().clone());
    state.jwt_secret = JWT_SECRET.to_string();
    state.master_key = "test-master-key".to_string();
    state.ctx.jwt_secret = state.jwt_secret.clone();
    state.ctx.master_key = state.master_key.clone();
    state.api_db = db.pool().clone();
    state.ctx.db = db.pool().clone();

    sqlx::query("INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind("pager@example.com")
        .bind("hash")
        .execute(&state.api_db)
        .await
        .unwrap();

    for idx in 0..3 {
        let read_at: Option<chrono::DateTime<Utc>> = if idx == 2 { Some(Utc::now()) } else { None };
        sqlx::query(
            "INSERT INTO workspace_notifications (id, user_id, tenant_id, kind, title, body, route, entity_name, resource_id, metadata, created_at, read_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, '{}'::jsonb, NOW() - ($10 * INTERVAL '1 minute'), $11)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(None::<Uuid>)
        .bind("app_updated")
        .bind(format!("Notification {idx}"))
        .bind("Body")
        .bind("/apps")
        .bind(Some(format!("app-{idx}")))
        .bind(None::<String>)
        .bind(idx)
        .bind(read_at)
        .execute(&state.api_db)
        .await
        .unwrap();
    }

    let token = create_token(
        &user_id.to_string(),
        "pager@example.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let response = create_app(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/notifications?limit=1&offset=1")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["notifications"].as_array().unwrap().len(), 1);
    assert_eq!(payload["has_more"], true);
    assert_eq!(payload["next_offset"], 2);

    let unread_response = create_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/notifications?limit=5&unread_only=true")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(unread_response.status(), StatusCode::OK);
    let unread_body = axum::body::to_bytes(unread_response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let unread_payload: serde_json::Value = serde_json::from_slice(&unread_body).unwrap();
    assert_eq!(unread_payload["notifications"].as_array().unwrap().len(), 2);
    assert_eq!(unread_payload["unread_count"], 2);
    assert_eq!(unread_payload["has_more"], false);
}
