use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use futures::Stream;
use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use crate::error::{ApiResult, SseResponse};

#[derive(serde::Serialize, rovo::schemars::JsonSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub services: HashMap<String, String>,
}

async fn get_system_health(state: &crate::AppState) -> HashMap<String, String> {
    let mut services = HashMap::new();

    services.insert("API".to_string(), "ONLINE".to_string());

    use mikrom_proto::scheduler::{ListAppsRequest, ListAppsResponse};
    let nats_req = ListAppsRequest {
        user_id: "system".to_string(),
        status: None,
    };

    let scheduler_res: anyhow::Result<ListAppsResponse> = state
        .nats
        .with_timeout(Duration::from_secs(2))
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
        .with_timeout(Duration::from_secs(2))
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
        .with_timeout(Duration::from_secs(2))
        .request("mikrom.builder.get_status", builder_req)
        .await;

    if builder_res.is_ok() {
        services.insert("Builder".to_string(), "ONLINE".to_string());
    } else {
        services.insert("Builder".to_string(), "OFFLINE".to_string());
    }

    async fn check_tcp(addr_str: &str) -> bool {
        let clean_addr = addr_str
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/');

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
pub async fn health_stream(
    State(state): State<crate::AppState>,
) -> ApiResult<SseResponse<impl Stream<Item = Result<Event, Infallible>>>> {
    let stream = async_stream::stream! {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
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
