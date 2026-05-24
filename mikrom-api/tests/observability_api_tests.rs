mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::MockAppRepository;
use mikrom_api::domain::user::{MockUserRepository, UserRole};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower::Service;
use uuid::Uuid;

const JWT_SECRET: &str = "test-secret";

async fn setup_app(mock_app_repo: MockAppRepository) -> Option<(axum::Router, async_nats::Client)> {
    let mock_user_repo = MockUserRepository::new();
    let (deployment_events, _) = tokio::sync::broadcast::channel(100);
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = async_nats::connect(nats_url).await.ok()?;

    let mut mock_scheduler = mikrom_api::domain::MockScheduler::new();
    mock_scheduler
        .expect_list_apps()
        .times(0..)
        .returning(|_| Ok(mikrom_proto::scheduler::ListAppsResponse::default()));

    let state = AppState {
        ctx: mikrom_api::application::ApiContext::default(),
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        volume_repo: Arc::new(mikrom_api::domain::MockVolumeRepository::new()),
        github_repo: Arc::new(mikrom_api::domain::github::MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/dummy")
            .unwrap(),
        jwt_secret: JWT_SECRET.into(),
        master_key: "key".into(),
        deployment_events: deployment_events.clone(),
        acme_email: "admin@mikrom.spluca.org".into(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        mesh_status: tokio::sync::watch::channel(
            mikrom_api::application::vms::MeshStatus::default(),
        )
        .0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    Some((create_app(state), nats_client))
}

#[tokio::test]
async fn test_app_logs_stream_auth() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-logs-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            ..Default::default()
        }))
    });

    let Some((mut router, _)) = setup_app(mock_app_repo).await else {
        return;
    };
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/logs/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}

#[tokio::test]
async fn test_app_metrics_stream_auth() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let app_name = "test-metrics-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            ..Default::default()
        }))
    });

    let Some((mut router, _)) = setup_app(mock_app_repo).await else {
        return;
    };
    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/metrics/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}

#[tokio::test]
async fn test_app_logs_stream_includes_scale_state() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let app_name = "test-logs-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            active_deployment_id: Some(dep_id),
            desired_replicas: 1,
            ..Default::default()
        }))
    });
    mock_app_repo
        .expect_get_active_deployment()
        .returning(move |_| {
            Ok(Some(mikrom_api::domain::app::Deployment {
                id: dep_id,
                app_id,
                user_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-1".to_string()),
                ..Default::default()
            }))
        });

    let Some((mut router, nats_client)) = setup_app(mock_app_repo).await else {
        return;
    };

    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/logs/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let payload = serde_json::json!({
        "message": "hello",
    });
    let subject = format!("mikrom.logs.{}.stdout", app_id);
    nats_client
        .publish(subject, serde_json::to_vec(&payload).unwrap().into())
        .await
        .unwrap();

    let mut body_stream = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), body_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);

    assert!(chunk_str.contains("scale_state"));
    assert!(chunk_str.contains("warming_up"));
}

#[tokio::test]
async fn test_app_logs_stream_wraps_plain_text_with_scale_state() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let app_name = "test-plain-logs-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            active_deployment_id: Some(dep_id),
            desired_replicas: 1,
            ..Default::default()
        }))
    });
    mock_app_repo
        .expect_get_active_deployment()
        .returning(move |_| {
            Ok(Some(mikrom_api::domain::app::Deployment {
                id: dep_id,
                app_id,
                user_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-1".to_string()),
                ..Default::default()
            }))
        });

    let Some((mut router, nats_client)) = setup_app(mock_app_repo).await else {
        return;
    };

    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/logs/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let subject = format!("mikrom.logs.{}.stdout", app_id);
    nats_client
        .publish(subject, "plain log line".as_bytes().to_vec().into())
        .await
        .unwrap();

    let mut body_stream = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), body_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);

    assert!(chunk_str.contains("plain log line"));
    assert!(chunk_str.contains("scale_state"));
    assert!(chunk_str.contains("warming_up"));
}

#[tokio::test]
async fn test_app_metrics_stream_includes_scale_state() {
    let mut mock_app_repo = MockAppRepository::new();
    let app_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();
    let app_name = "test-metrics-app";

    mock_app_repo.expect_get_app_by_name().returning(move |_| {
        Ok(Some(mikrom_api::domain::app::App {
            id: app_id,
            name: "test-app".to_string(),
            git_url: "git".to_string(),
            port: mikrom_api::domain::types::Port::new(8080).unwrap(),
            user_id,
            active_deployment_id: Some(dep_id),
            desired_replicas: 1,
            ..Default::default()
        }))
    });
    mock_app_repo
        .expect_get_active_deployment()
        .returning(move |_| {
            Ok(Some(mikrom_api::domain::app::Deployment {
                id: dep_id,
                app_id,
                user_id,
                status: "RUNNING".to_string(),
                job_id: Some("job-1".to_string()),
                ..Default::default()
            }))
        });

    let Some((mut router, nats_client)) = setup_app(mock_app_repo).await else {
        return;
    };

    let token = create_token(
        &user_id.to_string(),
        "test@test.com",
        &UserRole::User,
        JWT_SECRET,
    )
    .unwrap();

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/apps/{}/metrics/stream", app_name))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let response = router.call(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let payload = serde_json::json!({
        "deployment_id": dep_id.to_string(),
        "status": "RUNNING",
        "cpu_usage": 10.0,
        "ram_used_bytes": 1024,
    });
    let subject = format!("mikrom.metrics.{}.vm-1", app_id);
    nats_client
        .publish(subject, serde_json::to_vec(&payload).unwrap().into())
        .await
        .unwrap();

    let mut body_stream = response.into_body().into_data_stream();
    let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), body_stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let chunk_str = String::from_utf8_lossy(&chunk);

    assert!(chunk_str.contains("scale_state"));
    assert!(chunk_str.contains("warming_up"));
}
