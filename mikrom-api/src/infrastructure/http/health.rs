use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use futures::Stream;
use futures::future::join_all;
use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;
use tracing::info;

use crate::error::{ApiResult, SseResponse};

const HEALTH_STREAM_INTERVAL: Duration = Duration::from_secs(15);

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub services: HashMap<String, String>,
}

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct ReAttachResponse {
    pub tenants: Vec<ReAttachResponseTenant>,
}

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct ReAttachResponseTenant {
    pub id: String,
    #[serde(rename = "gen")]
    pub r#gen: u32,
}

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct DeletionValidateResponse {
    pub tenants: Vec<ValidateResponseTenant>,
}

#[derive(serde::Deserialize, Debug, rovo::schemars::JsonSchema)]
pub struct DeletionValidateRequest {
    pub tenants: Vec<ValidateRequestTenant>,
}

#[derive(serde::Deserialize, Debug, rovo::schemars::JsonSchema)]
pub struct ValidateRequestTenant {
    pub id: String,
    #[serde(rename = "gen")]
    pub r#gen: u32,
}

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct ValidateResponseTenant {
    pub id: String,
    pub valid: bool,
}

async fn get_system_health(state: &crate::AppState) -> HashMap<String, String> {
    let mut services = HashMap::new();

    services.insert("API".to_string(), "ONLINE".to_string());

    if state
        .app_repo
        .get_app_by_name("__health_check__")
        .await
        .is_ok()
    {
        services.insert("Database".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Database".to_string(), "OFFLINE".to_string());
    }

    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};
    let nats_req = ListAppsRequest {
        tenant_id: "system".to_string(),
        status: None,
    };

    let scheduler_res: anyhow::Result<ListAppsResponse> = state
        .nats
        .with_timeout(state.nats_request_timeout())
        .request("mikrom.scheduler.list_apps", nats_req)
        .await;

    if scheduler_res.is_ok() {
        services.insert("Scheduler".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Scheduler".to_string(), "OFFLINE".to_string());
    }

    use mikrom_proto::scheduler::{ListWorkersRequest, ListWorkersResponse};
    let agents_req = ListWorkersRequest {};
    let agents_res: anyhow::Result<ListWorkersResponse> = state
        .nats
        .with_timeout(state.nats_request_timeout())
        .request("mikrom.scheduler.list_workers", agents_req)
        .await;

    match agents_res {
        Ok(workers_resp) if !workers_resp.workers.is_empty() => {
            services.insert("Agents".to_string(), "ONLINE".to_string());
        },
        _ => {
            services.insert("Agents".to_string(), "OFFLINE".to_string());
        },
    }

    use mikrom_proto::builder::{GetBuildStatusRequest, GetBuildStatusResponse};
    let builder_req = GetBuildStatusRequest {
        build_id: "health-check".to_string(),
    };
    let builder_res: anyhow::Result<GetBuildStatusResponse> = state
        .nats
        .with_timeout(state.nats_request_timeout())
        .request("mikrom.builder.get_status", builder_req)
        .await;

    if builder_res.is_ok() {
        services.insert("Builder".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Builder".to_string(), "OFFLINE".to_string());
    }

    async fn check_tcp(addr_str: &str) -> bool {
        let clean_addr = crate::service_addr_for_tcp_check(addr_str);

        matches!(
            tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::TcpStream::connect(clean_addr)
            )
            .await,
            Ok(Ok(_))
        )
    }

    if check_tcp(&state.router_addr).await {
        services.insert("Router".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Router".to_string(), "OFFLINE".to_string());
    }

    services
}

#[rovo::rovo]
pub async fn health(State(state): State<crate::AppState>) -> Json<HealthResponse> {
    let services = get_system_health(&state).await;

    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        services,
    })
}

#[rovo::rovo]
pub async fn health_live(State(_state): State<crate::AppState>) -> &'static str {
    "OK"
}

#[rovo::rovo]
pub async fn health_ready(
    State(state): State<crate::AppState>,
) -> Result<Json<HealthResponse>, (axum::http::StatusCode, Json<HealthResponse>)> {
    let services = get_system_health(&state).await;
    let all_online = services.values().all(|s| s == "ONLINE");
    let resp = HealthResponse {
        status: if all_online {
            "ok".to_string()
        } else {
            "degraded".to_string()
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
        services,
    };
    if all_online {
        Ok(Json(resp))
    } else {
        Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(resp)))
    }
}

#[rovo::rovo]
pub async fn re_attach(
    State(state): State<crate::AppState>,
) -> crate::error::ApiResult<Json<ReAttachResponse>> {
    let databases = state
        .ctx
        .database_repo
        .list_databases()
        .await
        .map_err(|e| crate::error::ApiError::Internal(e.to_string()))?;

    let tenants = databases
        .into_iter()
        .filter_map(|database| {
            let tenant_id = database.neon_tenant_id?;
            let timeline_id = database.neon_timeline_id?;

            if database.engine != "neon" {
                return None;
            }

            if matches!(
                database.status,
                crate::domain::DatabaseStatus::Failed | crate::domain::DatabaseStatus::Deleting
            ) {
                return None;
            }

            if tenant_id.starts_with("pending-") || timeline_id.starts_with("pending-") {
                return None;
            }

            Some(ReAttachResponseTenant {
                id: tenant_id,
                r#gen: database.tenant_gen.unwrap_or(1),
            })
        })
        .collect();

    Ok(Json(ReAttachResponse { tenants }))
}

#[rovo::rovo]
pub async fn validate(
    State(state): State<crate::AppState>,
    Json(payload): Json<DeletionValidateRequest>,
) -> Json<DeletionValidateResponse> {
    let validated_items = join_all(payload.tenants.into_iter().map(|tenant| {
        let state = state.clone();

        async move {
            info!(
                tenant_id = %tenant.id,
                generation = tenant.r#gen,
                "[mikrom-api] Pageserver validando retención"
            );

            let valid = crate::application::database::DatabaseService::validate_tenant_retention(
                &state,
                &tenant.id,
                tenant.r#gen,
            )
            .await;

            ValidateResponseTenant {
                id: tenant.id,
                valid,
            }
        }
    }))
    .await;

    Json(DeletionValidateResponse {
        tenants: validated_items,
    })
}

#[rovo::rovo]
pub async fn health_stream(
    State(state): State<crate::AppState>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    let stream = async_stream::stream! {
        let mut interval = tokio::time::interval(HEALTH_STREAM_INTERVAL);
        loop {
            interval.tick().await;
            let services = get_system_health(&state).await;

            let response = HealthResponse {
                status: "ok".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                services,
            };

            if let Ok(data) = serde_json::to_string(&response) {
                yield Ok(Event::default().data(data));
            }
        }
    };

    Ok(SseResponse(
        Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new()),
    ))
}
