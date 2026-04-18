use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bcrypt::{DEFAULT_COST, hash, verify};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::repositories::user_repository::NewUser;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> impl IntoResponse {
    tracing::info!(email = %payload.email, "Registering new user");

    if payload.email.is_empty() || payload.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Email and password are required".to_string(),
            }),
        )
            .into_response();
    }

    if payload.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Password must be at least 8 characters".to_string(),
            }),
        )
            .into_response();
    }

    let count = match state.user_repo.count_by_email(&payload.email).await {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Database error".to_string(),
                }),
            )
                .into_response();
        }
    };

    if count > 0 {
        return (
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "Email already registered".to_string(),
            }),
        )
            .into_response();
    }

    let password_hash = match hash(&payload.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to hash password".to_string(),
                }),
            )
                .into_response();
        }
    };

    match state
        .user_repo
        .create(NewUser {
            email: payload.email.clone(),
            password_hash,
        })
        .await
    {
        Ok(user_id) => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                message: "User registered successfully".to_string(),
                user_id: user_id.to_string(),
            }),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to create user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to create user".to_string(),
                }),
            )
                .into_response()
        }
    }
}

pub async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> Response {
    tracing::info!(email = %payload.email, "User login attempt");
    if payload.email.is_empty() || payload.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Email and password are required".to_string(),
            }),
        )
            .into_response();
    }

    let user = match state.user_repo.find_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid credentials".to_string(),
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Database error".to_string(),
                }),
            )
                .into_response();
        }
    };

    if verify(&payload.password, &user.password_hash).unwrap_or(false) {
        match crate::auth::jwt::create_token(&user.id.to_string(), &user.email, &state.jwt_secret) {
            Ok(token) => (StatusCode::OK, Json(LoginResponse { token })).into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to create token".to_string(),
                }),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            }),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use sqlx::types::Uuid;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::repositories::user_repository::{DbError, NewUser, User, UserRepository};

    // ── Test helpers ─────────────────────────────────────────────────────────

    /// Repository whose write operations panic — used for validation-only tests
    /// that return before any DB call is made.
    struct PanicRepo;
    #[async_trait]
    impl UserRepository for PanicRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            panic!("PanicRepo: find_by_email should not be called in this test")
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            panic!("PanicRepo: create should not be called in this test")
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            panic!("PanicRepo: count_by_email should not be called in this test")
        }
    }

    /// Repository that reports a given email as already taken (count = 1).
    struct EmailTakenRepo;
    #[async_trait]
    impl UserRepository for EmailTakenRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Ok(Uuid::new_v4())
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Ok(1)
        }
    }

    /// Repository that always succeeds with a fresh user id.
    struct FreshRepo;
    #[async_trait]
    impl UserRepository for FreshRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Ok(Uuid::new_v4())
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Ok(0)
        }
    }

    /// Repository where count_by_email succeeds (email available) but create fails.
    struct CreateFailsRepo;
    #[async_trait]
    impl UserRepository for CreateFailsRepo {
        async fn find_by_email(&self, _: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _: NewUser) -> Result<Uuid, DbError> {
            Err(DbError {
                message: "insert error".to_string(),
            })
        }
        async fn count_by_email(&self, _: &str) -> Result<i64, DbError> {
            Ok(0)
        }
    }

    /// Repository that simulates a DB error on every call.
    struct ErrorRepo;
    #[async_trait]
    impl UserRepository for ErrorRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Err(DbError {
                message: "db error".to_string(),
            })
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Err(DbError {
                message: "db error".to_string(),
            })
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Err(DbError {
                message: "db error".to_string(),
            })
        }
    }

    /// Repository that returns a specific stored user on find_by_email.
    struct StoredUserRepo(User);
    #[async_trait]
    impl UserRepository for StoredUserRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Ok(Some(self.0.clone()))
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Ok(self.0.id)
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Ok(1)
        }
    }

    fn make_app(repo: impl UserRepository + 'static) -> axum::Router {
        let state = crate::AppState {
            user_repo: Arc::new(repo),
            scheduler_client: None,
            scheduler_config: crate::scheduler::SchedulerConfig::default(),
            jwt_secret: "test-handlers-secret".to_string(),
        };
        axum::Router::new()
            .route("/auth/register", axum::routing::post(register))
            .route("/auth/login", axum::routing::post(login))
            .with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // ── register — input validation (no DB call) ──────────────────────────────

    #[tokio::test]
    async fn test_register_empty_email() {
        let resp = make_app(PanicRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"","password":"password123"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_empty_password() {
        let resp = make_app(PanicRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"test@example.com","password":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_short_password() {
        let resp = make_app(PanicRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"test@example.com","password":"1234567"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── register — repository outcomes ───────────────────────────────────────

    #[tokio::test]
    async fn test_register_duplicate_email_returns_conflict() {
        let resp = make_app(EmailTakenRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"taken@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_register_success_returns_created() {
        let resp = make_app(FreshRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"new@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["message"], "User registered successfully");
        assert!(json["user_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_register_db_error_returns_500() {
        let resp = make_app(ErrorRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"new@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_register_create_fails_returns_500_with_message() {
        let resp = make_app(CreateFailsRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"new@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert_eq!(json["error"], "Failed to create user");
    }

    // ── login — input validation ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_login_empty_email() {
        let resp = make_app(PanicRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"","password":"password123"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_login_empty_password() {
        let resp = make_app(PanicRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"test@example.com","password":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── login — repository outcomes ───────────────────────────────────────────

    #[tokio::test]
    async fn test_login_user_not_found_returns_unauthorized() {
        let resp = make_app(FreshRepo) // find_by_email returns None
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"nobody@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_wrong_password_returns_unauthorized() {
        // StoredUserRepo returns a user with a real bcrypt hash of "correctpassword".
        let hash = bcrypt::hash("correctpassword", 4).unwrap();
        let user = User {
            id: Uuid::new_v4(),
            email: "user@example.com".to_string(),
            password_hash: hash,
        };
        let resp = make_app(StoredUserRepo(user))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"user@example.com","password":"wrongpassword"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_success_returns_token() {
        let hash = bcrypt::hash("validpassword", 4).unwrap();
        let user = User {
            id: Uuid::new_v4(),
            email: "user@example.com".to_string(),
            password_hash: hash,
        };
        let resp = make_app(StoredUserRepo(user))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"user@example.com","password":"validpassword"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["token"].as_str().is_some());
        assert!(!json["token"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_login_db_error_returns_500() {
        let resp = make_app(ErrorRepo)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        r#"{"email":"user@example.com","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ── serialization ─────────────────────────────────────────────────────────

    #[test]
    fn test_register_request_deserialization() {
        let json = r#"{"email":"test@example.com","password":"password123"}"#;
        let request: RegisterRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.email, "test@example.com");
        assert_eq!(request.password, "password123");
    }

    #[test]
    fn test_register_response_serialization() {
        let response = RegisterResponse {
            message: "success".to_string(),
            user_id: "123".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("123"));
    }

    #[test]
    fn test_login_request_deserialization() {
        let json = r#"{"email":"test@example.com","password":"password123"}"#;
        let request: LoginRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.email, "test@example.com");
        assert_eq!(request.password, "password123");
    }

    #[test]
    fn test_login_response_serialization() {
        let response = LoginResponse {
            token: "jwt.token.here".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("jwt.token.here"));
    }

    #[test]
    fn test_error_response_serialization() {
        let response = ErrorResponse {
            error: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Something went wrong"));
    }
}
