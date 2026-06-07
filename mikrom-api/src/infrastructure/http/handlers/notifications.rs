use crate::AppState;
use crate::application::notifications::{
    NotificationQuery, list_notifications, mark_all_notifications_read, mark_notification_read,
};
use crate::error::ApiResult;
use crate::infrastructure::auth::extractor::AuthUser;
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use uuid::Uuid;

fn parse_user_id(auth: &AuthUser) -> Result<Uuid, crate::error::ApiError> {
    Uuid::parse_str(&auth.user_id)
        .map_err(|_| crate::error::ApiError::Auth("Invalid user ID in token".into()))
}

#[rovo::rovo]
pub async fn list_user_notifications(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<NotificationQuery>,
) -> ApiResult<Json<crate::application::notifications::NotificationListResponse>> {
    let user_id = parse_user_id(&auth)?;
    let unread_only = query.unread_only.unwrap_or(false);
    let notifications = list_notifications(
        &state.api_db,
        user_id,
        query.limit,
        query.offset,
        unread_only,
    )
    .await
    .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    Ok(Json(notifications))
}

#[rovo::rovo]
pub async fn mark_user_notification_read(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let user_id = parse_user_id(&auth)?;
    let updated = mark_notification_read(&state.api_db, user_id, notification_id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    if !updated {
        return Err(crate::error::ApiError::NotFound(
            "Notification not found".to_string(),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[rovo::rovo]
pub async fn mark_all_user_notifications_read(
    auth: AuthUser,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    let user_id = parse_user_id(&auth)?;
    let _ = mark_all_notifications_read(&state.api_db, user_id)
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
