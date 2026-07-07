use crate::AppState;
use crate::domain::{NewUser, Tenant, User};
use crate::error::{ApiError, ApiResult};
use crate::workspace::{WorkspaceEvent, WorkspaceEventKind};
use totp_rs::{Secret, TOTP};
use uuid::Uuid;

pub struct AuthService;

pub struct RegisterParams {
    pub email: String,
    pub password: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar_url: Option<String>,
}

pub struct UpdateProfileParams {
    pub user_id: Uuid,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub avatar_url: Option<String>,
}

pub struct AuthResult {
    pub user: User,
    pub token: Option<String>,
    pub requires_2fa: bool,
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
                avatar_url: params.avatar_url,
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

        Ok(AuthResult {
            user,
            token: Some(token),
            requires_2fa: false,
        })
    }

    pub async fn login(
        state: &AppState,
        email: String,
        password: String,
        code: Option<String>,
    ) -> ApiResult<AuthResult> {
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

        // Check if TOTP is enabled
        if user.totp_enabled {
            if let Some(code) = code {
                // User provided code, verify it
                let secret = user
                    .totp_secret
                    .clone()
                    .ok_or(ApiError::Internal("User has 2FA enabled but no secret".into()))?;

                let totp = TOTP::new(
                    totp_rs::Algorithm::SHA1,
                    6,
                    1,
                    30,
                    Secret::Encoded(secret).to_bytes().map_err(|e| ApiError::Internal(e.to_string()))?,
                    None,
                    String::new(),
                )
                .map_err(|e| ApiError::Internal(e.to_string()))?;

                let is_valid = totp
                    .check_current(&code)
                    .map_err(|e| ApiError::Internal(e.to_string()))?;

                if !is_valid {
                    return Err(ApiError::Auth("Invalid 2FA code".into()));
                }
            } else {
                // User has not provided code, return requires_2fa: true
                return Ok(AuthResult {
                    user,
                    token: None,
                    requires_2fa: true,
                });
            }
        }

        // Generate JWT
        let token = crate::infrastructure::auth::jwt::create_token(
            &user.id.to_string(),
            &user.email,
            &user.role,
            &state.jwt_secret,
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(AuthResult {
            user,
            token: Some(token),
            requires_2fa: false,
        })
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
            .update_profile(
                params.user_id,
                params.first_name,
                params.last_name,
                params.avatar_url,
            )
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
        avatar_url: Option<String>,
    ) -> ApiResult<User> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        Self::update_profile(
            state,
            UpdateProfileParams {
                user_id,
                first_name,
                last_name,
                avatar_url,
            },
        )
        .await
    }

    pub async fn change_password(
        state: &AppState,
        auth_user_id: &str,
        current_password: String,
        new_password: String,
    ) -> ApiResult<()> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        let user = state
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("User not found".into()))?;

        if !crate::infrastructure::crypto::verify_password(&current_password, &user.password_hash)
            .map_err(|_| ApiError::Auth("Invalid current password".into()))?
        {
            return Err(ApiError::Auth("Invalid current password".into()));
        }

        if new_password.len() < 8 {
            return Err(ApiError::BadRequest(
                "New password must be at least 8 characters".into(),
            ));
        }

        let new_hash = crate::infrastructure::crypto::hash_password(&new_password)
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        state
            .user_repo
            .update_password(user_id, new_hash)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn setup_totp(state: &AppState, auth_user_id: &str) -> ApiResult<TotpSetupResponse> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        let user = state
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("User not found".into()))?;

        if user.totp_enabled {
            return Err(ApiError::BadRequest("2FA is already enabled".into()));
        }

        let raw_secret = Secret::generate_secret();
        let secret_bytes = raw_secret
            .to_bytes()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let secret_encoded = raw_secret.to_encoded().to_string();
        let issuer = Some("Mikrom".to_string());
        let account_name = user.email.clone();

        let totp = TOTP::new(
            totp_rs::Algorithm::SHA1,
            6,
            1,
            30,
            secret_bytes,
            issuer,
            account_name,
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        let otpauth_url = totp.get_url();

        state
            .user_repo
            .update_totp_secret(user_id, Some(secret_encoded.clone()))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(TotpSetupResponse {
            secret: secret_encoded,
            otpauth_url,
        })
    }

    pub async fn verify_totp(
        state: &AppState,
        auth_user_id: &str,
        code: String,
    ) -> ApiResult<()> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        let user = state
            .user_repo
            .find_by_id(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or(ApiError::NotFound("User not found".into()))?;

        let secret = user
            .totp_secret
            .ok_or(ApiError::BadRequest("2FA not set up. Request setup first.".into()))?;

        let totp = TOTP::new(
            totp_rs::Algorithm::SHA1,
            6,
            1,
            30,
            Secret::Encoded(secret).to_bytes().map_err(|e| ApiError::Internal(e.to_string()))?,
            None,
            String::new(),
        )
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        let is_valid = totp
            .check_current(&code)
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        if !is_valid {
            return Err(ApiError::Auth("Invalid 2FA code".into()));
        }

        state
            .user_repo
            .enable_totp(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn disable_totp(state: &AppState, auth_user_id: &str) -> ApiResult<()> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        state
            .user_repo
            .disable_totp(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn delete_account(state: &AppState, auth_user_id: &str) -> ApiResult<()> {
        let user_id = Uuid::parse_str(auth_user_id)
            .map_err(|_| ApiError::Auth("Invalid user ID in token".into()))?;

        state
            .user_repo
            .soft_delete(user_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TotpSetupResponse {
    pub secret: String,
    pub otpauth_url: String,
}
