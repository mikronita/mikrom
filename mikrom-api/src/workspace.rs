use crate::AppState;
use crate::auth::AuthUser;
use crate::auth::extractor::TenantContext;
use crate::error::SseResponse;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use futures::Stream;
use serde::Serialize;
use std::convert::Infallible;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, rovo::schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventKind {
    AppCreated,
    AppUpdated,
    AppDeleted,
    DeploymentChanged,
    ProfileUpdated,
    GithubAccountsChanged,
    BillingUpdated,
    SecurityRulesChanged,
    VolumeChanged,
    SnapshotChanged,
    DatabaseCreated,
    DatabaseUpdated,
    DatabaseDeleted,
    Refresh,
}

#[derive(Clone, Debug, Serialize, rovo::schemars::JsonSchema)]
pub struct WorkspaceEvent {
    pub kind: WorkspaceEventKind,
    pub user_id: Option<Uuid>,
    pub tenant_id: Option<Uuid>,
    pub app_id: Option<Uuid>,
    pub app_name: Option<String>,
    pub deployment_id: Option<Uuid>,
    pub volume_id: Option<Uuid>,
    pub resource_id: Option<String>,
}

#[rovo::rovo]
pub async fn workspace_events_stream(
    auth: AuthUser,
    tenant_ctx: TenantContext,
    State(state): State<AppState>,
) -> crate::error::ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    let mut rx = state.workspace_events.subscribe();
    let auth_user_id = Uuid::parse_str(&auth.user_id)
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;
    let tenant_id = tenant_ctx.tenant.id;

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if (event.tenant_id.is_none() || event.tenant_id == Some(tenant_id))
                        && (event.user_id.is_none() || event.user_id == Some(auth_user_id))
                        && let Ok(data) = serde_json::to_string(&event)
                    {
                        yield Ok(Event::default().data(data));
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let refresh_event = WorkspaceEvent {
                        kind: WorkspaceEventKind::Refresh,
                        user_id: Some(auth_user_id),
                        tenant_id: Some(tenant_id),
                        app_id: None,
                        app_name: None,
                        deployment_id: None,
                        volume_id: None,
                        resource_id: Some("refresh".to_string()),
                    };

                    if let Ok(data) = serde_json::to_string(&refresh_event) {
                        yield Ok(Event::default().data(data));
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Ok(SseResponse(
        Sse::new(stream).keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(10))
                .text("keep-alive"),
        ),
    ))
}
