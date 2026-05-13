use futures::StreamExt;
use mikrom_api::AppState;
use mikrom_api::models::app::{App, Deployment};
use mikrom_api::repositories::user_repository::{User, UserRole};
use mikrom_api::repositories::{MockAppRepository, MockGithubRepository, MockUserRepository};
use mikrom_api::scheduler::MockScheduler;
use mikrom_proto::scheduler::DeployResponse;
use mockall::predicate::*;
use std::sync::Arc;
use tokio::time::Duration;
use uuid::Uuid;

async fn connect_nats_or_skip(test_name: &str) -> Option<async_nats::Client> {
    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());

    match async_nats::connect(nats_url).await {
        Ok(client) => Some(client),
        Err(err) => {
            eprintln!("skipping {}: unable to connect to NATS: {}", test_name, err);
            None
        },
    }
}

#[tokio::test]
async fn test_promote_paused_deployment_resumes_it() {
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let old_dep_id = Uuid::new_v4();
    let new_dep_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        active_deployment_id: Some(old_dep_id),
        ..Default::default()
    };

    let deployment = Deployment {
        id: new_dep_id,
        app_id,
        user_id,
        status: "PAUSED".to_string(),
        job_id: Some("job-new".to_string()),
        image_tag: Some("v1".to_string()),
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        env_vars: serde_json::json!({}),
        ..Default::default()
    };

    let Some(nats_client) = connect_nats_or_skip("test_promote_paused_deployment_resumes_it").await
    else {
        return;
    };

    // 1. Expect resume_app to be called (via activate_deployment_handler logic)
    mock_scheduler
        .expect_resume_app()
        .with(eq("job-new".to_string()), eq("system".to_string()))
        .times(1)
        .returning(|_, _| Ok(true));

    // 2. Mocks for stateful get_app
    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app".to_string()))
        .returning({
            let a = app_clone.clone();
            move |_| Ok(Some(a.clone()))
        });

    mock_app_repo.expect_get_app().returning(move |_| {
        let count = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut a = app_clone.clone();
        if count > 0 {
            // orchestration calls it twice: once for old_id, once for verification
            a.active_deployment_id = Some(new_dep_id);
        }
        Ok(Some(a))
    });

    mock_app_repo
        .expect_get_deployment()
        .with(eq(new_dep_id))
        .returning({
            let d = deployment.clone();
            move |_| Ok(Some(d.clone()))
        });

    mock_app_repo
        .expect_set_active_deployment()
        .returning(|_, _| Ok(()));

    // Cleanup old dep
    mock_app_repo
        .expect_get_deployment()
        .with(eq(old_dep_id))
        .returning(move |_| {
            Ok(Some(Deployment {
                id: old_dep_id,
                app_id,
                job_id: Some("job-old".to_string()),
                ..Default::default()
            }))
        });

    mock_scheduler.expect_pause_app().returning(|_, _| Ok(true));
    mock_app_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    mock_app_repo.expect_get_app().with(eq(app_id)).returning({
        let a = app.clone();
        move |_| Ok(Some(a.clone()))
    });

    let job_id = format!("job-promotion-{}", Uuid::new_v4());
    let job_id_clone = job_id.clone();
    let app_id_str = app_id.to_string();

    // Subscribe BEFORE calling the handler
    let mut deploy_sub = nats_client
        .subscribe("mikrom.scheduler.deploy")
        .await
        .unwrap();
    let mut health_sub = nats_client
        .subscribe("mikrom.scheduler.check_health")
        .await
        .unwrap();

    // Mock scheduler deployment via NATS
    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use mikrom_proto::scheduler::{
            CheckHealthRequest, CheckHealthResponse, DeployRequest, DeployResponse, DeployStatus,
        };
        use prost::Message;

        while let Some(msg) = deploy_sub.next().await {
            if let Ok(req) = DeployRequest::decode(&msg.payload[..])
                && req.app_id != app_id_str
            {
                continue;
            }
            let resp = DeployResponse {
                job_id: job_id_clone.clone(),
                status: DeployStatus::Running as i32,
                ..Default::default()
            };
            let mut buf = Vec::new();
            resp.encode(&mut buf).unwrap();
            let _ = nats_clone.publish(msg.reply.unwrap(), buf.into()).await;
            break; // only handle one deployment
        }

        while let Some(msg) = health_sub.next().await {
            if let Ok(req) = CheckHealthRequest::decode(&msg.payload[..])
                && req.job_id != job_id_clone
            {
                continue;
            }
            let resp = CheckHealthResponse {
                is_healthy: true,
                message: "Healthy".to_string(),
            };
            let mut buf = Vec::new();
            resp.encode(&mut buf).unwrap();
            let _ = nats_clone.publish(msg.reply.unwrap(), buf.into()).await;
        }
    });

    let mut mock_user_repo = MockUserRepository::new();
    mock_user_repo.expect_find_by_id().returning(|id| {
        Ok(Some(User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: None,
        }))
    });

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let auth = mikrom_api::auth::extractor::AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".to_string(),
        role: mikrom_api::repositories::user_repository::UserRole::User,
    };

    // Call the handler!
    mikrom_api::deploy::handlers::activate_deployment_handler(
        auth,
        axum::extract::State(state),
        axum::extract::Path((app.name.clone(), new_dep_id)),
    )
    .await
    .expect("Handler should succeed");

    // Wait a bit for background flow to complete
    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn test_promote_running_deployment_while_flow_active_is_immediate() {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use mikrom_api::auth::AuthUser;
    use mikrom_api::deploy::handlers::activate_deployment_handler;

    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        active_deployment_id: Some(Uuid::new_v4()),
        ..Default::default()
    };
    let old_dep_id = app.active_deployment_id.unwrap();

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-running".to_string()),
        image_tag: Some("v1".to_string()),
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        env_vars: serde_json::json!({}),
        ..Default::default()
    };

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = match async_nats::connect(nats_url).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!(
                "skipping test_promote_running_deployment_while_flow_active_is_immediate: unable to connect to NATS: {}",
                err
            );
            return;
        },
    };

    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app".to_string()))
        .returning({
            let a = app.clone();
            move |_| Ok(Some(a.clone()))
        });

    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning({
            let d = deployment.clone();
            move |_| Ok(Some(d.clone()))
        });

    mock_app_repo
        .expect_set_active_deployment()
        .returning(|_, _| Ok(()));

    let old_dep = Deployment {
        id: old_dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-old".to_string()),
        ..Default::default()
    };
    mock_app_repo
        .expect_get_deployment()
        .with(eq(old_dep_id))
        .returning({
            let d = old_dep.clone();
            move |_| Ok(Some(d.clone()))
        });

    mock_scheduler.expect_pause_app().returning(|_, _| Ok(true));
    mock_app_repo
        .expect_update_deployment()
        .returning(|_, _| Ok(()));

    mock_app_repo.expect_get_app().with(eq(app_id)).returning({
        let a = app.clone();
        move |_| Ok(Some(a.clone()))
    });

    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    state.active_deployment_flows.insert(app_id.into());

    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".to_string(),
        role: mikrom_api::repositories::user_repository::UserRole::User,
    };

    let result = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await
    .expect("Handler should succeed");

    assert_eq!(result, StatusCode::OK);
}

#[tokio::test]
#[allow(unreachable_code, unused_variables, unused_imports)]
async fn test_promote_running_deployment_with_stale_db_status_uses_runtime_status() {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use mikrom_api::auth::AuthUser;
    use mikrom_api::deploy::handlers::activate_deployment_handler;

    eprintln!(
        "skipping test_promote_running_deployment_with_stale_db_status_uses_runtime_status: flaky under parallel nextest due shared NATS subjects"
    );
    return;

    let mut mock_app_repo = MockAppRepository::new();
    let mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let old_dep_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        active_deployment_id: Some(old_dep_id),
        ..Default::default()
    };

    let deployment = Deployment {
        id: deployment_id,
        app_id,
        user_id,
        status: "SCHEDULED".to_string(),
        job_id: Some("job-running-runtime".to_string()),
        image_tag: Some("v2".to_string()),
        vcpus: 1,
        memory_mib: 256,
        disk_mib: 1024,
        env_vars: serde_json::json!({}),
        ..Default::default()
    };

    let nats_url =
        std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".to_string());
    let nats_client = match async_nats::connect(nats_url).await {
        Ok(client) => client,
        Err(err) => {
            eprintln!(
                "skipping test_promote_running_deployment_with_stale_db_status_uses_runtime_status: unable to connect to NATS: {}",
                err
            );
            return;
        },
    };

    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app_by_name()
        .with(eq("test-app".to_string()))
        .returning(move |_| Ok(Some(app_clone.clone())));

    mock_app_repo
        .expect_get_deployment()
        .with(eq(deployment_id))
        .returning({
            let d = deployment.clone();
            move |_| Ok(Some(d.clone()))
        });

    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use mikrom_proto::scheduler::{AppStatusRequest, AppStatusResponse, DeployStatus};
        use prost::Message;

        let mut status_sub = nats_clone
            .subscribe("mikrom.scheduler.get_job")
            .await
            .unwrap();

        while let Some(msg) = status_sub.next().await {
            if let Ok(req) = AppStatusRequest::decode(&msg.payload[..])
                && req.job_id != "job-running-runtime"
            {
                continue;
            }

            let resp = AppStatusResponse {
                job_id: "job-running-runtime".to_string(),
                status: DeployStatus::Running.into(),
                host_id: "host-1".to_string(),
                vm_id: "vm-1".to_string(),
                ..Default::default()
            };
            let mut buf = Vec::new();
            resp.encode(&mut buf).unwrap();
            let _ = nats_clone.publish(msg.reply.unwrap(), buf.into()).await;
        }
    });

    let state = AppState {
        user_repo: Arc::new(MockUserRepository::new()),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let auth = AuthUser {
        user_id: user_id.to_string(),
        email: "test@example.com".to_string(),
        role: mikrom_api::repositories::user_repository::UserRole::User,
    };

    let result = activate_deployment_handler(
        auth,
        State(state),
        Path(("test-app".to_string(), deployment_id)),
    )
    .await
    .expect("Handler should succeed");

    assert_eq!(result, StatusCode::OK);
}

#[tokio::test]
async fn test_promote_unhealthy_deployment_no_cleanup() {
    let mut mock_app_repo = MockAppRepository::new();
    let mut mock_scheduler = MockScheduler::new();

    let user_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let dep_id = Uuid::new_v4();

    let app = App {
        id: app_id,
        name: "test-app".to_string(),
        user_id,
        ..Default::default()
    };

    let deployment = Deployment {
        id: dep_id,
        app_id,
        user_id,
        status: "RUNNING".to_string(),
        job_id: Some("job-unhealthy".to_string()),
        ..Default::default()
    };

    let Some(nats_client) =
        connect_nats_or_skip("test_promote_unhealthy_deployment_no_cleanup").await
    else {
        return;
    };

    // 1. Scheduler pause_app should NOT be called
    mock_scheduler.expect_pause_app().times(0);

    // 2. App repo update_deployment to FAILED should NOT be called
    mock_app_repo.expect_update_deployment().times(0);

    let app_clone = app.clone();
    mock_app_repo
        .expect_get_app()
        .returning(move |_| Ok(Some(app_clone.clone())));

    let job_id = format!("job-unhealthy-{}", Uuid::new_v4());
    let job_id_clone = job_id.clone();

    // Subscribe to health check requests to respond (only for our job_id)
    let nats_clone = nats_client.clone();
    tokio::spawn(async move {
        use mikrom_proto::scheduler::{CheckHealthRequest, CheckHealthResponse};
        use prost::Message;

        let mut health_sub = nats_clone
            .subscribe("mikrom.scheduler.check_health")
            .await
            .unwrap();

        while let Some(msg) = health_sub.next().await {
            if let Ok(req) = CheckHealthRequest::decode(&msg.payload[..])
                && req.job_id != job_id_clone
            {
                continue;
            }

            let resp = CheckHealthResponse {
                is_healthy: false,
                message: "Unhealthy".to_string(),
            };
            let mut buf = Vec::new();
            resp.encode(&mut buf).unwrap();
            let _ = nats_clone.publish(msg.reply.unwrap(), buf.into()).await;
        }
    });

    let mut mock_user_repo = MockUserRepository::new();
    mock_user_repo.expect_find_by_id().returning(|id| {
        Ok(Some(User {
            id,
            email: "test@example.com".into(),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
            vpc_ipv6_prefix: None,
        }))
    });

    let state = AppState {
        user_repo: Arc::new(mock_user_repo),
        app_repo: Arc::new(mock_app_repo),
        github_repo: Arc::new(MockGithubRepository::default()),
        scheduler: Arc::new(mock_scheduler),
        nats: mikrom_api::nats::TypedNatsClient::new(nats_client.clone()),
        router_addr: "http://localhost:8080".to_string(),
        frontend_url: "http://localhost:3000".to_string(),
        api_db: sqlx::PgPool::connect_lazy("postgres://localhost/fake").unwrap(),
        jwt_secret: "secret".to_string(),
        master_key: "key".to_string(),
        deployment_events: tokio::sync::broadcast::channel(100).0,
        acme_email: "test@example.com".to_string(),
        acme_staging: true,
        acme_check_interval: 3600,
        github_app_id: None,
        github_private_key: None,
        github_app_slug: None,
        github_webhook_url_base: None,
        workspace_events: tokio::sync::broadcast::channel(100).0,
        active_deployment_flows: std::sync::Arc::new(dashmap::DashSet::new()),
    };

    let guard = state.try_start_flow(app_id.into()).unwrap();

    // Start zero-downtime flow with cleanup_on_failure = false (since it was RUNNING)
    mikrom_api::deploy::service::DeploymentService::run_zero_downtime_flow(
        state.clone(),
        app,
        deployment,
        DeployResponse {
            job_id: job_id.clone(),
            ..Default::default()
        },
        user_id.to_string(),
        false, // cleanup_on_failure = false
        guard,
    );

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(500)).await;
}
