use crate::AppState;
use crate::auth::extractor::{AuthUser, TenantContext};
use crate::domain::Tenant;
use crate::error::{ApiError, ApiResult};
use crate::normalize_service_url;
use axum::http::HeaderMap;
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::FromRow;
use std::convert::TryFrom;
use std::env;
use std::sync::OnceLock;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

#[derive(Debug, Clone)]
pub struct PolarSettings {
    pub access_token: String,
    pub webhook_secret: String,
    pub base_url: String,
    pub default_product_id: Option<String>,
}

impl PolarSettings {
    pub fn from_env() -> ApiResult<Self> {
        let access_token = env::var("POLAR_ACCESS_TOKEN")
            .map_err(|_| ApiError::Internal("POLAR_ACCESS_TOKEN is not configured".into()))?;
        let webhook_secret = env::var("POLAR_WEBHOOK_SECRET")
            .map_err(|_| ApiError::Internal("POLAR_WEBHOOK_SECRET is not configured".into()))?;

        let base_url = if let Ok(url) = env::var("POLAR_API_BASE_URL") {
            url
        } else if env::var("POLAR_SERVER").ok().as_deref() == Some("sandbox") {
            "https://sandbox-api.polar.sh/v1".to_string()
        } else {
            "https://api.polar.sh/v1".to_string()
        };

        let default_product_id = env::var("POLAR_CHECKOUT_PRODUCT_ID").ok();

        Ok(Self {
            access_token,
            webhook_secret,
            base_url,
            default_product_id,
        })
    }
}

pub fn validate_polar_environment() -> ApiResult<()> {
    let _ = PolarSettings::from_env()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct BillingRow {
    pub tenant_id: Uuid,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub polar_product_id: Option<String>,
    pub plan_name: Option<String>,
    pub status: String,
    pub amount_cents: Option<i32>,
    pub currency: Option<String>,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct BillingPreferenceRow {
    pub tenant_id: Uuid,
    pub checkout_product_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
struct BillingProductCacheRow {
    pub product_id: String,
    pub name: String,
    pub description: Option<String>,
    pub price_amount_cents: Option<i32>,
    pub currency: Option<String>,
    pub recurring_interval: Option<String>,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub synced_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct BillingSummary {
    pub tenant_id: String,
    pub customer_external_id: String,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub polar_product_id: Option<String>,
    pub plan_name: Option<String>,
    pub status: String,
    pub amount_cents: Option<i32>,
    pub currency: Option<String>,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub default_checkout_product_id: Option<String>,
    pub selected_checkout_product_id: Option<String>,
    pub is_test_mode: bool,
    pub has_billing_record: bool,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CheckoutRequest {
    pub product_id: Option<String>,
}

#[derive(Debug, Deserialize, rovo::schemars::JsonSchema)]
pub struct CheckoutProductPreferenceRequest {
    pub product_id: Option<String>,
}

#[derive(Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct RedirectResponse {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct BillingProduct {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub price_amount_cents: Option<i32>,
    pub currency: Option<String>,
    pub recurring_interval: Option<String>,
    pub is_archived: bool,
    pub is_default_checkout_product: bool,
}

#[derive(Debug, Clone, Serialize, rovo::schemars::JsonSchema)]
pub struct BillingProductListResponse {
    pub products: Vec<BillingProduct>,
    pub default_checkout_product_id: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "data")]
enum PolarWebhookEvent {
    #[serde(rename = "customer.created")]
    CustomerCreated(PolarCustomer),
    #[serde(rename = "customer.updated")]
    CustomerUpdated(PolarCustomer),
    #[serde(rename = "customer.deleted")]
    CustomerDeleted(PolarCustomer),
    #[serde(rename = "customer.state_changed")]
    CustomerStateChanged(PolarCustomerState),
    #[serde(rename = "subscription.created")]
    SubscriptionCreated(PolarSubscription),
    #[serde(rename = "subscription.updated")]
    SubscriptionUpdated(PolarSubscription),
}

#[derive(Debug, Clone, Deserialize)]
struct PolarCustomer {
    id: String,
    external_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PolarCustomerState {
    id: String,
    external_id: Option<String>,
    active_subscriptions: Vec<PolarSubscription>,
}

#[derive(Debug, Clone, Deserialize)]
struct PolarSubscription {
    id: String,
    amount: Option<i32>,
    currency: Option<String>,
    status: Option<String>,
    current_period_start: Option<DateTime<Utc>>,
    current_period_end: Option<DateTime<Utc>>,
    trial_end: Option<DateTime<Utc>>,
    cancel_at_period_end: Option<bool>,
    product_id: Option<String>,
    customer: Option<PolarCustomer>,
    product: Option<PolarProduct>,
}

#[derive(Debug, Clone, Deserialize)]
struct PolarProduct {
    name: Option<String>,
}

fn base64_secret(secret: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(secret.as_bytes())
}

fn signed_payload_message(
    webhook_id: &str,
    webhook_timestamp: &str,
    body: &[u8],
) -> ApiResult<String> {
    let body = std::str::from_utf8(body)
        .map_err(|_| ApiError::BadRequest("Webhook payload must be valid UTF-8".into()))?;
    Ok(format!("{webhook_id}.{webhook_timestamp}.{body}"))
}

fn verify_webhook_signature(headers: &HeaderMap, body: &[u8], secret: &str) -> ApiResult<String> {
    let webhook_id = headers
        .get("webhook-id")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::Auth("Missing webhook-id header".into()))?;
    let webhook_timestamp = headers
        .get("webhook-timestamp")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::Auth("Missing webhook-timestamp header".into()))?;
    let signature_header = headers
        .get("webhook-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::Auth("Missing webhook-signature header".into()))?;

    let timestamp = webhook_timestamp
        .parse::<i64>()
        .map_err(|_| ApiError::Auth("Invalid webhook timestamp".into()))?;
    let now = Utc::now().timestamp();
    if (now - timestamp).abs() > 300 {
        return Err(ApiError::Auth(
            "Webhook timestamp outside allowed window".into(),
        ));
    }

    let message = signed_payload_message(webhook_id, webhook_timestamp, body)?;
    let signing_secret = base64_secret(secret);

    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes())
        .map_err(|_| ApiError::Internal("Failed to initialize webhook verifier".into()))?;
    mac.update(message.as_bytes());

    for signature in signature_header.split_whitespace() {
        let Some(signature_b64) = signature.strip_prefix("v1,") else {
            continue;
        };

        let Ok(signature_bytes) = base64::engine::general_purpose::STANDARD.decode(signature_b64)
        else {
            continue;
        };

        if mac.clone().verify_slice(&signature_bytes).is_ok() {
            return Ok(webhook_id.to_string());
        }
    }

    Err(ApiError::Auth("Invalid webhook signature".into()))
}

async fn load_billing_row(pool: &sqlx::PgPool, tenant_id: Uuid) -> ApiResult<Option<BillingRow>> {
    let row = sqlx::query_as::<_, BillingRow>(
        "SELECT tenant_id, polar_customer_id, polar_subscription_id, polar_product_id, plan_name, status, amount_cents, currency, current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at FROM tenant_billing WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(row)
}

async fn load_billing_preference(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
) -> ApiResult<Option<BillingPreferenceRow>> {
    let row = sqlx::query_as::<_, BillingPreferenceRow>(
        "SELECT tenant_id, checkout_product_id, created_at, updated_at FROM tenant_billing_preferences WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(row)
}

async fn upsert_billing_preference(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    checkout_product_id: Option<&str>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO tenant_billing_preferences (tenant_id, checkout_product_id, created_at, updated_at) VALUES ($1, $2, NOW(), NOW()) ON CONFLICT (tenant_id) DO UPDATE SET checkout_product_id = EXCLUDED.checkout_product_id, updated_at = NOW()",
    )
    .bind(tenant_id)
    .bind(checkout_product_id)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(())
}

async fn ensure_tenant_admin(
    state: &AppState,
    tenant_id: Uuid,
    auth_user: &AuthUser,
) -> ApiResult<()> {
    let user_id = Uuid::parse_str(&auth_user.user_id)
        .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

    let members = state
        .ctx
        .tenant_repo
        .get_members(tenant_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let is_admin = members
        .iter()
        .any(|member| member.user_id == user_id && member.role == "admin");

    if !is_admin {
        return Err(ApiError::Forbidden);
    }

    Ok(())
}

async fn load_cached_billing_products(
    pool: &sqlx::PgPool,
) -> ApiResult<Vec<BillingProductCacheRow>> {
    let rows = sqlx::query_as::<_, BillingProductCacheRow>(
        "SELECT product_id, name, description, price_amount_cents, currency, recurring_interval, is_archived, created_at, updated_at, synced_at FROM polar_billing_products ORDER BY name ASC, product_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(rows)
}

async fn replace_billing_products_cache(
    pool: &sqlx::PgPool,
    products: &[BillingProduct],
) -> ApiResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    sqlx::query("DELETE FROM polar_billing_products")
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    for product in products {
        sqlx::query(
            "INSERT INTO polar_billing_products (product_id, name, description, price_amount_cents, currency, recurring_interval, is_archived, created_at, updated_at, synced_at) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW(), NOW())",
        )
        .bind(&product.id)
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.price_amount_cents)
        .bind(&product.currency)
        .bind(&product.recurring_interval)
        .bind(product.is_archived)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    }

    tx.commit()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(())
}

async fn sync_billing_products(
    state: &AppState,
    settings: &PolarSettings,
) -> ApiResult<Vec<BillingProduct>> {
    let products = list_polar_products(settings).await?;
    replace_billing_products_cache(&state.api_db, &products).await?;
    Ok(products)
}

async fn load_latest_billing_products_sync_at(
    pool: &sqlx::PgPool,
) -> ApiResult<Option<DateTime<Utc>>> {
    let synced_at = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        "SELECT MAX(synced_at) FROM polar_billing_products",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(synced_at)
}

fn billing_products_from_cache(
    rows: Vec<BillingProductCacheRow>,
    default_checkout_product_id: Option<String>,
) -> Vec<BillingProduct> {
    rows.into_iter()
        .map(|row| {
            let product_id = row.product_id;
            let is_default_checkout_product = default_checkout_product_id
                .as_ref()
                .is_some_and(|default_id| default_id == &product_id);

            BillingProduct {
                id: product_id,
                name: row.name,
                description: row.description,
                price_amount_cents: row.price_amount_cents,
                currency: row.currency,
                recurring_interval: row.recurring_interval,
                is_archived: row.is_archived,
                is_default_checkout_product,
            }
        })
        .collect()
}

async fn upsert_billing_row(pool: &sqlx::PgPool, row: &BillingRow) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO tenant_billing (tenant_id, polar_customer_id, polar_subscription_id, polar_product_id, plan_name, status, amount_cents, currency, current_period_start, current_period_end, cancel_at_period_end, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, COALESCE($12, NOW()), NOW()) ON CONFLICT (tenant_id) DO UPDATE SET polar_customer_id = EXCLUDED.polar_customer_id, polar_subscription_id = EXCLUDED.polar_subscription_id, polar_product_id = EXCLUDED.polar_product_id, plan_name = EXCLUDED.plan_name, status = EXCLUDED.status, amount_cents = EXCLUDED.amount_cents, currency = EXCLUDED.currency, current_period_start = EXCLUDED.current_period_start, current_period_end = EXCLUDED.current_period_end, cancel_at_period_end = EXCLUDED.cancel_at_period_end, updated_at = NOW()",
    )
    .bind(row.tenant_id)
    .bind(&row.polar_customer_id)
    .bind(&row.polar_subscription_id)
    .bind(&row.polar_product_id)
    .bind(&row.plan_name)
    .bind(&row.status)
    .bind(row.amount_cents)
    .bind(&row.currency)
    .bind(row.current_period_start)
    .bind(row.current_period_end)
    .bind(row.cancel_at_period_end)
    .bind(row.created_at)
    .execute(pool)
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(())
}

fn row_from_tenant(tenant: &Tenant) -> BillingRow {
    BillingRow {
        tenant_id: tenant.id,
        polar_customer_id: None,
        polar_subscription_id: None,
        polar_product_id: None,
        plan_name: None,
        status: "none".to_string(),
        amount_cents: None,
        currency: None,
        current_period_start: None,
        current_period_end: None,
        cancel_at_period_end: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn billing_summary_from_row(
    row: Option<BillingRow>,
    tenant_id: Uuid,
    default_checkout_product_id: Option<String>,
    selected_checkout_product_id: Option<String>,
    is_test_mode: bool,
) -> BillingSummary {
    if let Some(row) = row {
        BillingSummary {
            tenant_id: tenant_id.to_string(),
            customer_external_id: tenant_id.to_string(),
            polar_customer_id: row.polar_customer_id,
            polar_subscription_id: row.polar_subscription_id,
            polar_product_id: row.polar_product_id,
            plan_name: row.plan_name,
            status: row.status,
            amount_cents: row.amount_cents,
            currency: row.currency,
            current_period_start: row.current_period_start,
            current_period_end: row.current_period_end,
            cancel_at_period_end: row.cancel_at_period_end,
            default_checkout_product_id,
            selected_checkout_product_id,
            is_test_mode,
            has_billing_record: true,
        }
    } else {
        BillingSummary {
            tenant_id: tenant_id.to_string(),
            customer_external_id: tenant_id.to_string(),
            polar_customer_id: None,
            polar_subscription_id: None,
            polar_product_id: None,
            plan_name: None,
            status: "none".to_string(),
            amount_cents: None,
            currency: None,
            current_period_start: None,
            current_period_end: None,
            cancel_at_period_end: false,
            default_checkout_product_id,
            selected_checkout_product_id,
            is_test_mode,
            has_billing_record: false,
        }
    }
}

fn polar_api_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn polar_customer_email_for_tenant(email: &str, tenant_id: Uuid) -> String {
    if let Some((local_part, domain)) = email.split_once('@') {
        format!("{local_part}+mikrom-{tenant_id}@{domain}")
    } else {
        format!("{email}+mikrom-{tenant_id}")
    }
}

async fn create_customer_session(
    settings: &PolarSettings,
    tenant: &Tenant,
    return_url: &str,
) -> ApiResult<String> {
    let response = http_client()
        .post(polar_api_url(&settings.base_url, "/customer-sessions"))
        .bearer_auth(&settings.access_token)
        .json(&serde_json::json!({
            "external_customer_id": tenant.id.to_string(),
            "return_url": return_url,
        }))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to reach Polar: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response>"));

    if !status.is_success() {
        return Err(ApiError::Internal(format!(
            "Polar customer session failed ({status}): {body}"
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| ApiError::Internal(format!("Invalid Polar response: {e}")))?;
    json.get("customer_portal_url")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::Internal("Polar response did not include customer_portal_url".into())
        })
}

async fn customer_exists_in_polar(
    settings: &PolarSettings,
    external_customer_id: &str,
) -> ApiResult<bool> {
    let response = http_client()
        .get(polar_api_url(
            &settings.base_url,
            &format!("/customers/external/{external_customer_id}"),
        ))
        .bearer_auth(&settings.access_token)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to reach Polar: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response>"));

    if status.is_success() {
        return Ok(true);
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }

    Err(ApiError::Internal(format!(
        "Polar customer lookup failed ({status}): {body}"
    )))
}

async fn create_customer_in_polar(
    settings: &PolarSettings,
    external_customer_id: &str,
    email: &str,
) -> ApiResult<()> {
    let response = http_client()
        .post(polar_api_url(&settings.base_url, "/customers"))
        .bearer_auth(&settings.access_token)
        .json(&serde_json::json!({
            "external_id": external_customer_id,
            "email": email,
        }))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to reach Polar: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response>"));

    if status.is_success() {
        return Ok(());
    }

    let already_exists = status == reqwest::StatusCode::CONFLICT
        || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY;

    if already_exists && customer_exists_in_polar(settings, external_customer_id).await? {
        return Ok(());
    }

    Err(ApiError::Internal(format!(
        "Polar customer creation failed ({status}): {body}"
    )))
}

async fn ensure_polar_customer_exists(
    settings: &PolarSettings,
    external_customer_id: &str,
    email: &str,
) -> ApiResult<()> {
    if customer_exists_in_polar(settings, external_customer_id).await? {
        return Ok(());
    }

    create_customer_in_polar(settings, external_customer_id, email).await
}

async fn create_checkout_session(
    settings: &PolarSettings,
    tenant: &Tenant,
    product_id: &str,
    return_url: &str,
    success_url: &str,
) -> ApiResult<String> {
    let response = http_client()
        .post(polar_api_url(&settings.base_url, "/checkouts"))
        .bearer_auth(&settings.access_token)
        .json(&serde_json::json!({
            "products": [product_id],
            "external_customer_id": tenant.id.to_string(),
            "success_url": success_url,
            "return_url": return_url,
        }))
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to reach Polar: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response>"));

    if !status.is_success() {
        let body_lower = body.to_lowercase();
        if body_lower.contains("organization is not ready to accept payments") {
            return Err(ApiError::BadRequest(
                "Polar organization is not ready to accept payments. Complete the Polar payments setup or switch Mikrom to Polar sandbox mode before buying a plan."
                    .into(),
            ));
        }

        return Err(ApiError::Internal(format!(
            "Polar checkout failed ({status}): {body}"
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| ApiError::Internal(format!("Invalid Polar response: {e}")))?;
    json.get("url")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| ApiError::Internal("Polar response did not include checkout url".into()))
}

fn product_entries_from_value(value: serde_json::Value) -> ApiResult<Vec<serde_json::Value>> {
    match value {
        serde_json::Value::Array(items) => Ok(items),
        serde_json::Value::Object(map) => {
            for key in ["items", "data", "results", "products"] {
                if let Some(serde_json::Value::Array(items)) = map.get(key) {
                    return Ok(items.clone());
                }
            }

            Err(ApiError::Internal(
                "Polar response did not include a product list".into(),
            ))
        },
        _ => Err(ApiError::Internal(
            "Polar response did not include a product list".into(),
        )),
    }
}

fn product_id_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| value.get("product_id").and_then(|value| value.as_str()))
        .map(str::to_string)
}

fn product_name_from_value(value: &serde_json::Value, product_id: &str) -> String {
    value
        .get("name")
        .and_then(|value| value.as_str())
        .or_else(|| value.get("title").and_then(|value| value.as_str()))
        .or_else(|| value.get("slug").and_then(|value| value.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| product_id.to_string())
}

fn product_description_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("description")
        .and_then(|value| value.as_str())
        .or_else(|| value.get("summary").and_then(|value| value.as_str()))
        .or_else(|| {
            value
                .get("metadata")
                .and_then(|metadata| metadata.get("description").and_then(|value| value.as_str()))
        })
        .map(str::to_string)
}

fn recurring_interval_from_value(value: &serde_json::Value) -> Option<String> {
    let direct_value = value
        .get("recurring_interval")
        .or_else(|| value.get("interval"))
        .and_then(|value| value.as_str())
        .map(str::to_string);

    if direct_value.is_some() {
        return direct_value;
    }

    let mut stack = vec![value];
    while let Some(current) = stack.pop() {
        if let Some(interval) = current
            .get("recurring_interval")
            .or_else(|| current.get("interval"))
            .and_then(|value| value.as_str())
        {
            return Some(interval.to_string());
        }

        if let Some(object) = current.as_object() {
            stack.extend(object.values());
        }

        if let Some(array) = current.as_array() {
            stack.extend(array.iter());
        }
    }

    None
}

fn first_integer_from_fields(value: &serde_json::Value, fields: &[&str]) -> Option<i32> {
    for field in fields {
        if let Some(number) = value.get(*field) {
            if let Some(integer) = number.as_i64().and_then(|value| i32::try_from(value).ok()) {
                return Some(integer);
            }

            if let Some(integer) = number
                .as_u64()
                .and_then(|value| i64::try_from(value).ok())
                .and_then(|value| i32::try_from(value).ok())
            {
                return Some(integer);
            }

            if let Some(float) = number.as_f64() {
                let rounded = float.round();
                if (float - rounded).abs() < f64::EPSILON {
                    if let Ok(integer) = i32::try_from(rounded as i64) {
                        return Some(integer);
                    }
                }
            }

            if let Some(integer) = number.as_str().and_then(parse_cents_from_string) {
                return Some(integer);
            }
        }
    }

    None
}

fn first_currency_from_fields(value: &serde_json::Value, fields: &[&str]) -> Option<String> {
    for field in fields {
        if let Some(currency) = value.get(*field).and_then(|value| value.as_str()) {
            return Some(currency.to_string());
        }
    }

    None
}

fn parse_cents_from_string(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(integer) = trimmed.parse::<i32>() {
        return Some(integer);
    }

    let mut negative = false;
    let mut digits = trimmed;
    if let Some(rest) = digits.strip_prefix('-') {
        negative = true;
        digits = rest;
    }

    let (whole, fractional) = digits.split_once('.')?;
    let whole = whole.trim();
    let fractional = fractional.trim();
    if whole.is_empty() || fractional.len() > 2 || !whole.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if !fractional.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let whole_value = whole.parse::<i64>().ok()?;
    let fractional_value = match fractional.len() {
        0 => 0_i64,
        1 => fractional.parse::<i64>().ok()? * 10,
        _ => fractional.parse::<i64>().ok()?,
    };
    let cents = whole_value
        .checked_mul(100)?
        .checked_add(fractional_value)?;
    let cents = if negative {
        cents.checked_neg()?
    } else {
        cents
    };
    i32::try_from(cents).ok()
}

fn price_source_candidates<'a>(
    value: &'a serde_json::Value,
) -> Vec<(String, &'a serde_json::Value)> {
    let mut candidates = Vec::new();

    let mut push_candidate = |label: String, candidate: Option<&'a serde_json::Value>| {
        if let Some(candidate) = candidate {
            candidates.push((label, candidate));
        }
    };

    push_candidate("price".to_string(), value.get("price"));
    push_candidate("default_price".to_string(), value.get("default_price"));
    push_candidate("price_data".to_string(), value.get("price_data"));
    push_candidate(
        "default_price_data".to_string(),
        value.get("default_price_data"),
    );
    push_candidate("recurring_price".to_string(), value.get("recurring_price"));
    push_candidate("billing_price".to_string(), value.get("billing_price"));

    if let Some(prices) = value.get("prices").and_then(|value| value.as_array()) {
        candidates.extend(
            prices
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("prices[{index}]"), value)),
        );
    }

    if let Some(prices) = value
        .get("recurring_prices")
        .and_then(|value| value.as_array())
    {
        candidates.extend(
            prices
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("recurring_prices[{index}]"), value)),
        );
    }

    if let Some(prices) = value.get("plan_prices").and_then(|value| value.as_array()) {
        candidates.extend(
            prices
                .iter()
                .enumerate()
                .map(|(index, value)| (format!("plan_prices[{index}]"), value)),
        );
    }

    candidates
}

fn recursive_find_integer_field(value: &serde_json::Value, fields: &[&str]) -> Option<i32> {
    let mut stack = vec![value];
    while let Some(current) = stack.pop() {
        if let Some(amount) = first_integer_from_fields(current, fields) {
            return Some(amount);
        }

        if let Some(object) = current.as_object() {
            stack.extend(object.values());
        }

        if let Some(array) = current.as_array() {
            stack.extend(array.iter());
        }
    }

    None
}

fn recursive_find_currency_field(value: &serde_json::Value, fields: &[&str]) -> Option<String> {
    let mut stack = vec![value];
    while let Some(current) = stack.pop() {
        if let Some(currency) = first_currency_from_fields(current, fields) {
            return Some(currency);
        }

        if let Some(object) = current.as_object() {
            stack.extend(object.values());
        }

        if let Some(array) = current.as_array() {
            stack.extend(array.iter());
        }
    }

    None
}

fn polar_is_test_mode() -> bool {
    matches!(env::var("POLAR_SERVER").ok().as_deref(), Some("sandbox"))
        || env::var("POLAR_API_BASE_URL")
            .ok()
            .is_some_and(|url| url.contains("sandbox-api.polar.sh"))
}

fn price_details_from_value(value: &serde_json::Value) -> (Option<i32>, Option<String>) {
    let direct_price_source = price_source_candidates(value);

    let mut amount = None;
    let mut currency = None;

    for (_source_label, source) in direct_price_source {
        if amount.is_none() {
            amount = first_integer_from_fields(
                source,
                &[
                    "amount",
                    "unit_amount",
                    "unit_amount_decimal",
                    "amount_cents",
                    "price_amount_cents",
                    "price_amount",
                    "price_amount_decimal",
                ],
            );
        }
        if currency.is_none() {
            currency = first_currency_from_fields(
                source,
                &["currency", "currency_code", "price_currency"],
            );
        }
        if amount.is_some() && currency.is_some() {
            return (amount, currency);
        }
    }

    let recursive_amount = recursive_find_integer_field(
        value,
        &[
            "amount",
            "unit_amount",
            "unit_amount_decimal",
            "amount_cents",
            "price_amount_cents",
            "price_amount",
            "price_amount_decimal",
        ],
    );
    let recursive_currency =
        recursive_find_currency_field(value, &["currency", "currency_code", "price_currency"]);
    (amount.or(recursive_amount), currency.or(recursive_currency))
}

fn is_archived_from_value(value: &serde_json::Value) -> bool {
    value
        .get("is_archived")
        .and_then(|value| value.as_bool())
        .or_else(|| value.get("archived").and_then(|value| value.as_bool()))
        .or_else(|| {
            value
                .get("is_active")
                .and_then(|value| value.as_bool())
                .map(|active| !active)
        })
        .unwrap_or(false)
}

fn parse_polar_products(
    raw_products: Vec<serde_json::Value>,
    default_checkout_product_id: Option<String>,
) -> Vec<BillingProduct> {
    raw_products
        .into_iter()
        .filter_map(|product| {
            let id = product_id_from_value(&product)?;
            let (price_amount_cents, currency) = price_details_from_value(&product);
            let is_default_checkout_product = default_checkout_product_id
                .as_ref()
                .is_some_and(|default_id| default_id == &id);

            Some(BillingProduct {
                name: product_name_from_value(&product, &id),
                description: product_description_from_value(&product),
                recurring_interval: recurring_interval_from_value(&product),
                is_archived: is_archived_from_value(&product),
                is_default_checkout_product,
                id,
                price_amount_cents,
                currency,
            })
        })
        .collect()
}

async fn list_polar_products(settings: &PolarSettings) -> ApiResult<Vec<BillingProduct>> {
    let response = http_client()
        .get(polar_api_url(&settings.base_url, "/products"))
        .bearer_auth(&settings.access_token)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to reach Polar: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<unreadable response>"));

    if !status.is_success() {
        return Err(ApiError::Internal(format!(
            "Polar product listing failed ({status}): {body}"
        )));
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| ApiError::Internal(format!("Invalid Polar response: {e}")))?;
    let products = product_entries_from_value(json)?;
    Ok(parse_polar_products(
        products,
        settings.default_product_id.clone(),
    ))
}

fn resolve_subscription_product_name(subscription: &PolarSubscription) -> Option<String> {
    subscription
        .product
        .as_ref()
        .and_then(|product| product.name.clone())
        .or_else(|| subscription.product_id.clone())
}

fn subscription_status(subscription: &PolarSubscription) -> String {
    subscription
        .status
        .clone()
        .unwrap_or_else(|| "active".to_string())
}

fn subscription_row_from_event(
    tenant_id: Uuid,
    existing: Option<BillingRow>,
    customer: Option<&PolarCustomer>,
    subscription: &PolarSubscription,
) -> BillingRow {
    let mut row = existing.unwrap_or_else(|| {
        row_from_tenant(&Tenant {
            id: tenant_id,
            tenant_id: tenant_id.to_string(),
            name: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    });

    row.tenant_id = tenant_id;
    row.polar_customer_id = customer
        .map(|customer| customer.id.clone())
        .or_else(|| row.polar_customer_id.clone());
    row.polar_subscription_id = Some(subscription.id.clone());
    row.polar_product_id = subscription.product_id.clone();
    row.plan_name = resolve_subscription_product_name(subscription).or(row.plan_name);
    row.status = subscription_status(subscription);
    row.amount_cents = subscription.amount;
    row.currency = subscription.currency.clone();
    row.current_period_start = subscription.current_period_start.or(subscription.trial_end);
    row.current_period_end = subscription.current_period_end.or(subscription.trial_end);
    row.cancel_at_period_end = subscription.cancel_at_period_end.unwrap_or(false);
    row.updated_at = Utc::now();
    row
}

fn customer_row_from_event(
    tenant_id: Uuid,
    existing: Option<BillingRow>,
    customer: &PolarCustomer,
    status: &str,
) -> BillingRow {
    let mut row = existing.unwrap_or_else(|| {
        row_from_tenant(&Tenant {
            id: tenant_id,
            tenant_id: tenant_id.to_string(),
            name: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    });

    row.tenant_id = tenant_id;
    row.polar_customer_id = Some(customer.id.clone());
    row.status = status.to_string();
    row.updated_at = Utc::now();
    row
}

fn state_changed_row_from_event(
    tenant_id: Uuid,
    existing: Option<BillingRow>,
    customer_state: &PolarCustomerState,
) -> BillingRow {
    let mut row = existing.unwrap_or_else(|| {
        row_from_tenant(&Tenant {
            id: tenant_id,
            tenant_id: tenant_id.to_string(),
            name: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    });

    row.tenant_id = tenant_id;
    row.polar_customer_id = Some(customer_state.id.clone());

    if let Some(subscription) = customer_state.active_subscriptions.first() {
        row = subscription_row_from_event(
            tenant_id,
            Some(row),
            Some(&PolarCustomer {
                id: customer_state.id.clone(),
                external_id: customer_state.external_id.clone(),
            }),
            subscription,
        );
    } else {
        row.status = "none".to_string();
        row.polar_subscription_id = None;
        row.polar_product_id = None;
        row.plan_name = None;
        row.amount_cents = None;
        row.currency = None;
        row.current_period_start = None;
        row.current_period_end = None;
        row.cancel_at_period_end = false;
        row.updated_at = Utc::now();
    }

    row
}

async fn sync_billing_record_from_event(
    state: &AppState,
    tenant_id: Uuid,
    record: BillingRow,
) -> ApiResult<()> {
    upsert_billing_row(&state.api_db, &record).await?;
    state.publish_workspace_event(crate::workspace::WorkspaceEvent {
        kind: crate::workspace::WorkspaceEventKind::BillingUpdated,
        user_id: None,
        tenant_id: Some(tenant_id),
        app_id: None,
        app_name: None,
        deployment_id: None,
        volume_id: None,
        resource_id: record
            .polar_subscription_id
            .clone()
            .or(record.polar_customer_id.clone()),
    });
    Ok(())
}

pub async fn get_billing_summary(
    state: &AppState,
    tenant_ctx: &TenantContext,
) -> ApiResult<BillingSummary> {
    let default_checkout_product_id = env::var("POLAR_CHECKOUT_PRODUCT_ID").ok();
    let row = load_billing_row(&state.api_db, tenant_ctx.tenant.id).await?;
    let preference = load_billing_preference(&state.api_db, tenant_ctx.tenant.id).await?;
    Ok(billing_summary_from_row(
        row,
        tenant_ctx.tenant.id,
        default_checkout_product_id,
        preference.and_then(|preference| preference.checkout_product_id),
        polar_is_test_mode(),
    ))
}

pub async fn update_billing_checkout_product(
    state: &AppState,
    tenant_ctx: &TenantContext,
    auth_user: &AuthUser,
    product_id: Option<String>,
) -> ApiResult<BillingSummary> {
    ensure_tenant_admin(state, tenant_ctx.tenant.id, auth_user).await?;
    upsert_billing_preference(&state.api_db, tenant_ctx.tenant.id, product_id.as_deref()).await?;
    get_billing_summary(state, tenant_ctx).await
}

pub async fn create_billing_portal_link(
    state: &AppState,
    tenant_ctx: &TenantContext,
    auth_user: &AuthUser,
) -> ApiResult<RedirectResponse> {
    let settings = PolarSettings::from_env()?;
    let customer_email = polar_customer_email_for_tenant(&auth_user.email, tenant_ctx.tenant.id);
    ensure_polar_customer_exists(
        &settings,
        &tenant_ctx.tenant.id.to_string(),
        &customer_email,
    )
    .await?;
    let frontend_url = normalize_service_url(&state.frontend_url);
    let return_url = format!("{frontend_url}/settings?tab=billing");
    let url = create_customer_session(&settings, &tenant_ctx.tenant, &return_url).await?;
    Ok(RedirectResponse { url })
}

pub async fn create_billing_checkout_link(
    state: &AppState,
    tenant_ctx: &TenantContext,
    product_id: Option<String>,
) -> ApiResult<RedirectResponse> {
    let settings = PolarSettings::from_env()?;
    let selected_product_id = (if let Some(product_id) = product_id {
        Some(product_id)
    } else {
        load_billing_preference(&state.api_db, tenant_ctx.tenant.id)
            .await?
            .and_then(|preference| preference.checkout_product_id)
            .or_else(|| settings.default_product_id.clone())
    })
    .ok_or_else(|| {
        ApiError::BadRequest("Missing product_id and POLAR_CHECKOUT_PRODUCT_ID".into())
    })?;
    let frontend_url = normalize_service_url(&state.frontend_url);
    let return_url = format!("{frontend_url}/settings?tab=billing");
    let success_url = format!("{frontend_url}/settings?tab=billing&checkout=success");
    let url = create_checkout_session(
        &settings,
        &tenant_ctx.tenant,
        &selected_product_id,
        &return_url,
        &success_url,
    )
    .await?;

    Ok(RedirectResponse { url })
}

pub async fn list_billing_products(state: &AppState) -> ApiResult<BillingProductListResponse> {
    let settings = PolarSettings::from_env()?;
    let default_checkout_product_id = settings.default_product_id.clone();

    match list_polar_products(&settings).await {
        Ok(products) => {
            replace_billing_products_cache(&state.api_db, &products).await?;
            Ok(BillingProductListResponse {
                products,
                default_checkout_product_id,
                last_synced_at: load_latest_billing_products_sync_at(&state.api_db).await?,
            })
        },
        Err(fetch_error) => {
            let cached_products = load_cached_billing_products(&state.api_db).await?;
            if cached_products.is_empty() {
                return Err(fetch_error);
            }

            Ok(BillingProductListResponse {
                products: billing_products_from_cache(
                    cached_products,
                    default_checkout_product_id.clone(),
                ),
                default_checkout_product_id,
                last_synced_at: load_latest_billing_products_sync_at(&state.api_db).await?,
            })
        },
    }
}

pub async fn refresh_billing_products(
    state: &AppState,
    tenant_ctx: &TenantContext,
    auth_user: &AuthUser,
) -> ApiResult<BillingProductListResponse> {
    ensure_tenant_admin(state, tenant_ctx.tenant.id, auth_user).await?;
    let settings = PolarSettings::from_env()?;
    let products = sync_billing_products(state, &settings).await?;
    Ok(BillingProductListResponse {
        products,
        default_checkout_product_id: settings.default_product_id,
        last_synced_at: load_latest_billing_products_sync_at(&state.api_db).await?,
    })
}

pub async fn handle_polar_webhook(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<()> {
    let webhook_secret = env::var("POLAR_WEBHOOK_SECRET")
        .map_err(|_| ApiError::Internal("POLAR_WEBHOOK_SECRET is not configured".into()))?;
    handle_polar_webhook_with_secret(state, headers, body, &webhook_secret).await
}

async fn handle_polar_webhook_with_secret(
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
    webhook_secret: &str,
) -> ApiResult<()> {
    let webhook_id = verify_webhook_signature(headers, body, webhook_secret)?;
    let event: PolarWebhookEvent = serde_json::from_slice(body)
        .map_err(|e| ApiError::BadRequest(format!("Invalid Polar webhook payload: {e}")))?;

    match event {
        PolarWebhookEvent::CustomerCreated(customer)
        | PolarWebhookEvent::CustomerUpdated(customer) => {
            let Some(external_id) = customer.external_id.as_deref() else {
                return Ok(());
            };
            let tenant_id = Uuid::parse_str(external_id)
                .map_err(|_| ApiError::BadRequest("Invalid Polar external_id".into()))?;
            let existing = load_billing_row(&state.api_db, tenant_id).await?;
            let row = customer_row_from_event(tenant_id, existing, &customer, "none");
            sync_billing_record_from_event(state, tenant_id, row).await?;
        },
        PolarWebhookEvent::CustomerDeleted(customer) => {
            let Some(external_id) = customer.external_id.as_deref() else {
                return Ok(());
            };
            let tenant_id = Uuid::parse_str(external_id)
                .map_err(|_| ApiError::BadRequest("Invalid Polar external_id".into()))?;
            let existing = load_billing_row(&state.api_db, tenant_id).await?;
            let mut row = existing.unwrap_or_else(|| {
                row_from_tenant(&Tenant {
                    id: tenant_id,
                    tenant_id: tenant_id.to_string(),
                    name: String::new(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            });
            row.status = "none".to_string();
            row.polar_customer_id = Some(customer.id);
            row.polar_subscription_id = None;
            row.polar_product_id = None;
            row.plan_name = None;
            row.amount_cents = None;
            row.currency = None;
            row.current_period_start = None;
            row.current_period_end = None;
            row.cancel_at_period_end = false;
            row.updated_at = Utc::now();
            sync_billing_record_from_event(state, tenant_id, row).await?;
        },
        PolarWebhookEvent::CustomerStateChanged(customer_state) => {
            let Some(external_id) = customer_state.external_id.as_deref() else {
                return Ok(());
            };
            let tenant_id = Uuid::parse_str(external_id)
                .map_err(|_| ApiError::BadRequest("Invalid Polar external_id".into()))?;
            let existing = load_billing_row(&state.api_db, tenant_id).await?;
            let row = state_changed_row_from_event(tenant_id, existing, &customer_state);
            sync_billing_record_from_event(state, tenant_id, row).await?;
        },
        PolarWebhookEvent::SubscriptionCreated(subscription)
        | PolarWebhookEvent::SubscriptionUpdated(subscription) => {
            let customer = subscription.customer.as_ref().ok_or_else(|| {
                ApiError::BadRequest("Polar subscription payload missing customer".into())
            })?;
            let Some(external_id) = customer.external_id.as_deref() else {
                return Ok(());
            };
            let tenant_id = Uuid::parse_str(external_id)
                .map_err(|_| ApiError::BadRequest("Invalid Polar external_id".into()))?;
            let existing = load_billing_row(&state.api_db, tenant_id).await?;
            let row =
                subscription_row_from_event(tenant_id, existing, Some(customer), &subscription);
            sync_billing_record_from_event(state, tenant_id, row).await?;
        },
    }

    tracing::debug!(webhook_id = %webhook_id, "Processed Polar webhook");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use chrono::Utc;
    use hmac::{Hmac, Mac};
    use serde_json::json;
    use serial_test::serial;
    use sha2::Sha256;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    type TestHmacSha256 = Hmac<Sha256>;

    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid test datetime")
            .with_timezone(&Utc)
    }

    fn test_tenant() -> Tenant {
        Tenant {
            id: Uuid::new_v4(),
            tenant_id: "acme".to_string(),
            name: "Acme".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn signed_signature_header(
        secret: &str,
        webhook_id: &str,
        webhook_timestamp: &str,
        body: &[u8],
    ) -> String {
        let body = std::str::from_utf8(body).expect("utf8 body");
        let message = format!("{webhook_id}.{webhook_timestamp}.{body}");
        let signing_secret = base64_secret(secret);

        let mut mac =
            TestHmacSha256::new_from_slice(signing_secret.as_bytes()).expect("test hmac init");
        mac.update(message.as_bytes());

        let signature =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        format!("v1,{signature}")
    }

    #[test]
    fn billing_summary_from_row_uses_default_values_without_record() {
        let tenant = test_tenant();
        let summary = billing_summary_from_row(
            None,
            tenant.id,
            Some("prod_123".to_string()),
            Some("prod_selected".to_string()),
            false,
        );

        assert_eq!(summary.tenant_id, tenant.id.to_string());
        assert_eq!(summary.customer_external_id, tenant.id.to_string());
        assert_eq!(summary.status, "none");
        assert_eq!(
            summary.default_checkout_product_id.as_deref(),
            Some("prod_123")
        );
        assert_eq!(
            summary.selected_checkout_product_id.as_deref(),
            Some("prod_selected")
        );
        assert!(!summary.has_billing_record);
    }

    #[test]
    fn billing_summary_from_row_preserves_recorded_subscription_data() {
        let tenant = test_tenant();
        let row = BillingRow {
            tenant_id: tenant.id,
            polar_customer_id: Some("cus_123".to_string()),
            polar_subscription_id: Some("sub_123".to_string()),
            polar_product_id: Some("prod_123".to_string()),
            plan_name: Some("Pro".to_string()),
            status: "active".to_string(),
            amount_cents: Some(2500),
            currency: Some("usd".to_string()),
            current_period_start: Some(dt("2026-05-01T00:00:00Z")),
            current_period_end: Some(dt("2026-06-01T00:00:00Z")),
            cancel_at_period_end: true,
            created_at: dt("2026-05-01T00:00:00Z"),
            updated_at: dt("2026-05-02T00:00:00Z"),
        };

        let summary = billing_summary_from_row(Some(row), tenant.id, None, None, false);

        assert_eq!(summary.polar_customer_id.as_deref(), Some("cus_123"));
        assert_eq!(summary.polar_subscription_id.as_deref(), Some("sub_123"));
        assert_eq!(summary.plan_name.as_deref(), Some("Pro"));
        assert_eq!(summary.status, "active");
        assert_eq!(summary.amount_cents, Some(2500));
        assert_eq!(summary.currency.as_deref(), Some("usd"));
        assert!(summary.has_billing_record);
    }

    #[test]
    fn subscription_state_events_use_the_first_active_subscription() {
        let tenant = test_tenant();
        let subscription = PolarSubscription {
            id: "sub_123".to_string(),
            amount: Some(2500),
            currency: Some("usd".to_string()),
            status: Some("active".to_string()),
            current_period_start: Some(dt("2026-05-01T00:00:00Z")),
            current_period_end: Some(dt("2026-06-01T00:00:00Z")),
            trial_end: None,
            cancel_at_period_end: Some(true),
            product_id: Some("prod_123".to_string()),
            customer: Some(PolarCustomer {
                id: "cus_123".to_string(),
                external_id: Some(tenant.id.to_string()),
            }),
            product: Some(PolarProduct {
                name: Some("Pro".to_string()),
            }),
        };

        let row = state_changed_row_from_event(
            tenant.id,
            None,
            &PolarCustomerState {
                id: "cus_123".to_string(),
                external_id: Some(tenant.id.to_string()),
                active_subscriptions: vec![subscription],
            },
        );

        assert_eq!(row.polar_customer_id.as_deref(), Some("cus_123"));
        assert_eq!(row.polar_subscription_id.as_deref(), Some("sub_123"));
        assert_eq!(row.plan_name.as_deref(), Some("Pro"));
        assert_eq!(row.status, "active");
        assert_eq!(row.amount_cents, Some(2500));
        assert_eq!(row.currency.as_deref(), Some("usd"));
        assert!(row.cancel_at_period_end);
    }

    #[test]
    fn subscription_state_events_clear_plan_when_no_subscriptions_remain() {
        let tenant = test_tenant();
        let row = state_changed_row_from_event(
            tenant.id,
            Some(BillingRow {
                tenant_id: tenant.id,
                polar_customer_id: Some("cus_123".to_string()),
                polar_subscription_id: Some("sub_123".to_string()),
                polar_product_id: Some("prod_123".to_string()),
                plan_name: Some("Pro".to_string()),
                status: "active".to_string(),
                amount_cents: Some(2500),
                currency: Some("usd".to_string()),
                current_period_start: Some(dt("2026-05-01T00:00:00Z")),
                current_period_end: Some(dt("2026-06-01T00:00:00Z")),
                cancel_at_period_end: true,
                created_at: dt("2026-05-01T00:00:00Z"),
                updated_at: dt("2026-05-02T00:00:00Z"),
            }),
            &PolarCustomerState {
                id: "cus_123".to_string(),
                external_id: Some(tenant.id.to_string()),
                active_subscriptions: vec![],
            },
        );

        assert_eq!(row.status, "none");
        assert!(row.polar_subscription_id.is_none());
        assert!(row.polar_product_id.is_none());
        assert!(row.plan_name.is_none());
        assert!(row.amount_cents.is_none());
        assert!(row.currency.is_none());
        assert!(!row.cancel_at_period_end);
    }

    #[tokio::test]
    async fn checkout_and_portal_requests_hit_polar_endpoints() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: Some("prod_default".to_string()),
        };
        let tenant = test_tenant();
        let return_url = "http://localhost:3000/settings?tab=billing";
        let success_url = "http://localhost:3000/settings?tab=billing&checkout=success";

        Mock::given(method("POST"))
            .and(path("/checkouts"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "products": ["prod_checkout"],
                "external_customer_id": tenant.id.to_string(),
                "success_url": success_url,
                "return_url": return_url
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "url": "https://polar.sh/checkout/session"
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/customer-sessions"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "external_customer_id": tenant.id.to_string(),
                "return_url": return_url
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "customer_portal_url": "https://polar.sh/portal/session"
            })))
            .mount(&server)
            .await;

        let checkout_url =
            create_checkout_session(&settings, &tenant, "prod_checkout", return_url, success_url)
                .await
                .expect("checkout url");
        let portal_url = create_customer_session(&settings, &tenant, return_url)
            .await
            .expect("portal url");

        assert_eq!(checkout_url, "https://polar.sh/checkout/session");
        assert_eq!(portal_url, "https://polar.sh/portal/session");
    }

    #[tokio::test]
    async fn checkout_session_surfaces_polar_payment_readiness_error() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: Some("prod_default".to_string()),
        };
        let tenant = test_tenant();
        let return_url = "http://localhost:3000/settings?tab=billing";
        let success_url = "http://localhost:3000/settings?tab=billing&checkout=success";

        Mock::given(method("POST"))
            .and(path("/checkouts"))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_string("Organization is not ready to accept payments"),
            )
            .mount(&server)
            .await;

        let result =
            create_checkout_session(&settings, &tenant, "prod_checkout", return_url, success_url)
                .await;

        assert!(matches!(
            result,
            Err(ApiError::BadRequest(message))
                if message.contains("Polar organization is not ready to accept payments")
        ));
    }

    #[tokio::test]
    async fn portal_customer_is_created_before_session_when_missing() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: None,
        };
        let tenant = test_tenant();
        let email = "owner@example.com";
        let polar_email = polar_customer_email_for_tenant(email, tenant.id);
        let return_url = "http://localhost:3000/settings?tab=billing";

        Mock::given(method("GET"))
            .and(path(format!("/customers/external/{}", tenant.id)))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/customers"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "external_id": tenant.id.to_string(),
                "email": polar_email
            })))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/customer-sessions"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "external_customer_id": tenant.id.to_string(),
                "return_url": return_url
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "customer_portal_url": "https://polar.sh/portal/session"
            })))
            .mount(&server)
            .await;

        ensure_polar_customer_exists(&settings, &tenant.id.to_string(), &polar_email)
            .await
            .expect("customer should be created");

        let portal_url = create_customer_session(&settings, &tenant, return_url)
            .await
            .expect("portal url");

        assert_eq!(portal_url, "https://polar.sh/portal/session");
    }

    #[tokio::test]
    async fn create_customer_in_polar_treats_conflict_as_success_when_customer_now_exists() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: None,
        };
        let tenant = test_tenant();
        let email = "owner@example.com";
        let polar_email = polar_customer_email_for_tenant(email, tenant.id);

        Mock::given(method("POST"))
            .and(path("/customers"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "external_id": tenant.id.to_string(),
                "email": polar_email
            })))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": "already exists"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/customers/external/{}", tenant.id)))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cus_123",
                "external_id": tenant.id.to_string()
            })))
            .mount(&server)
            .await;

        create_customer_in_polar(&settings, &tenant.id.to_string(), &polar_email)
            .await
            .expect("customer should be treated as existing after conflict");
    }

    #[tokio::test]
    async fn create_customer_in_polar_ignores_conflict_body_text_when_lookup_succeeds() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: None,
        };
        let tenant = test_tenant();
        let email = "owner@example.com";
        let polar_email = polar_customer_email_for_tenant(email, tenant.id);

        Mock::given(method("POST"))
            .and(path("/customers"))
            .and(header("authorization", "Bearer polar-token"))
            .and(body_json(json!({
                "external_id": tenant.id.to_string(),
                "email": polar_email
            })))
            .respond_with(ResponseTemplate::new(409).set_body_json(json!({
                "error": "duplicate customer reference"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/customers/external/{}", tenant.id)))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cus_123",
                "external_id": tenant.id.to_string()
            })))
            .mount(&server)
            .await;

        create_customer_in_polar(&settings, &tenant.id.to_string(), &polar_email)
            .await
            .expect("customer should be treated as existing after conflict lookup");
    }

    #[tokio::test]
    async fn list_polar_products_normalizes_array_and_marks_default_product() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: Some("prod_default".to_string()),
        };

        Mock::given(method("GET"))
            .and(path("/products"))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
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
                        "id": "prod_extra",
                        "title": "Add-on",
                        "summary": "Optional add-on",
                        "price": {
                            "unit_amount": 500,
                            "currency": "usd"
                        },
                        "archived": true
                    }
                ]
            })))
            .mount(&server)
            .await;

        let products = list_polar_products(&settings).await.expect("products");

        assert_eq!(products.len(), 2);
        assert_eq!(products[0].id, "prod_default");
        assert_eq!(products[0].name, "Pro");
        assert_eq!(products[0].description.as_deref(), Some("Production tier"));
        assert_eq!(products[0].price_amount_cents, Some(2500));
        assert_eq!(products[0].currency.as_deref(), Some("usd"));
        assert_eq!(products[0].recurring_interval.as_deref(), Some("month"));
        assert!(products[0].is_default_checkout_product);
        assert!(!products[1].is_default_checkout_product);
        assert!(products[1].is_archived);
    }

    #[tokio::test]
    async fn list_polar_products_handles_nested_price_fields() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: Some("prod_nested".to_string()),
        };

        Mock::given(method("GET"))
            .and(path("/products"))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {
                        "id": "prod_nested",
                        "title": "Nested plan",
                        "pricing": {
                            "default_price": {
                                "unit_amount": 9900,
                                "currency_code": "eur"
                            }
                        },
                        "recurring": {
                            "interval": "year"
                        }
                    },
                    {
                        "id": "prod_decimal",
                        "name": "Decimal plan",
                        "price": {
                            "unit_amount_decimal": "49.99",
                            "currency_code": "usd"
                        },
                        "interval": "month"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let products = list_polar_products(&settings).await.expect("products");

        assert_eq!(products.len(), 1);
        assert_eq!(products[0].id, "prod_nested");
        assert_eq!(products[0].name, "Nested plan");
        assert_eq!(products[0].price_amount_cents, Some(9900));
        assert_eq!(products[0].currency.as_deref(), Some("eur"));
        assert_eq!(products[0].recurring_interval.as_deref(), Some("year"));
        assert!(products[0].is_default_checkout_product);

        assert_eq!(products[1].id, "prod_decimal");
        assert_eq!(products[1].name, "Decimal plan");
        assert_eq!(products[1].price_amount_cents, Some(4999));
        assert_eq!(products[1].currency.as_deref(), Some("usd"));
        assert_eq!(products[1].recurring_interval.as_deref(), Some("month"));
    }

    #[tokio::test]
    async fn list_polar_products_uses_first_valid_price_entry() {
        let server = MockServer::start().await;
        let settings = PolarSettings {
            access_token: "polar-token".to_string(),
            webhook_secret: "webhook-secret".to_string(),
            base_url: server.uri(),
            default_product_id: Some("prod_multi_price".to_string()),
        };

        Mock::given(method("GET"))
            .and(path("/products"))
            .and(header("authorization", "Bearer polar-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {
                        "id": "prod_multi_price",
                        "name": "Multi price plan",
                        "prices": [
                            {
                                "currency": "usd"
                            },
                            {
                                "unit_amount": 7500,
                                "currency": "usd"
                            }
                        ]
                    }
                ]
            })))
            .mount(&server)
            .await;

        let products = list_polar_products(&settings).await.expect("products");

        assert_eq!(products.len(), 1);
        assert_eq!(products[0].id, "prod_multi_price");
        assert_eq!(products[0].name, "Multi price plan");
        assert_eq!(products[0].price_amount_cents, Some(7500));
        assert_eq!(products[0].currency.as_deref(), Some("usd"));
        assert!(products[0].is_default_checkout_product);
    }

    #[test]
    #[serial]
    fn validate_polar_environment_rejects_missing_access_token() {
        let original_access_token = std::env::var("POLAR_ACCESS_TOKEN").ok();
        let original_webhook_secret = std::env::var("POLAR_WEBHOOK_SECRET").ok();

        unsafe {
            if let Some(value) = original_webhook_secret.as_ref() {
                std::env::set_var("POLAR_WEBHOOK_SECRET", value);
            } else {
                std::env::set_var("POLAR_WEBHOOK_SECRET", "webhook-secret");
            }
            std::env::remove_var("POLAR_ACCESS_TOKEN");
        }

        let result = validate_polar_environment();

        unsafe {
            match original_access_token {
                Some(value) => std::env::set_var("POLAR_ACCESS_TOKEN", value),
                None => std::env::remove_var("POLAR_ACCESS_TOKEN"),
            }
            match original_webhook_secret {
                Some(value) => std::env::set_var("POLAR_WEBHOOK_SECRET", value),
                None => std::env::remove_var("POLAR_WEBHOOK_SECRET"),
            }
        }

        assert!(matches!(
            result,
            Err(ApiError::Internal(message)) if message.contains("POLAR_ACCESS_TOKEN is not configured")
        ));
    }

    #[test]
    #[serial]
    fn validate_polar_environment_rejects_missing_webhook_secret() {
        let original_access_token = std::env::var("POLAR_ACCESS_TOKEN").ok();
        let original_webhook_secret = std::env::var("POLAR_WEBHOOK_SECRET").ok();

        unsafe {
            if let Some(value) = original_access_token.as_ref() {
                std::env::set_var("POLAR_ACCESS_TOKEN", value);
            } else {
                std::env::set_var("POLAR_ACCESS_TOKEN", "polar-token");
            }
            std::env::remove_var("POLAR_WEBHOOK_SECRET");
        }

        let result = validate_polar_environment();

        unsafe {
            match original_access_token {
                Some(value) => std::env::set_var("POLAR_ACCESS_TOKEN", value),
                None => std::env::remove_var("POLAR_ACCESS_TOKEN"),
            }
            match original_webhook_secret {
                Some(value) => std::env::set_var("POLAR_WEBHOOK_SECRET", value),
                None => std::env::remove_var("POLAR_WEBHOOK_SECRET"),
            }
        }

        assert!(matches!(
            result,
            Err(ApiError::Internal(message)) if message.contains("POLAR_WEBHOOK_SECRET is not configured")
        ));
    }

    #[test]
    #[serial]
    fn validate_polar_environment_rejects_missing_all_required_secrets() {
        let original_access_token = std::env::var("POLAR_ACCESS_TOKEN").ok();
        let original_webhook_secret = std::env::var("POLAR_WEBHOOK_SECRET").ok();

        unsafe {
            std::env::remove_var("POLAR_ACCESS_TOKEN");
            std::env::remove_var("POLAR_WEBHOOK_SECRET");
        }

        let result = validate_polar_environment();

        unsafe {
            match original_access_token {
                Some(value) => std::env::set_var("POLAR_ACCESS_TOKEN", value),
                None => std::env::remove_var("POLAR_ACCESS_TOKEN"),
            }
            match original_webhook_secret {
                Some(value) => std::env::set_var("POLAR_WEBHOOK_SECRET", value),
                None => std::env::remove_var("POLAR_WEBHOOK_SECRET"),
            }
        }

        assert!(matches!(
            result,
            Err(ApiError::Internal(message)) if message.contains("POLAR_ACCESS_TOKEN is not configured")
        ));
    }

    #[test]
    fn webhook_signature_verification_accepts_signed_payload() {
        let body = br#"{"type":"customer.created","data":{"id":"cus_123","external_id":"550e8400-e29b-41d4-a716-446655440000"}}"#;
        let webhook_id = "wh_123";
        let webhook_timestamp = Utc::now().timestamp().to_string();
        let secret = "webhook-secret";

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_str(webhook_id).unwrap());
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&webhook_timestamp).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&signed_signature_header(
                secret,
                webhook_id,
                &webhook_timestamp,
                body,
            ))
            .unwrap(),
        );

        let verified =
            verify_webhook_signature(&headers, body, secret).expect("signature should verify");
        assert_eq!(verified, webhook_id);
    }

    #[test]
    fn webhook_signature_verification_rejects_invalid_signature() {
        let body = br#"{"type":"customer.created","data":{"id":"cus_123","external_id":"550e8400-e29b-41d4-a716-446655440000"}}"#;
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_static("wh_123"));
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&Utc::now().timestamp().to_string()).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_static("v1,bad-signature"),
        );

        let err = verify_webhook_signature(&headers, body, "webhook-secret")
            .expect_err("signature should fail");
        assert!(matches!(err, ApiError::Auth(_)));
    }

    #[test]
    fn webhook_signature_verification_accepts_one_of_multiple_signatures() {
        let body = br#"{"type":"customer.created","data":{"id":"cus_123","external_id":"550e8400-e29b-41d4-a716-446655440000"}}"#;
        let webhook_id = "wh_123";
        let webhook_timestamp = Utc::now().timestamp().to_string();
        let secret = "webhook-secret";

        let valid_signature = signed_signature_header(secret, webhook_id, &webhook_timestamp, body);
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_str(webhook_id).unwrap());
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&webhook_timestamp).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&format!("v1,bad-signature {valid_signature}")).unwrap(),
        );

        let verified = verify_webhook_signature(&headers, body, secret).expect("signature should verify");
        assert_eq!(verified, webhook_id);
    }

    #[tokio::test]
    async fn webhook_handler_rejects_invalid_external_id_before_db_access() {
        let state = AppState::default();
        let body =
            br#"{"type":"customer.created","data":{"id":"cus_123","external_id":"not-a-uuid"}}"#;
        let webhook_id = "wh_123";
        let webhook_timestamp = Utc::now().timestamp().to_string();
        let secret = "webhook-secret";

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_static(webhook_id));
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&webhook_timestamp).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&signed_signature_header(
                secret,
                webhook_id,
                &webhook_timestamp,
                body,
            ))
            .unwrap(),
        );

        let result = handle_polar_webhook_with_secret(&state, &headers, body, secret).await;

        assert!(matches!(result, Err(ApiError::BadRequest(_))));
    }

    #[tokio::test]
    async fn webhook_handler_rejects_subscription_without_customer() {
        let state = AppState::default();
        let body = br#"{"type":"subscription.created","data":{"id":"sub_123","status":"active"}}"#;
        let webhook_id = "wh_123";
        let webhook_timestamp = Utc::now().timestamp().to_string();
        let secret = "webhook-secret";

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_static(webhook_id));
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&webhook_timestamp).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&signed_signature_header(
                secret,
                webhook_id,
                &webhook_timestamp,
                body,
            ))
            .unwrap(),
        );

        let result = handle_polar_webhook_with_secret(&state, &headers, body, secret).await;

        assert!(matches!(
            result,
            Err(ApiError::BadRequest(message)) if message.contains("Polar subscription payload missing customer")
        ));
    }
}
