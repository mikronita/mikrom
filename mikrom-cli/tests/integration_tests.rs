use mikrom_cli::application::ports::ApiClient;
use mikrom_cli::infrastructure::http::client::ReqwestApiClient;
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_client_health() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), None, None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/health"))
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
    let client = ReqwestApiClient::new(server.uri(), None, None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/auth/register"))
        .and(body_json(
            json!({ "email": "test@example.com", "password": "password" }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "user": {
                "id": "user-123",
                "email": "test@example.com",
                "role": "User",
                "first_name": null,
                "last_name": null,
                "vpc_ipv6_prefix": "fd00:abcd::"
            },
            "token": "secret-token"
        })))
        .mount(&server)
        .await;

    let res = client
        .register("test@example.com", "password")
        .await
        .unwrap();
    assert_eq!(res.user.id, "user-123");
    assert_eq!(res.token, "secret-token");
}

#[tokio::test]
async fn test_client_login() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), None, None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/auth/login"))
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
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/auth/me"))
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
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("PUT"))
        .and(path("/v1/auth/me"))
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
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps"))
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
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/apps"))
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
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/apps/test-app/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "github_webhook_secret": "real-secret-456"
        })))
        .mount(&server)
        .await;

    let res = client.get_app_secret("test-app").await.unwrap();
    assert_eq!(res, Some("real-secret-456".to_string()));
}

#[tokio::test]
async fn test_client_deploy_app() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps/test-app/deploy"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "job_id": "job-123",
            "status": "BUILDING",
            "message": "Started"
        })))
        .mount(&server)
        .await;

    let res = client
        .deploy_app_version("test-app", 1, 512, None)
        .await
        .unwrap();
    assert_eq!(res.job_id.unwrap(), "job-123");
    assert_eq!(res.status, "BUILDING");
}

#[tokio::test]
async fn test_client_activate_deployment() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps/test-app/deployments/dep-123/activate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(null)))
        .mount(&server)
        .await;

    client
        .activate_deployment("test-app", "dep-123")
        .await
        .unwrap();
}

#[tokio::test]
async fn test_client_stop_deployment() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("DELETE"))
        .and(path("/v1/apps/test-app/deployments/job-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true
        })))
        .mount(&server)
        .await;

    let res = client.stop_deployment("test-app", "job-123").await.unwrap();
    assert_eq!(res["success"], true);
}

#[tokio::test]
async fn test_client_pause_deployment() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps/test-app/deployments/job-123/pause"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true
        })))
        .mount(&server)
        .await;

    let res = client
        .pause_deployment("test-app", "job-123")
        .await
        .unwrap();
    assert_eq!(res["success"], true);
}

#[tokio::test]
async fn test_client_resume_deployment() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps/test-app/deployments/job-123/resume"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true
        })))
        .mount(&server)
        .await;

    let res = client
        .resume_deployment("test-app", "job-123")
        .await
        .unwrap();
    assert_eq!(res["success"], true);
}

#[tokio::test]
async fn test_client_list_app_volumes() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/apps/app-123/volumes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "vol-1",
                "name": "data",
                "size_mib": 1024,
                "created_at": "2024-01-01T00:00:00Z",
                "mount_point": "/data",
                "access_mode": 0
            }
        ])))
        .mount(&server)
        .await;

    let res = client.list_volumes("app-123").await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].volume.id, "vol-1");
    assert_eq!(res[0].mount_point, "/data");
    assert_eq!(res[0].access_mode, 0);
}

#[tokio::test]
async fn test_client_list_all_volumes_includes_attachments() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/volumes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "vol-1",
                "name": "data",
                "size_mib": 1024,
                "created_at": "2024-01-01T00:00:00Z",
                "attachments": [
                    {
                        "app_id": "app-1",
                        "app_name": "svc",
                        "mount_point": "/data",
                        "access_mode": 1
                    }
                ]
            }
        ])))
        .mount(&server)
        .await;

    let res = client.list_all_volumes().await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].volume.name, "data");
    assert_eq!(res[0].attachments[0].app_name, "svc");
}

#[tokio::test]
async fn test_client_create_volume_uses_global_endpoint() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/volumes"))
        .and(body_json(json!({
            "name": "data",
            "size_mib": 2048
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "vol-1",
            "name": "data",
            "size_mib": 2048,
            "created_at": "2024-01-01T00:00:00Z"
        })))
        .mount(&server)
        .await;

    let res = client.create_volume("data", 2048).await.unwrap();
    assert_eq!(res.id, "vol-1");
    assert_eq!(res.name, "data");
    assert_eq!(res.size_mib, 2048);
}

#[tokio::test]
async fn test_client_attach_volume_uses_attach_endpoint() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/apps/app-123/volumes/attach"))
        .and(body_json(json!({
            "volume_id": "vol-1",
            "mount_point": "/data",
            "access_mode": 2
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "app_id": "app-123",
            "volume_id": "vol-1",
            "mount_point": "/data",
            "access_mode": 2,
            "created_at": "2024-01-01T00:00:00Z"
        })))
        .mount(&server)
        .await;

    let res = client
        .attach_volume("app-123", "vol-1", "/data", 2)
        .await
        .unwrap();
    assert_eq!(res.app_id, "app-123");
    assert_eq!(res.volume_id, "vol-1");
    assert_eq!(res.access_mode, 2);
}

#[tokio::test]
async fn test_client_detach_volume_uses_detach_endpoint() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("DELETE"))
        .and(path("/v1/apps/app-123/volumes/vol-1/detach"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    client.detach_volume("app-123", "vol-1").await.unwrap();
}

#[tokio::test]
async fn test_client_delete_deployment_record() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("DELETE"))
        .and(path("/v1/apps/test-app/deployments/job-123/delete"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true
        })))
        .mount(&server)
        .await;

    let res = client
        .delete_deployment_record("test-app", "job-123")
        .await
        .unwrap();
    assert_eq!(res["success"], true);
}

#[tokio::test]
async fn test_client_get_deployment_status() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/apps/test-app/deployments/job-123"))
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
            "ram_used_bytes": 1024,
            "ipv6_address": "fd00::1"
        })))
        .mount(&server)
        .await;

    let res = client
        .get_deployment_status("test-app", "job-123")
        .await
        .unwrap();
    assert_eq!(res.status, "RUNNING");
    assert_eq!(res.host_id, "node-1");
    assert_eq!(res.ipv6_address, Some("fd00::1".to_string()));
}

#[tokio::test]
async fn test_client_list_active_deployments_with_ipv6() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/deployments/active"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "job_id": "job-1",
            "app_name": "app-1",
            "image": "nginx",
            "status": "RUNNING",
            "host_id": "node-1",
            "ipv6_address": "fd00::1"
        }])))
        .mount(&server)
        .await;

    let res = client.list_active_deployments().await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].ipv6_address, Some("fd00::1".to_string()));
}

#[tokio::test]
async fn test_client_error_handling() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), None, None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/auth/login"))
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

#[tokio::test]
async fn test_client_error_handling_invalid_error_json() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), None, None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/auth/login"))
        .respond_with(ResponseTemplate::new(500).set_body_string("not json"))
        .mount(&server)
        .await;

    let res = client.login("test@example.com", "wrong").await;
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    // 500 is retryable; on the last attempt the response is returned,
    // and parsing the invalid JSON body yields this message
    assert!(err_msg.contains("Failed to parse error response"));
}

#[tokio::test]
async fn test_client_list_databases() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("GET"))
        .and(path("/v1/databases"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": "db-1",
                "name": "orders",
                "engine": "neon",
                "status": "running",
                "vcpus": 1,
                "memory_mib": 512,
                "disk_mib": 1024,
                "created_at": "2026-01-01T00:00:00Z"
            }
        ])))
        .mount(&server)
        .await;

    let res = client.list_databases().await.unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].name, "orders");
}

#[tokio::test]
async fn test_client_create_database() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/databases"))
        .and(body_json(json!({
            "name": "orders",
            "engine": "neon",
            "postgres_version": 16,
            "vcpus": 2,
            "memory_mib": 1024,
            "disk_mib": 4096,
            "settings": {
                "max_connections": "200"
            }
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "db-1",
            "name": "orders",
            "engine": "neon",
            "postgres_version": 16,
            "status": "pending",
            "vcpus": 2,
            "memory_mib": 1024,
            "disk_mib": 4096,
            "created_at": "2026-01-01T00:00:00Z"
        })))
        .mount(&server)
        .await;

    let res = client
        .create_database(mikrom_cli::domain::models::CreateDatabaseRequest {
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            vcpus: Some(2),
            memory_mib: Some(1024),
            disk_mib: Some(4096),
            settings: Some(std::collections::HashMap::from([(
                "max_connections".to_string(),
                "200".to_string(),
            )])),
        })
        .await
        .unwrap();

    assert_eq!(res.id, "db-1");
    assert_eq!(res.status, "pending");
}

#[tokio::test]
async fn test_client_delete_database() {
    let server = MockServer::start().await;
    let client = ReqwestApiClient::new(server.uri(), Some("token".into()), None).unwrap();

    Mock::given(method("DELETE"))
        .and(path("/v1/databases/db-1"))
        .respond_with(ResponseTemplate::new(204).set_body_string(""))
        .mount(&server)
        .await;

    client.delete_database("db-1").await.unwrap();
}
