use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use sqlx::Row;
use tracing::{error, info, warn};

pub async fn acme_challenge_handler(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query("SELECT key_auth FROM acme_challenges WHERE token = $1")
        .bind(&token)
        .fetch_optional(&state.db)
        .await;

    match result {
        Ok(Some(row)) => {
            let key_auth: String = row.get("key_auth");
            info!("Serving ACME challenge for token: {}", token);
            key_auth.into_response()
        },
        Ok(None) => {
            warn!("ACME challenge token not found: {}", token);
            StatusCode::NOT_FOUND.into_response()
        },
        Err(e) => {
            error!("Database error serving ACME challenge: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common_utils;
    use axum::extract::Path;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;
    use moka::future::Cache;

    #[tokio::test]
    async fn test_acme_challenge_handler_not_found() {
        let test_db = common_utils::TestDb::new().await;
        let db = test_db.pool().clone();

        let state = AppState {
            db,
            cache: Cache::builder().build(),
            client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(HttpConnector::new()),
        };

        let response = acme_challenge_handler(State(state), Path("non-existent-token".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
