use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::json;
use serial_test::serial;
use sha2::Sha256;
use sqlx::Row;
use tower::ServiceExt;

use mikrom_api::create_app;
use mikrom_api::domain::TenantRepository;
use mikrom_api::infrastructure::db::PostgresTenantRepository;
use mikrom_api::test_utils::TestDb;

type HmacSha256 = Hmac<Sha256>;

fn sign_webhook(secret: &str, webhook_id: &str, webhook_timestamp: &str, body: &[u8]) -> String {
    let body = std::str::from_utf8(body).expect("webhook body must be utf-8");
    let message = format!("{webhook_id}.{webhook_timestamp}.{body}");
    let signing_secret = base64::engine::general_purpose::STANDARD.encode(secret.as_bytes());

    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes()).expect("hmac init");
    mac.update(message.as_bytes());

    format!(
        "v1,{}",
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
    )
}

async fn fetch_billing_row(pool: &sqlx::PgPool, tenant_id: uuid::Uuid) -> sqlx::postgres::PgRow {
    sqlx::query(
        "SELECT polar_customer_id, polar_subscription_id, polar_product_id, plan_name, status, amount_cents, currency, current_period_end, cancel_at_period_end FROM tenant_billing WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .expect("billing row should exist")
}

async fn post_polar_webhook(
    app: &axum::Router,
    body: serde_json::Value,
    webhook_secret: &str,
    webhook_id: &str,
) -> (StatusCode, String) {
    let body_bytes = serde_json::to_vec(&body).expect("serialize webhook body");
    let webhook_timestamp = Utc::now().timestamp().to_string();
    let signature = sign_webhook(webhook_secret, webhook_id, &webhook_timestamp, &body_bytes);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/polar")
                .header("content-type", "application/json")
                .header("webhook-id", webhook_id)
                .header("webhook-timestamp", webhook_timestamp)
                .header("webhook-signature", signature)
                .body(Body::from(body_bytes))
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let response_body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");

    (status, String::from_utf8_lossy(&response_body).to_string())
}

#[tokio::test]
#[serial]
#[ignore = "requires a PostgreSQL test database"]
async fn polar_webhook_http_upserts_tenant_billing_row() {
    let Ok(db) = TestDb::try_new().await else {
        eprintln!("Skipping billing webhook test: database unavailable");
        return;
    };
    let pool = db.pool().clone();
    let state = mikrom_api::test_utils::create_test_app_state(pool.clone());
    let app = create_app(state);

    let tenant_repo = PostgresTenantRepository::new(pool.clone());
    let tenant = tenant_repo
        .create(
            "Billing Project".to_string(),
            mikrom_api::domain::Tenant::generate_slug(),
        )
        .await
        .expect("create tenant");
    let webhook_secret = "integration-webhook-secret";
    let created_body = json!({
        "type": "subscription.created",
        "data": {
            "id": "sub_123",
            "amount": 2500,
            "currency": "usd",
            "status": "active",
            "current_period_start": "2026-05-01T00:00:00Z",
            "current_period_end": "2026-06-01T00:00:00Z",
            "trial_end": null,
            "cancel_at_period_end": false,
            "product_id": "prod_123",
            "customer": {
                "id": "cus_123",
                "external_id": tenant.id.to_string()
            },
            "product": {
                "name": "Pro"
            }
        }
    });

    let original_secret = std::env::var("POLAR_WEBHOOK_SECRET").ok();
    unsafe {
        std::env::set_var("POLAR_WEBHOOK_SECRET", webhook_secret);
    }

    let (status, response_body) =
        post_polar_webhook(&app, created_body.clone(), webhook_secret, "wh_123").await;

    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "unexpected response body: {}",
        response_body
    );

    let row = fetch_billing_row(&pool, tenant.id).await;

    assert_eq!(
        row.try_get::<Option<String>, _>("polar_customer_id")
            .expect("polar_customer_id"),
        Some("cus_123".to_string())
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("polar_subscription_id")
            .expect("polar_subscription_id"),
        Some("sub_123".to_string())
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("polar_product_id")
            .expect("polar_product_id"),
        Some("prod_123".to_string())
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("plan_name")
            .expect("plan_name"),
        Some("Pro".to_string())
    );
    assert_eq!(
        row.try_get::<String, _>("status").expect("status"),
        "active"
    );
    assert_eq!(
        row.try_get::<Option<i32>, _>("amount_cents")
            .expect("amount_cents"),
        Some(2500)
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("currency")
            .expect("currency"),
        Some("usd".to_string())
    );
    assert!(
        !row.try_get::<bool, _>("cancel_at_period_end")
            .expect("cancel_at_period_end")
    );
    assert!(
        row.try_get::<Option<chrono::DateTime<Utc>>, _>("current_period_end")
            .expect("current_period_end")
            .is_some()
    );

    let updated_body = json!({
        "type": "subscription.updated",
        "data": {
            "id": "sub_123",
            "amount": 4000,
            "currency": "usd",
            "status": "past_due",
            "current_period_start": "2026-05-01T00:00:00Z",
            "current_period_end": "2026-06-15T00:00:00Z",
            "trial_end": null,
            "cancel_at_period_end": true,
            "product_id": "prod_456",
            "customer": {
                "id": "cus_123",
                "external_id": tenant.id.to_string()
            },
            "product": {
                "name": "Pro Plus"
            }
        }
    });

    let (updated_status, updated_response_body) =
        post_polar_webhook(&app, updated_body.clone(), webhook_secret, "wh_124").await;

    assert_eq!(
        updated_status,
        StatusCode::ACCEPTED,
        "unexpected response body: {}",
        updated_response_body
    );

    let updated_row = fetch_billing_row(&pool, tenant.id).await;
    let row_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tenant_billing WHERE tenant_id = $1")
            .bind(tenant.id)
            .fetch_one(&pool)
            .await
            .expect("row count");

    assert_eq!(row_count, 1);
    assert_eq!(
        updated_row
            .try_get::<Option<String>, _>("polar_subscription_id")
            .expect("polar_subscription_id"),
        Some("sub_123".to_string())
    );
    assert_eq!(
        updated_row
            .try_get::<Option<String>, _>("polar_product_id")
            .expect("polar_product_id"),
        Some("prod_456".to_string())
    );
    assert_eq!(
        updated_row
            .try_get::<Option<String>, _>("plan_name")
            .expect("plan_name"),
        Some("Pro Plus".to_string())
    );
    assert_eq!(
        updated_row.try_get::<String, _>("status").expect("status"),
        "past_due"
    );
    assert_eq!(
        updated_row
            .try_get::<Option<i32>, _>("amount_cents")
            .expect("amount_cents"),
        Some(4000)
    );
    assert!(
        updated_row
            .try_get::<bool, _>("cancel_at_period_end")
            .expect("cancel_at_period_end")
    );

    let (duplicate_status, duplicate_response_body) =
        post_polar_webhook(&app, updated_body, webhook_secret, "wh_125").await;

    assert_eq!(
        duplicate_status,
        StatusCode::ACCEPTED,
        "unexpected response body: {}",
        duplicate_response_body
    );

    let duplicate_row_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tenant_billing WHERE tenant_id = $1")
            .bind(tenant.id)
            .fetch_one(&pool)
            .await
            .expect("duplicate row count");

    assert_eq!(duplicate_row_count, 1);

    let deleted_body = json!({
        "type": "customer.deleted",
        "data": {
            "id": "cus_123",
            "external_id": tenant.id.to_string()
        }
    });

    let (deleted_status, deleted_response_body) =
        post_polar_webhook(&app, deleted_body, webhook_secret, "wh_126").await;

    assert_eq!(
        deleted_status,
        StatusCode::ACCEPTED,
        "unexpected response body: {}",
        deleted_response_body
    );

    let deleted_row = fetch_billing_row(&pool, tenant.id).await;
    let deleted_row_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tenant_billing WHERE tenant_id = $1")
            .bind(tenant.id)
            .fetch_one(&pool)
            .await
            .expect("deleted row count");

    assert_eq!(deleted_row_count, 1);
    assert_eq!(
        deleted_row.try_get::<String, _>("status").expect("status"),
        "none"
    );
    assert!(
        deleted_row
            .try_get::<Option<String>, _>("polar_subscription_id")
            .expect("polar_subscription_id")
            .is_none()
    );
    assert!(
        deleted_row
            .try_get::<Option<String>, _>("plan_name")
            .expect("plan_name")
            .is_none()
    );

    if let Some(secret) = original_secret {
        unsafe {
            std::env::set_var("POLAR_WEBHOOK_SECRET", secret);
        }
    } else {
        unsafe {
            std::env::remove_var("POLAR_WEBHOOK_SECRET");
        }
    }
}
