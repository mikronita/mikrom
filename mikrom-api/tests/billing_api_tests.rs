use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use serial_test::serial;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use mikrom_api::AppState;
use mikrom_api::auth::jwt::create_token;
use mikrom_api::create_app;
use mikrom_api::domain::TenantRepository;
use mikrom_api::domain::user::{NewUser, UserRepository, UserRole};
use mikrom_api::domain::{
    MockAppRepository, MockDatabaseRepository, MockScheduler, MockUserRepository,
    MockVolumeRepository,
};
use mikrom_api::infrastructure::db::{PostgresTenantRepository, PostgresUserRepository};
use mikrom_api::test_utils::TestDb;

fn set_env(key: &str, value: &str) -> Option<String> {
    let previous = std::env::var(key).ok();
    unsafe {
        std::env::set_var(key, value);
    }
    previous
}

fn restore_env(key: &str, previous: Option<String>) {
    if let Some(value) = previous {
        unsafe {
            std::env::set_var(key, value);
        }
    } else {
        unsafe {
            std::env::remove_var(key);
        }
    }
}

async fn build_billing_state_with_token_role(
    pool: &sqlx::PgPool,
    token_role: UserRole,
) -> (AppState, String, String, String) {
    let user_repo = PostgresUserRepository::new(pool.clone());
    let tenant_repo = PostgresTenantRepository::new(pool.clone());

    let user = user_repo
        .create(NewUser {
            email: format!("billing_test_{}@example.com", Uuid::new_v4()),
            password_hash: "hash".into(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
        })
        .await
        .expect("create user");

    let tenant = tenant_repo
        .create(
            "Billing Project".to_string(),
            mikrom_api::domain::Tenant::generate_slug(),
        )
        .await
        .expect("create tenant");
    tenant_repo
        .add_member(tenant.id, user, "admin")
        .await
        .expect("add member");

    let mut state = mikrom_api::test_utils::create_test_app_state(pool.clone());
    state.user_repo = Arc::new(MockUserRepository::new());
    state.ctx.user_repo = state.user_repo.clone();
    state.tenant_repo = Arc::new(tenant_repo);
    state.ctx.tenant_repo = state.tenant_repo.clone();
    state.app_repo = Arc::new(MockAppRepository::new());
    state.ctx.app_repo = state.app_repo.clone();
    state.database_repo = Arc::new(MockDatabaseRepository::new());
    state.ctx.database_repo = state.database_repo.clone();
    state.volume_repo = Arc::new(MockVolumeRepository::new());
    state.ctx.volume_repo = state.volume_repo.clone();
    state.scheduler = Arc::new(MockScheduler::new());
    state.ctx.scheduler = state.scheduler.clone();

    let token = create_token(
        &user.to_string(),
        "billing@example.com",
        &token_role,
        "billing-secret",
    )
    .expect("create token");

    (state, tenant.tenant_id, tenant.id.to_string(), token)
}

async fn build_billing_state(pool: &sqlx::PgPool) -> (AppState, String, String, String) {
    build_billing_state_with_token_role(pool, UserRole::User).await
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_summary_endpoint_returns_default_snapshot_for_tenant() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, _tenant_id, token) =
        build_billing_state_with_token_role(&pool, UserRole::Admin).await;

    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_product = set_env("POLAR_CHECKOUT_PRODUCT_ID", "prod_default");

    let app = create_app(AppState {
        frontend_url: "http://[::1]:5173".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/billing")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["has_billing_record"], false);
    assert_eq!(payload["status"], "none");
    assert_eq!(payload["default_checkout_product_id"], "prod_default");
    assert!(payload["selected_checkout_product_id"].is_null());
    assert_eq!(payload["tenant_id"], payload["customer_external_id"]);

    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_CHECKOUT_PRODUCT_ID", prev_product);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_checkout_product_can_be_persisted_per_tenant() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, _tenant_id, token) =
        build_billing_state_with_token_role(&pool, UserRole::Admin).await;
    let polar = MockServer::start().await;

    let prev_access_token = set_env("POLAR_ACCESS_TOKEN", "polar-token");
    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_base_url = set_env("POLAR_API_BASE_URL", &polar.uri());
    let prev_product = set_env("POLAR_CHECKOUT_PRODUCT_ID", "prod_default");

    Mock::given(method("POST"))
        .and(path("/checkouts"))
        .and(header("authorization", "Bearer polar-token"))
        .and(body_json(serde_json::json!({
            "products": ["prod_custom"],
            "external_customer_id": _tenant_id.clone(),
            "success_url": "http://localhost:3000/settings?tab=billing&checkout=success",
            "return_url": "http://localhost:3000/settings?tab=billing"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": "https://polar.sh/checkout/session"
        })))
        .mount(&polar)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/customers/external/{_tenant_id}")))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/customers"))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&polar)
        .await;

    let app = create_app(AppState {
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/billing/checkout-product")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"product_id":"prod_custom"}"#))
                .expect("request"),
        )
        .await
        .expect("update response");

    assert_eq!(update_response.status(), StatusCode::OK);
    let update_body = axum::body::to_bytes(update_response.into_body(), 1024 * 1024)
        .await
        .expect("update body");
    let update_json: Value = serde_json::from_slice(&update_body).expect("update json");
    assert_eq!(update_json["selected_checkout_product_id"], "prod_custom");
    assert_eq!(update_json["default_checkout_product_id"], "prod_default");

    let summary_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/billing")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug.clone())
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("summary response");

    assert_eq!(summary_response.status(), StatusCode::OK);
    let summary_body = axum::body::to_bytes(summary_response.into_body(), 1024 * 1024)
        .await
        .expect("summary body");
    let summary_json: Value = serde_json::from_slice(&summary_body).expect("summary json");
    assert_eq!(summary_json["selected_checkout_product_id"], "prod_custom");
    assert!(
        summary_json["has_billing_record"]
            .as_bool()
            .is_some_and(|value| !value)
    );

    let checkout_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/checkout")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("checkout response");

    assert_eq!(checkout_response.status(), StatusCode::OK);
    let checkout_body = axum::body::to_bytes(checkout_response.into_body(), 1024 * 1024)
        .await
        .expect("checkout body");
    let checkout_json: Value = serde_json::from_slice(&checkout_body).expect("checkout json");
    assert_eq!(checkout_json["url"], "https://polar.sh/checkout/session");

    restore_env("POLAR_ACCESS_TOKEN", prev_access_token);
    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_API_BASE_URL", prev_base_url);
    restore_env("POLAR_CHECKOUT_PRODUCT_ID", prev_product);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_checkout_product_update_requires_tenant_admin() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, _tenant_id, token) = build_billing_state(&pool).await;

    let app = create_app(AppState {
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/billing/checkout-product")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"product_id":"prod_custom"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_checkout_and_portal_endpoints_proxy_to_polar() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, tenant_id, token) = build_billing_state(&pool).await;
    let polar = MockServer::start().await;

    let prev_access_token = set_env("POLAR_ACCESS_TOKEN", "polar-token");
    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_base_url = set_env("POLAR_API_BASE_URL", &polar.uri());
    let prev_product = set_env("POLAR_CHECKOUT_PRODUCT_ID", "prod_default");

    Mock::given(method("GET"))
        .and(path(format!("/customers/external/{tenant_id}")))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/customers"))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/checkouts"))
        .and(header("authorization", "Bearer polar-token"))
        .and(body_json(serde_json::json!({
            "products": ["prod_custom"],
            "external_customer_id": tenant_id.clone(),
            "success_url": "http://localhost:5173/settings?tab=billing&checkout=success",
            "return_url": "http://localhost:5173/settings?tab=billing"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": "https://polar.sh/checkout/session"
        })))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/customer-sessions"))
        .and(header("authorization", "Bearer polar-token"))
        .and(body_json(serde_json::json!({
            "external_customer_id": tenant_id.clone(),
            "return_url": "http://localhost:5173/settings?tab=billing"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "customer_portal_url": "https://polar.sh/portal/session"
        })))
        .mount(&polar)
        .await;

    let app = create_app(AppState {
        frontend_url: "http://[::1]:5173".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let checkout_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/checkout")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug.clone())
                .header("content-type", "application/json")
                .body(Body::from(r#"{"product_id":"prod_custom"}"#))
                .expect("request"),
        )
        .await
        .expect("checkout response");

    assert_eq!(checkout_response.status(), StatusCode::OK);
    let checkout_body = axum::body::to_bytes(checkout_response.into_body(), 1024 * 1024)
        .await
        .expect("checkout body");
    let checkout_json: Value = serde_json::from_slice(&checkout_body).expect("checkout json");
    assert_eq!(checkout_json["url"], "https://polar.sh/checkout/session");

    let portal_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/portal")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("portal response");

    let portal_status = portal_response.status();
    let portal_body = axum::body::to_bytes(portal_response.into_body(), 1024 * 1024)
        .await
        .expect("portal body");
    assert_eq!(
        portal_status,
        StatusCode::OK,
        "portal response body: {}",
        String::from_utf8_lossy(&portal_body)
    );
    let portal_json: Value = serde_json::from_slice(&portal_body).expect("portal json");
    assert_eq!(portal_json["url"], "https://polar.sh/portal/session");

    restore_env("POLAR_ACCESS_TOKEN", prev_access_token);
    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_API_BASE_URL", prev_base_url);
    restore_env("POLAR_CHECKOUT_PRODUCT_ID", prev_product);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_products_endpoint_lists_polar_catalog() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, _tenant_id, token) = build_billing_state(&pool).await;
    let polar = MockServer::start().await;

    let prev_access_token = set_env("POLAR_ACCESS_TOKEN", "polar-token");
    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_base_url = set_env("POLAR_API_BASE_URL", &polar.uri());
    let prev_product = set_env("POLAR_CHECKOUT_PRODUCT_ID", "prod_default");

    Mock::given(method("GET"))
        .and(path("/products"))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [
                {
                    "id": "prod_default",
                    "name": "Pro",
                    "description": "Production tier",
                    "price": {
                        "amount": 2500,
                        "currency": "usd"
                    },
                    "recurring_interval": "month",
                    "is_archived": false
                },
                {
                    "id": "prod_archive",
                    "title": "Legacy",
                    "summary": "Legacy plan",
                    "price": {
                        "unit_amount": 1500,
                        "currency": "usd"
                    },
                    "archived": true
                }
            ]
        })))
        .mount(&polar)
        .await;

    let app = create_app(AppState {
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/billing/products")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug.clone())
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(payload["default_checkout_product_id"], "prod_default");
    assert_eq!(
        payload["products"].as_array().map(|items| items.len()),
        Some(2)
    );
    assert_eq!(payload["products"][0]["id"], "prod_default");
    assert_eq!(payload["products"][0]["name"], "Pro");
    assert_eq!(payload["products"][0]["price_amount_cents"], 2500);
    assert_eq!(payload["products"][0]["is_default_checkout_product"], true);
    assert!(payload["last_synced_at"].is_string());

    let cached_polar = MockServer::start().await;
    let prev_cached_base_url = set_env("POLAR_API_BASE_URL", &cached_polar.uri());

    let cached_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/billing/products")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("cached response");

    assert_eq!(cached_response.status(), StatusCode::OK);
    let cached_body = axum::body::to_bytes(cached_response.into_body(), 1024 * 1024)
        .await
        .expect("cached body");
    let cached_payload: Value = serde_json::from_slice(&cached_body).expect("cached json");
    assert_eq!(
        cached_payload["products"]
            .as_array()
            .map(|items| items.len()),
        Some(2)
    );
    assert_eq!(cached_payload["products"][1]["id"], "prod_archive");
    assert!(cached_payload["last_synced_at"].is_string());

    restore_env("POLAR_ACCESS_TOKEN", prev_access_token);
    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_API_BASE_URL", prev_cached_base_url);
    restore_env("POLAR_API_BASE_URL", prev_base_url);
    restore_env("POLAR_CHECKOUT_PRODUCT_ID", prev_product);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_products_refresh_endpoint_syncs_catalog() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, _tenant_id, token) = build_billing_state(&pool).await;
    let polar = MockServer::start().await;

    let prev_access_token = set_env("POLAR_ACCESS_TOKEN", "polar-token");
    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_base_url = set_env("POLAR_API_BASE_URL", &polar.uri());
    let prev_product = set_env("POLAR_CHECKOUT_PRODUCT_ID", "prod_default");

    Mock::given(method("GET"))
        .and(path("/products"))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [
                {
                    "id": "prod_refresh",
                    "name": "Refresh",
                    "description": "Manually refreshed product",
                    "price": {
                        "amount": 7500,
                        "currency": "usd"
                    },
                    "recurring_interval": "month",
                    "is_archived": false
                }
            ]
        })))
        .mount(&polar)
        .await;

    let app = create_app(AppState {
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/products/refresh")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    assert_eq!(payload["default_checkout_product_id"], "prod_default");
    assert_eq!(
        payload["products"].as_array().map(|items| items.len()),
        Some(1)
    );
    assert_eq!(payload["products"][0]["id"], "prod_refresh");
    assert_eq!(payload["products"][0]["name"], "Refresh");
    assert!(payload["last_synced_at"].is_string());

    restore_env("POLAR_ACCESS_TOKEN", prev_access_token);
    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_API_BASE_URL", prev_base_url);
    restore_env("POLAR_CHECKOUT_PRODUCT_ID", prev_product);
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn billing_portal_endpoint_creates_missing_polar_customer_before_session() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing API test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let (state, tenant_slug, tenant_id, token) = build_billing_state(&pool).await;
    let polar = MockServer::start().await;

    let prev_access_token = set_env("POLAR_ACCESS_TOKEN", "polar-token");
    let prev_secret = set_env("POLAR_WEBHOOK_SECRET", "billing-webhook-secret");
    let prev_base_url = set_env("POLAR_API_BASE_URL", &polar.uri());

    Mock::given(method("GET"))
        .and(path(format!("/customers/external/{tenant_id}")))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/customers"))
        .and(header("authorization", "Bearer polar-token"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": "cus_123",
            "external_id": tenant_id.clone(),
            "email": "billing@example.com",
            "email_verified": true,
            "name": null,
            "metadata": {},
            "billing_address": null,
            "tax_id": null,
            "type": "individual",
            "organization_id": "org_123",
            "deleted_at": null,
            "avatar_url": "https://www.gravatar.com/avatar/xxx?d=404",
            "locale": null,
            "created_at": "2026-06-08T00:00:00Z",
            "modified_at": null
        })))
        .mount(&polar)
        .await;

    Mock::given(method("POST"))
        .and(path("/customer-sessions"))
        .and(header("authorization", "Bearer polar-token"))
        .and(body_json(serde_json::json!({
            "external_customer_id": tenant_id.clone(),
            "return_url": "http://localhost:3000/settings?tab=billing"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "customer_portal_url": "https://polar.sh/portal/session"
        })))
        .mount(&polar)
        .await;

    let app = create_app(AppState {
        frontend_url: "http://localhost:3000".to_string(),
        jwt_secret: "billing-secret".to_string(),
        api_db: pool.clone(),
        ..state
    });

    let portal_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/billing/portal")
                .header("Authorization", format!("Bearer {token}"))
                .header("x-mikrom-tenant-id", tenant_slug)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("portal response");

    assert_eq!(portal_response.status(), StatusCode::OK);
    let portal_body = axum::body::to_bytes(portal_response.into_body(), 1024 * 1024)
        .await
        .expect("portal body");
    let portal_json: Value = serde_json::from_slice(&portal_body).expect("portal json");
    assert_eq!(portal_json["url"], "https://polar.sh/portal/session");

    restore_env("POLAR_ACCESS_TOKEN", prev_access_token);
    restore_env("POLAR_WEBHOOK_SECRET", prev_secret);
    restore_env("POLAR_API_BASE_URL", prev_base_url);
}
