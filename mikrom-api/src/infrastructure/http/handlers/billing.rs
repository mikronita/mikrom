use crate::AppState;
use crate::auth::extractor::TenantContext;
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
    State(state): State<AppState>,
) -> ApiResult<Json<crate::application::billing::RedirectResponse>> {
    let url = crate::application::billing::create_billing_portal_link(&state, &tenant_ctx).await?;
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
