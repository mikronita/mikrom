use crate::auth::handlers::*;
use crate::deploy::handlers::*;
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
        list_vms
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
            VmInfo,
            VmStatusResponse,
            crate::repositories::user_repository::UserRole,
            ErrorResponse
        )
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "apps", description = "Application management endpoints"),
        (name = "deployment", description = "Legacy/Direct deployment endpoints"),
        (name = "vms", description = "Virtual Machine status and control"),
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
