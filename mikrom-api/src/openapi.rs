use crate::auth::handlers::*;
use crate::deploy::handlers::*;
use crate::deploy::webhooks::*;
use crate::error::ErrorResponse;
use crate::models::app::*;
use crate::vms::*;
use utoipa::{
    Modify, OpenApi,
    openapi::security::{Http, HttpAuthScheme, SecurityScheme},
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::health,
        register,
        login,
        get_profile,
        update_profile,
        crate::deploy::deploy_app,
        create_app_handler,
        list_apps_handler,
        delete_app_handler,
        deploy_app_version_handler,
        list_deployments_handler,
        deployments_stream_handler,
        activate_deployment_handler,
        list_active_deployments,
        watch_deployments,
        get_deployment_status,
        get_deployment_logs,
        pause_deployment,
        resume_deployment,
        stop_deployment,
        delete_deployment_record,
        github_webhook_handler
    ),
    components(
        schemas(
            crate::HealthResponse,
            RegisterRequest, RegisterResponse,
            LoginRequest, LoginResponse,
            UpdateProfileRequest, ProfileResponse,
            crate::deploy::VolumeRequest,
            crate::deploy::DeployRequestBody,
            crate::deploy::DeployResponseBody,
            CreateAppRequest, AppResponse,
            ManualDeployRequest,
            App, Deployment,
            LiveDeploymentInfo,
            LiveDeploymentStatus,
            crate::repositories::user_repository::UserRole,
            ErrorResponse
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "apps", description = "Application management"),
        (name = "deployment", description = "Application deployment and lifecycle management"),
        (name = "system", description = "System and health endpoints")
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "jwt",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
}
