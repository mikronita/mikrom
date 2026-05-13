use crate::AppState;
use crate::auth::AuthUser;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use futures::Stream;
use serde::Serialize;
use std::convert::Infallible;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventKind {
    AppCreated,
    AppUpdated,
    AppDeleted,
    DeploymentChanged,
    ProfileUpdated,
    GithubAccountsChanged,
    SecurityRulesChanged,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceEvent {
    pub kind: WorkspaceEventKind,
    pub user_id: Option<Uuid>,
    pub app_id: Option<Uuid>,
    pub app_name: Option<String>,
    pub deployment_id: Option<Uuid>,
    pub resource_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/v1/workspace/events",
    responses(
        (status = 200, description = "SSE stream of workspace events", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse)
    ),
    tag = "system",
    security(
        ("jwt" = [])
    )
)]
pub async fn workspace_events_stream(
    auth: AuthUser,
    State(state): State<AppState>,
) -> crate::error::ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut rx = state.workspace_events.subscribe();
    let auth_user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if (event.user_id.is_none() || event.user_id == Some(auth_user_id))
                        && let Ok(data) = serde_json::to_string(&event)
                    {
                        yield Ok(Event::default().data(data));
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(10))
            .text("keep-alive"),
    ))
}
