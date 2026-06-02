use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    connect_to_url(database_url).await
}

pub async fn connect_to_url(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    // sqlx::migrate! embeds the migration directory at compile time, so Rust source
    // changes are the reliable way to force Cargo to pick up new migration files.
    sqlx::migrate!("./migrations").run(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestDb;

    #[tokio::test]
    async fn test_connect_with_invalid_url() {
        let result = connect_to_url("invalid://url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_migrations() {
        let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
        dotenvy::from_path(env_path).ok();
        let _db = TestDb::new().await;
        let pool = _db.pool();
        let result = run_migrations(pool).await;
        assert!(result.is_ok());
    }
}
