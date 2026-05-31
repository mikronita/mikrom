use crate::AppState;
use crate::domain::{NewUser, Tenant, User};
use crate::error::{ApiError, ApiResult};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use uuid::Uuid;

pub struct AuthService;

pub struct RegisterParams {
    pub email: String,
    pub password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

pub struct UpdateProfileParams {
    pub user_id: Uuid,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

pub struct AuthResult {
    pub user: User,
    pub token: String,
}

impl AuthService {
    fn generate_unique_tenant_slug() -> String {
        Tenant::generate_slug()
    }

    pub async fn register(state: &AppState, params: RegisterParams) -> ApiResult<AuthResult> {
        // Check if user already exists
        let count = state
            .user_repo
            .count_by_email(&params.email)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        if count > 0 {
            return Err(ApiError::Conflict("User already exists".into()));
        }

        // Hash password
        let password_hash = crate::infrastructure::crypto::hash_password(&params.password)
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // Create user
        let user_id = state
            .user_repo
            .create(NewUser {
                email: params.email.clone(),
                password_hash,
                role: crate::domain::UserRole::User,
                first_name: params.first_name,
                last_name: params.last_name,
            })
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let user = state
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::Internal("User not found after creation".into()))?;

        // Create default project (tenant), retrying slug generation on collision.
        let tenant = {
            const MAX_TENANT_CREATE_ATTEMPTS: usize = 5;

            let mut last_error = None;
            let mut attempt = 0;
            loop {
                attempt += 1;
                let slug = Self::generate_unique_tenant_slug();
                match state
                    .tenant_repo
                    .create("Default Project".to_string(), slug)
                    .await
                {
                    Ok(tenant) => break tenant,
                    Err(err) if attempt < MAX_TENANT_CREATE_ATTEMPTS => {
                        last_error = Some(err);
                    },
                    Err(err) => {
                        let error = last_error.unwrap_or(err);
                        return Err(ApiError::Internal(error.to_string()));
                    },
                }
            }
        };

        state
            .tenant_repo
            .add_member(tenant.id, user.id, "admin")
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        // Generate JWT
        let token = crate::infrastructure::auth::jwt::create_token(
            &user.id.to_string(),
            &user.email,
            &user.role,
            &state.jwt_secret,
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(AuthResult { user, token })
    }

    pub async fn login(state: &AppState, email: String, password: String) -> ApiResult<AuthResult> {
        let user = state
            .user_repo
            .find_by_email(&email)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::Auth("Invalid credentials".into()))?;

        // Verify password
        if !crate::infrastructure::crypto::verify_password(&password, &user.password_hash)
            .map_err(|_| ApiError::Auth("Invalid credentials".into()))?
        {
            return Err(ApiError::Auth("Invalid credentials".into()));
        }

        // Generate JWT
        let token = crate::infrastructure::auth::jwt::create_token(
            &user.id.to_string(),
            &user.email,
            &user.role,
            &state.jwt_secret,
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(AuthResult { user, token })
    }

    pub async fn get_profile(state: &AppState, user_id: Uuid) -> ApiResult<User> {
        state
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("User not found".into()))
    }

    pub async fn get_profile_by_auth(state: &AppState, auth_user_id: &str) -> ApiResult<User> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;
        Self::get_profile(state, user_id).await
    }

    pub async fn update_profile(state: &AppState, params: UpdateProfileParams) -> ApiResult<User> {
        state
            .user_repo
            .update_profile(params.user_id, params.first_name, params.last_name)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state.publish_workspace_event(WorkspaceEvent {
            kind: WorkspaceEventKind::ProfileUpdated,
            user_id: Some(params.user_id),
            tenant_id: None,
            app_id: None,
            app_name: None,
            deployment_id: None,
            volume_id: None,
            resource_id: None,
        });

        Self::get_profile(state, params.user_id).await
    }

    pub async fn update_profile_by_auth(
        state: &AppState,
        auth_user_id: &str,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> ApiResult<User> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        Self::update_profile(
            state,
            UpdateProfileParams {
                user_id,
                first_name,
                last_name,
            },
        )
        .await
    }
}
