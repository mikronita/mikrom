use mikrom_cli::client::MikromClient;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_client_health() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), None);

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "ok",
            "version": "1.0.0",
            "services": { "API": "ONLINE" }
        })))
        .mount(&server)
        .await;

    let res = client.health().await.unwrap();
    assert_eq!(res.status, "ok");
    assert_eq!(res.services.get("API").unwrap(), "ONLINE");
}

#[tokio::test]
async fn test_client_register() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), None);

    Mock::given(method("POST"))
        .and(path("/auth/register"))
        .and(body_json(
            json!({ "email": "test@example.com", "password": "password" }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "message": "User created",
            "user_id": "user-123"
        })))
        .mount(&server)
        .await;

    let res = client
        .register("test@example.com", "password")
        .await
        .unwrap();
    assert_eq!(res.user_id, "user-123");
}

#[tokio::test]
async fn test_client_login() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), None);

    Mock::given(method("POST"))
        .and(path("/auth/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token": "secret-token"
        })))
        .mount(&server)
        .await;

    let res = client.login("test@example.com", "password").await.unwrap();
    assert_eq!(res.token, "secret-token");
}

#[tokio::test]
async fn test_client_whoami() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("GET"))
        .and(path("/auth/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "user-123",
            "email": "test@example.com",
            "role": "User",
            "first_name": "Test",
            "last_name": "User"
        })))
        .mount(&server)
        .await;

    let res = client.whoami().await.unwrap();
    assert_eq!(res.user_id, "user-123");
    assert_eq!(res.email, "test@example.com");
    assert_eq!(res.role.as_deref(), Some("User"));
}

#[tokio::test]
async fn test_client_update_profile() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("PUT"))
        .and(path("/auth/me"))
        .and(body_json(
            json!({ "first_name": "New", "last_name": "Name" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "user-123",
            "email": "test@example.com",
            "role": "User",
            "first_name": "New",
            "last_name": "Name"
        })))
        .mount(&server)
        .await;

    let res = client
        .update_profile(Some("New".into()), Some("Name".into()))
        .await
        .unwrap();
    assert_eq!(res.first_name.as_deref(), Some("New"));
    assert_eq!(res.last_name.as_deref(), Some("Name"));
}

#[tokio::test]
async fn test_client_create_app() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("POST"))
        .and(path("/apps"))
        .and(header("authorization", "Bearer token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "app-123",
            "name": "test-app",
            "git_url": "https://github.com/test/repo",
            "port": 8080,
            "created_at": "2023-01-01T00:00:00Z"
        })))
        .mount(&server)
        .await;

    let res = client
        .create_app("test-app", "https://github.com/test/repo")
        .await
        .unwrap();
    assert_eq!(res.id, "app-123");
    assert_eq!(res.name, "test-app");
}

#[tokio::test]
async fn test_client_list_apps() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "app-1",
                "name": "app-one",
                "git_url": "url-1",
                "port": 80,
                "created_at": "2023-01-01T00:00:00Z"
            }
        ])))
        .mount(&server)
        .await;

    let res = client.list_apps().await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].name, "app-one");
}

#[tokio::test]
async fn test_client_get_app_secret() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("GET"))
        .and(path("/apps/test-app/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "github_webhook_secret": "real-secret-456"
        })))
        .mount(&server)
        .await;

    let res = client.get_app_secret("test-app").await.unwrap();
    assert_eq!(res, "real-secret-456");
}

#[tokio::test]
async fn test_client_deploy_app() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("POST"))
        .and(path("/apps/test-app/deploy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job_id": "job-123",
            "status": "BUILDING",
            "message": "Started"
        })))
        .mount(&server)
        .await;

    let res = client.deploy_app_version("test-app").await.unwrap();
    assert_eq!(res.job_id.unwrap(), "job-123");
    assert_eq!(res.status, "BUILDING");
}

#[tokio::test]
async fn test_client_get_deployment_status() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), Some("token".into()));

    Mock::given(method("GET"))
        .and(path("/apps/test-app/deployments/job-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job_id": "job-123",
            "status": "RUNNING",
            "host_id": "node-1",
            "vm_id": "vm-1",
            "scheduled_at": 1000,
            "started_at": 2000,
            "stopped_at": 0,
            "error_message": "",
            "cpu_usage": 0.5,
            "ram_used_bytes": 1024
        })))
        .mount(&server)
        .await;

    let res = client
        .get_deployment_status("test-app", "job-123")
        .await
        .unwrap();
    assert_eq!(res.status, "RUNNING");
    assert_eq!(res.host_id, "node-1");
}

#[tokio::test]
async fn test_client_error_handling() {
    let server = MockServer::start().await;
    let client = MikromClient::new(server.uri(), None);

    Mock::given(method("POST"))
        .and(path("/auth/login"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "Invalid credentials"
        })))
        .mount(&server)
        .await;

    let res = client.login("test@example.com", "wrong").await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Invalid credentials"));
}
