use crate::AppState;
use crate::auth::extractor::{AuthUser, TenantContext};
use crate::error::ApiResult;
use axum::{Json, body::Bytes, extract::State, http::HeaderMap, http::StatusCode};

#[rovo::rovo]
pub async fn get_billing_summary(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
) -> ApiResult<Json<crate::application::billing::BillingSummary>> {
    let summary = crate::application::billing::get_billing_summary(&state, &tenant_ctx).await?;
    Ok(Json(summary))
}

#[rovo::rovo]
pub async fn list_billing_products(
    State(state): State<AppState>,
) -> ApiResult<Json<crate::application::billing::BillingProductListResponse>> {
    let products = crate::application::billing::list_billing_products(&state).await?;
    Ok(Json(products))
}

#[rovo::rovo]
pub async fn refresh_billing_products(
    tenant_ctx: TenantContext,
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<crate::application::billing::BillingProductListResponse>> {
    let products =
        crate::application::billing::refresh_billing_products(&state, &tenant_ctx, &auth_user)
            .await?;
    Ok(Json(products))
}

#[rovo::rovo]
pub async fn update_billing_checkout_product(
    tenant_ctx: TenantContext,
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<crate::application::billing::CheckoutProductPreferenceRequest>,
) -> ApiResult<Json<crate::application::billing::BillingSummary>> {
    let summary = crate::application::billing::update_billing_checkout_product(
        &state,
        &tenant_ctx,
        &auth_user,
        payload.product_id,
    )
    .await?;
    Ok(Json(summary))
}

#[rovo::rovo]
pub async fn create_billing_checkout(
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
    Json(payload): Json<crate::application::billing::CheckoutRequest>,
) -> ApiResult<Json<crate::application::billing::RedirectResponse>> {
    let url = crate::application::billing::create_billing_checkout_link(
        &state,
        &tenant_ctx,
        payload.product_id,
    )
    .await?;

    Ok(Json(url))
}

#[rovo::rovo]
pub async fn create_billing_portal(
    tenant_ctx: TenantContext,
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<Json<crate::application::billing::RedirectResponse>> {
    let url =
        crate::application::billing::create_billing_portal_link(&state, &tenant_ctx, &auth_user)
            .await?;
    Ok(Json(url))
}

#[rovo::rovo]
pub async fn polar_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    crate::application::billing::handle_polar_webhook(&state, &headers, &body).await?;
    Ok(StatusCode::ACCEPTED)
}
