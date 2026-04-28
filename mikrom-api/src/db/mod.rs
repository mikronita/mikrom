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
    sqlx::migrate!("./migrations").run(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_connect_with_invalid_url() {
        let result = connect_to_url("invalid://url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore]
    async fn test_run_migrations() {
        let pool =
            connect_to_url("postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test")
                .await
                .unwrap();
        let result = run_migrations(&pool).await;
        assert!(result.is_ok());
    }
}
