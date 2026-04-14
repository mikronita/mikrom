use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use serde::{Deserialize, Serialize};
use uuid::Uuid as UuidType;

use crate::AppState;

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
) -> Response {
    if payload.email.is_empty() || payload.password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "Email and password are required".to_string(),
        })).into_response();
    }

    if payload.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "Password must be at least 8 characters".to_string(),
        })).into_response();
    }

    let existing: Result<(i64,), _> = sqlx::query_as(
        "SELECT COUNT(*) FROM users WHERE email = $1"
    )
    .bind(&payload.email)
    .fetch_one(&state.db)
    .await;

    let count = match existing {
        Ok((c,)) => c,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: "Database error".to_string(),
            })).into_response();
        }
    };

    if count > 0 {
        return (StatusCode::CONFLICT, Json(ErrorResponse {
            error: "Email already registered".to_string(),
        })).into_response();
    }

    let password_hash = match hash(&payload.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: "Failed to hash password".to_string(),
            })).into_response();
        }
    };

    let user_id = UuidType::new_v4();

    let result = sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)"
    )
    .bind(user_id)
    .bind(&payload.email)
    .bind(&password_hash)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => (StatusCode::CREATED, Json(RegisterResponse {
            message: "User registered successfully".to_string(),
            user_id: user_id.to_string(),
        })).into_response(),
        Err(e) => {
            tracing::error!("Failed to create user: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: "Failed to create user".to_string(),
            })).into_response()
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Response {
    if payload.email.is_empty() || payload.password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "Email and password are required".to_string(),
        })).into_response();
    }

    let result: Result<Option<(sqlx::types::Uuid, String)>, _> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE email = $1"
    )
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await;

    let user = match result {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            })).into_response();
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: "Database error".to_string(),
            })).into_response();
        }
    };

    if verify(&payload.password, &user.1).unwrap_or(false) {
        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
        match crate::auth::jwt::create_token(&user.0.to_string(), &payload.email, &jwt_secret) {
            Ok(token) => (StatusCode::OK, Json(LoginResponse { token })).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse {
                error: "Failed to create token".to_string(),
            })).into_response(),
        }
    } else {
        (StatusCode::UNAUTHORIZED, Json(ErrorResponse {
            error: "Invalid credentials".to_string(),
        })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_register_empty_email() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_empty_password() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_short_password() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"test@example.com","password":"1234567"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_register_success() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);
        let email = format!("newuser_{}@example.com", uuid::Uuid::new_v4());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({
                        "email": email,
                        "password": "password123"
                    }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "User registered successfully");
        assert!(json["user_id"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_register_duplicate_email() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);
        let email = format!("duplicate_{}@example.com", uuid::Uuid::new_v4());
        let body = serde_json::json!({
            "email": email,
            "password": "password123"
        }).to_string();

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_login_empty_email() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_login_empty_password() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
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

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_login_user_not_found() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"email":"nonexistent@example.com","password":"password123"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);
        let email = format!("loginwrong_{}@example.com", uuid::Uuid::new_v4());
        
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({
                        "email": email,
                        "password": "correctpassword"
                    }).to_string()))
                    .unwrap(),
            )
            .await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({
                        "email": email,
                        "password": "wrongpassword"
                    }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_login_success() {
        let pool = create_test_pool().await;
        let app = create_test_app(pool);
        let email = format!("loginok_{}@example.com", uuid::Uuid::new_v4());
        
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/register")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({
                        "email": email,
                        "password": "validpassword123"
                    }).to_string()))
                    .unwrap(),
            )
            .await;

        unsafe { std::env::set_var("JWT_SECRET", "test-secret") };

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/login")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::json!({
                        "email": email,
                        "password": "validpassword123"
                    }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["token"].as_str().is_some());
    }

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

    async fn create_test_pool() -> sqlx::PgPool {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://mikrom:mikrom_password@localhost:5432/mikrom_api".to_string());
        
        sqlx::PgPool::connect(&database_url)
            .await
            .expect("Failed to connect to test database")
    }

    fn create_test_app(pool: sqlx::PgPool) -> axum::Router {
        let state = crate::AppState { db: pool };
        axum::Router::new()
            .route("/auth/register", axum::routing::post(register))
            .route("/auth/login", axum::routing::post(login))
            .with_state(state)
    }
}
