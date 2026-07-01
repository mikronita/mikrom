use axum::extract::ConnectInfo;
use axum::middleware;
use rovo::Router;
use rovo::routing::put;
use rovo::routing::{delete, get, patch, post};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::infrastructure::http::handlers::{
    auth::{get_profile, login, register, update_profile},
    billing::{
        create_billing_checkout, create_billing_portal, get_billing_summary, list_billing_products,
        polar_webhook, refresh_billing_products, update_billing_checkout_product,
    },
    github::{github_callback, github_install, list_repos},
    notifications::{
        list_user_notifications, mark_all_user_notifications_read, mark_user_notification_read,
    },
    vms::{
        attach_volume_runtime_handler, cancel_migration_handler, create_security_rule_handler,
        delete_deployment_record, delete_security_rule_handler, detach_volume_runtime_handler,
        get_deployment_logs, get_deployment_status, get_mesh_status_handler,
        list_active_deployments, list_security_rules_handler, mesh_status_stream_handler,
        pause_deployment, query_balloon_handler, query_migration_handler, resume_deployment,
        set_balloon_handler, start_migration_handler, stop_deployment, vm_snapshot_create_handler,
        vm_snapshot_delete_handler, vm_snapshot_list_handler, vm_snapshot_restore_handler,
        watch_deployments,
    },
    webhooks::{github_webhook_handler, github_webhook_handler_generic},
};

pub fn create_app_with_rate_limits(
    state: crate::AppState,
    rate_limiter: Arc<crate::rate_limit::RateLimiter>,
) -> axum::Router {
    rate_limiter.start_cleanup_task();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut api = crate::openapi::build_openapi();

    let public_routes = Router::new()
        .route(
            &format!("{}/health", crate::API_V1),
            get(crate::infrastructure::http::health::health),
        )
        .route(
            &format!("{}/health/stream", crate::API_V1),
            get(crate::infrastructure::http::health::health_stream),
        )
        .route(
            "/re-attach",
            post(crate::infrastructure::http::health::re_attach),
        )
        .route(
            &format!("{}/webhooks/polar", crate::API_V1),
            post(polar_webhook),
        )
        .route(
            "/validate",
            post(crate::infrastructure::http::health::validate),
        )
        .finish_api(&mut api);

    let protected_routes = Router::new()
        .route(&format!("{}/auth/register", crate::API_V1), post(register))
        .route(&format!("{}/auth/login", crate::API_V1), post(login))
        .route(
            &format!("{}/webhooks/github/{{app_name}}", crate::API_V1),
            post(github_webhook_handler),
        )
        .route(
            &format!("{}/webhooks/github", crate::API_V1),
            post(github_webhook_handler_generic),
        )
        .route(
            &format!("{}/auth/me", crate::API_V1),
            get(get_profile).put(update_profile),
        )
        .route(
            &format!("{}/billing", crate::API_V1),
            get(get_billing_summary),
        )
        .route(
            &format!("{}/billing/products", crate::API_V1),
            get(list_billing_products),
        )
        .route(
            &format!("{}/billing/products/refresh", crate::API_V1),
            post(refresh_billing_products),
        )
        .route(
            &format!("{}/billing/checkout-product", crate::API_V1),
            put(update_billing_checkout_product),
        )
        .route(
            &format!("{}/billing/checkout", crate::API_V1),
            post(create_billing_checkout),
        )
        .route(
            &format!("{}/billing/portal", crate::API_V1),
            post(create_billing_portal),
        )
        .route(
            &format!("{}/notifications", crate::API_V1),
            get(list_user_notifications),
        )
        .route(
            &format!("{}/notifications/{{notification_id}}/read", crate::API_V1),
            post(mark_user_notification_read),
        )
        .route(
            &format!("{}/notifications/read-all", crate::API_V1),
            post(mark_all_user_notifications_read),
        )
        .route(
            &format!("{}/github/install", crate::API_V1),
            get(github_install),
        )
        .route(
            &format!("{}/github/callback", crate::API_V1),
            get(github_callback),
        )
        .route(&format!("{}/github/repos", crate::API_V1), get(list_repos))
        .route(
            &format!("{}/github/accounts", crate::API_V1),
            get(crate::infrastructure::http::handlers::github::list_accounts),
        )
        .route(
            &format!("{}/projects", crate::API_V1),
            get(crate::infrastructure::http::handlers::projects::list_projects)
                .post(crate::infrastructure::http::handlers::projects::create_project),
        )
        .route(
            &format!("{}/projects/{{tenant_id}}", crate::API_V1),
            get(crate::infrastructure::http::handlers::projects::get_project)
                .patch(crate::infrastructure::http::handlers::projects::update_project)
                .delete(crate::infrastructure::http::handlers::projects::delete_project),
        )
        .route(
            &format!("{}/apps", crate::API_V1),
            post(crate::infrastructure::http::handlers::deploy::create_app_handler)
                .get(crate::infrastructure::http::handlers::deploy::list_apps_handler),
        )
        .route(
            &format!("{}/deploy", crate::API_V1),
            post(crate::infrastructure::http::handlers::deploy::deploy_app),
        )
        .route(
            &format!("{}/apps/{{app_name}}", crate::API_V1),
            get(crate::infrastructure::http::handlers::deploy::get_app_handler)
                .patch(crate::infrastructure::http::handlers::deploy::update_app_handler)
                .delete(crate::infrastructure::http::handlers::deploy::delete_app_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/secret", crate::API_V1),
            get(crate::infrastructure::http::handlers::deploy::get_app_secret_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/scale", crate::API_V1),
            patch(crate::infrastructure::http::handlers::deploy::scale_app_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/deploy", crate::API_V1),
            post(crate::infrastructure::http::handlers::deploy::deploy_app_version_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/deployments", crate::API_V1),
            get(crate::infrastructure::http::handlers::deploy::list_deployments_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/deployments/stream", crate::API_V1),
            get(crate::infrastructure::http::handlers::deploy::deployments_stream_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/logs/stream", crate::API_V1),
            get(crate::infrastructure::http::handlers::vms::app_logs_stream_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/metrics/stream", crate::API_V1),
            get(crate::infrastructure::http::handlers::vms::app_metrics_stream_handler),
        )
        .route(
            &format!("{}/workspace/events", crate::API_V1),
            get(crate::workspace::workspace_events_stream),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{deployment_id}}/activate",
                crate::API_V1
            ),
            post(crate::infrastructure::http::handlers::deploy::activate_deployment_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/deployments/{{job_id}}", crate::API_V1),
            get(get_deployment_status).delete(stop_deployment),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/logs",
                crate::API_V1
            ),
            get(get_deployment_logs),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/pause",
                crate::API_V1
            ),
            post(pause_deployment),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/resume",
                crate::API_V1
            ),
            post(resume_deployment),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/delete",
                crate::API_V1
            ),
            delete(delete_deployment_record),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/snapshot",
                crate::API_V1
            ),
            post(vm_snapshot_create_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/snapshot/{{snapshot_name}}/restore",
                crate::API_V1
            ),
            post(vm_snapshot_restore_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/snapshot/{{snapshot_name}}",
                crate::API_V1
            ),
            delete(vm_snapshot_delete_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/snapshots",
                crate::API_V1
            ),
            get(vm_snapshot_list_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/volumes/attach",
                crate::API_V1
            ),
            post(attach_volume_runtime_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/volumes/detach",
                crate::API_V1
            ),
            post(detach_volume_runtime_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/migration/start",
                crate::API_V1
            ),
            post(start_migration_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/migration/cancel",
                crate::API_V1
            ),
            post(cancel_migration_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/migration/status",
                crate::API_V1
            ),
            get(query_migration_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/deployments/{{job_id}}/balloon",
                crate::API_V1
            ),
            post(set_balloon_handler).get(query_balloon_handler),
        )
        .route(
            &format!("{}/apps/{{app_name}}/security-groups", crate::API_V1),
            get(list_security_rules_handler).post(create_security_rule_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_name}}/security-groups/{{rule_id}}",
                crate::API_V1
            ),
            delete(delete_security_rule_handler),
        )
        .route(
            &format!("{}/networking/mesh", crate::API_V1),
            get(get_mesh_status_handler),
        )
        .route(
            &format!("{}/networking/mesh/stream", crate::API_V1),
            get(mesh_status_stream_handler),
        )
        .route(
            &format!("{}/deployments/active", crate::API_V1),
            get(list_active_deployments),
        )
        .route(
            &format!("{}/deployments/events", crate::API_V1),
            get(watch_deployments),
        )
        .route(
            &format!("{}/apps/{{app_id}}/volumes", crate::API_V1),
            get(crate::application::volumes::list_volumes_handler),
        )
        .route(
            &format!("{}/apps/{{app_id}}/volumes/attach", crate::API_V1),
            post(crate::application::volumes::attach_volume_handler),
        )
        .route(
            &format!(
                "{}/apps/{{app_id}}/volumes/{{volume_id}}/detach",
                crate::API_V1
            ),
            delete(crate::application::volumes::detach_volume_handler),
        )
        .route(
            &format!("{}/volumes", crate::API_V1),
            post(crate::application::volumes::create_volume_handler)
                .get(crate::application::volumes::list_all_volumes_handler),
        )
        .route(
            &format!("{}/volumes/{{volume_id}}/snapshots", crate::API_V1),
            post(crate::application::volumes::create_snapshot_handler)
                .get(crate::application::volumes::list_snapshots_handler),
        )
        .route(
            &format!("{}/volumes/{{volume_id}}/restore", crate::API_V1),
            post(crate::application::volumes::restore_snapshot_handler),
        )
        .route(
            &format!("{}/volumes/{{volume_id}}/clone", crate::API_V1),
            post(crate::application::volumes::clone_volume_handler),
        )
        .route(
            &format!("{}/volumes/{{volume_id}}", crate::API_V1),
            delete(crate::application::volumes::delete_volume_handler),
        )
        .route(
            &format!("{}/snapshots/{{snapshot_id}}", crate::API_V1),
            delete(crate::application::volumes::delete_snapshot_handler),
        )
        .route(
            &format!("{}/databases", crate::API_V1),
            post(crate::infrastructure::http::handlers::database::create_database)
                .get(crate::infrastructure::http::handlers::database::list_databases),
        )
        .route(
            &format!("{}/databases/{{id}}", crate::API_V1),
            delete(crate::infrastructure::http::handlers::database::delete_database),
        )
        .route(
            &format!("{}/databases/{{id}}/connection", crate::API_V1),
            get(crate::infrastructure::http::handlers::database::get_database_connection),
        )
        .route(
            &format!("{}/databases/{{id}}/branches", crate::API_V1),
            get(crate::infrastructure::http::handlers::database::list_database_branches),
        )
        .route(
            &format!("{}/databases/{{id}}/backups", crate::API_V1),
            get(crate::infrastructure::http::handlers::database::get_database_backups),
        )
        .route(
            &format!("{}/databases/{{id}}/backups/snapshots", crate::API_V1),
            get(crate::infrastructure::http::handlers::database::list_database_snapshots)
                .post(crate::infrastructure::http::handlers::database::create_database_snapshot),
        )
        .route(
            &format!("{}/databases/{{id}}/backups/restore", crate::API_V1),
            post(crate::infrastructure::http::handlers::database::restore_database_snapshot),
        )
        .route(
            &format!(
                "{}/databases/{{id}}/backups/snapshots/{{snapshot_name}}",
                crate::API_V1
            ),
            delete(crate::infrastructure::http::handlers::database::delete_database_snapshot),
        )
        .finish_api(&mut api);

    let protected_routes_layered = protected_routes.route_layer(middleware::from_fn_with_state(
        rate_limiter,
        crate::rate_limit::rate_limit_middleware,
    ));

    let oas_router = Router::new()
        .with_oas_route(api, crate::OPENAPI_PATH)
        .with_swagger(crate::SWAGGER_PATH)
        .finish();

    public_routes
        .merge(protected_routes_layered)
        .merge(oas_router)
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let remote_addr = request
                        .extensions()
                        .get::<ConnectInfo<SocketAddr>>()
                        .map(|ci| ci.0.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    tracing::info_span!(
                        "request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                        client_ip = %remote_addr,
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(cors)
}
