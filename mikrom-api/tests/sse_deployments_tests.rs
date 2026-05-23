use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mockall::predicate::eq;
use tokio_stream::StreamExt;
use tower::Service;
use tower::ServiceExt;

use mikrom_api::domain::app::{App, Deployment};
use mikrom_api::domain::{MockAppRepository, MockUserRepository, UserRole};
use mikrom_api::infrastructure::auth::jwt::create_token;
use mikrom_api::test_utils::create_test_app_state;

#[path = "common/mod.rs"]
mod common;

const JWT_SECRET: &str = "test-secret";

async fn setup_app(mock_app_repo: MockAppRepository) -> Option<axum::Router> {
    let mock_user_repo = MockUserRepository::new();
    let nats_client = common::get_nats_client_or_skip().await?;

    let mut state =
        create_test_app_state(sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap());
    state.user_repo = Arc::new(mock_user_repo);
    state.app_repo = Arc::new(mock_app_repo);
    state.ctx.user_repo = state.user_repo.clone();
    state.ctx.app_repo = state.app_repo.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client);
    state.ctx.nats = state.nats.clone();
    state.jwt_secret = JWT_SECRET.into();
    state.ctx.jwt_secret = JWT_SECRET.into();

    Some(mikrom_api::create_app(state))
}

#[tokio::test]
async fn test_sse_deployments_stream_initial_data() {
    let app_name = "test-app";
    let app_id = uuid::Uuid::new_v4();
    let user_id = uuid::Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                user_id,
                ..Default::default()
            }))
        });

    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));

    let router = setup_app(mock_app_repo).await;
    if router.is_none() {
        return;
    }
    let router = router.unwrap();

    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/deployments/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let body = response.into_body().into_data_stream();
    let mut chunks = body;
    let first_chunk = chunks.next().await.unwrap().unwrap();
    let first_str = String::from_utf8_lossy(&first_chunk);

    assert!(first_str.contains("data: []"));
}

#[tokio::test]
async fn test_sse_deployments_stream_updates() {
    let app_name = "test-app-updates";
    let app_id = uuid::Uuid::new_v4();
    let user_id = uuid::Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                user_id,
                ..Default::default()
            }))
        });

    // 1. Initial list empty
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .times(1)
        .returning(|_| Ok(vec![]));

    // 2. Second list (after event) has one deployment
    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .times(1)
        .returning(move |_| {
            Ok(vec![Deployment {
                id: uuid::Uuid::new_v4(),
                app_id,
                user_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-updated".to_string()),
                ..Default::default()
            }])
        });

    let mock_user_repo = MockUserRepository::new();
    let (deployment_events, _) = tokio::sync::broadcast::channel::<uuid::Uuid>(100);
    let tx = deployment_events.clone();
    let Some(nats_client) = common::get_nats_client_or_skip().await else {
        return;
    };

    let mut state =
        create_test_app_state(sqlx::PgPool::connect_lazy("postgres://localhost/dummy").unwrap());
    state.user_repo = Arc::new(mock_user_repo);
    state.app_repo = Arc::new(mock_app_repo);
    state.ctx.user_repo = state.user_repo.clone();
    state.ctx.app_repo = state.app_repo.clone();
    state.nats = mikrom_api::nats::TypedNatsClient::new(nats_client);
    state.ctx.nats = state.nats.clone();
    state.jwt_secret = JWT_SECRET.into();
    state.ctx.jwt_secret = JWT_SECRET.into();
    state.deployment_events = tx.clone();

    let mut router = mikrom_api::create_app(state);
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/deployments/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    let mut body_stream = response.into_body().into_data_stream();

    // 1. Initial empty data
    let first_chunk = body_stream.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first_chunk).contains("[]"));

    // 2. Trigger event
    tx.send(app_id).unwrap();

    // 3. Receive update
    let second_chunk = body_stream.next().await.unwrap().unwrap();
    let second_str = String::from_utf8_lossy(&second_chunk);
    assert!(second_str.contains("job-updated"));
    assert!(second_str.contains("RUNNING"));
}

#[tokio::test]
async fn test_sse_deployments_auth_via_query_param() {
    let app_name = "test-app-query-auth";
    let app_id = uuid::Uuid::new_v4();
    let user_id = uuid::Uuid::new_v4();

    let mut mock_app_repo = MockAppRepository::new();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq(app_name))
        .returning(move |_| {
            Ok(Some(App {
                id: app_id,
                name: app_name.to_string(),
                user_id,
                ..Default::default()
            }))
        });

    mock_app_repo
        .expect_list_deployments_by_app()
        .with(eq(app_id))
        .returning(|_| Ok(vec![]));

    let router = setup_app(mock_app_repo).await;
    if router.is_none() {
        return;
    }
    let router = router.unwrap();

    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/v1/apps/{}/deployments/stream?token={}",
            app_name, token
        ))
        .body(Body::empty())
        .unwrap();

    let response = router.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
