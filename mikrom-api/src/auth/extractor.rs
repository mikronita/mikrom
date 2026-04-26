use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::auth::jwt::Claims;

/// Authenticated user extracted from the `Authorization: Bearer <token>` header.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub email: String,
    pub role: crate::repositories::user_repository::UserRole,
}

#[derive(Debug, Serialize)]
struct AuthError {
    error: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, axum::Json(self)).into_response()
    }
}

#[axum::async_trait]
impl FromRequestParts<crate::AppState> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = if let Some(header) = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            header
                .strip_prefix("Bearer ")
                .ok_or_else(|| {
                    AuthError {
                        error: "Authorization header must use Bearer scheme".to_string(),
                    }
                    .into_response()
                })?
                .to_string()
        } else {
            // Try query param for EventSource/SSE support
            #[derive(serde::Deserialize)]
            struct TokenQuery {
                token: String,
            }

            axum::extract::Query::<TokenQuery>::from_request_parts(parts, state)
                .await
                .ok()
                .map(|q| q.0.token)
                .ok_or_else(|| {
                    AuthError {
                        error: "Missing Authorization header or token query parameter".to_string(),
                    }
                    .into_response()
                })?
        };

        let Claims {
            sub, email, role, ..
        } = crate::auth::jwt::verify_token(&token, &state.jwt_secret).map_err(|_| {
            AuthError {
                error: "Invalid or expired token".to_string(),
            }
            .into_response()
        })?;

        Ok(AuthUser {
            user_id: sub,
            email,
            role,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request, routing::get};
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::repositories::user_repository::{DbError, NewUser, User, UserRepository};

    struct NoopRepo;
    #[async_trait]
    impl UserRepository for NoopRepo {
        async fn find_by_email(&self, _: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn find_by_id(&self, _: sqlx::types::Uuid) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _: NewUser) -> Result<sqlx::types::Uuid, DbError> {
            Ok(sqlx::types::Uuid::new_v4())
        }
        async fn count_by_email(&self, _: &str) -> Result<i64, DbError> {
            Ok(0)
        }
        async fn update_profile(
            &self,
            id: sqlx::types::Uuid,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<User, DbError> {
            Ok(User {
                id,
                email: "noop@example.com".to_string(),
                password_hash: "".to_string(),
                role: crate::repositories::user_repository::UserRole::User,
                first_name: None,
                last_name: None,
            })
        }
    }

    /// Minimal handler that uses AuthUser — returns 200 with the user id.
    async fn whoami(auth: AuthUser) -> axum::Json<serde_json::Value> {
        axum::Json(
            serde_json::json!({ "user_id": auth.user_id, "email": auth.email, "role": auth.role }),
        )
    }

    fn make_app(jwt_secret: &str) -> axum::Router {
        let db_pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let app_repo = crate::repositories::PostgresAppRepository::new(db_pool);
        let state = crate::AppState {
            user_repo: Arc::new(NoopRepo),
            app_repo: Arc::new(app_repo),
            scheduler: Arc::new(crate::scheduler::MockScheduler::new()),
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            builder_addr: "http://localhost:5004".to_string(),
            router_addr: "http://localhost:8080".to_string(),
            jwt_secret: jwt_secret.to_string(),
            master_key: "test-master-key".into(),
            deployment_events: tokio::sync::broadcast::channel(1).0,
            build_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        axum::Router::new()
            .route("/whoami", get(whoami))
            .with_state(state)
    }

    fn make_token(secret: &str) -> String {
        crate::auth::jwt::create_token(
            "uid-1",
            "user@example.com",
            &crate::repositories::user_repository::UserRole::User,
            secret,
        )
        .unwrap()
    }

    // ── missing / malformed header ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_missing_auth_header_returns_401() {
        let resp = make_app("any-secret")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_wrong_scheme_returns_401() {
        let resp = make_app("any-secret")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .header("Authorization", "Basic dXNlcjpwYXNz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_malformed_token_returns_401() {
        let resp = make_app("any-secret")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .header("Authorization", "Bearer not.a.real.token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_wrong_secret_returns_401() {
        let token = make_token("wrong-secret");
        let resp = make_app("correct-secret")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── valid token ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_valid_token_returns_200_with_claims() {
        let secret = "test-secret-extractor";
        let token = make_token(secret);
        let resp = make_app(secret)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["user_id"], "uid-1");
        assert_eq!(json["email"], "user@example.com");
    }

    // ── error response JSON ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_unauthorized_response_has_error_field() {
        let resp = make_app("any-secret")
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/whoami")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["error"].as_str().is_some());
    }
}
