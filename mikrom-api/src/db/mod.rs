use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn connect() -> Result<PgPool, sqlx::Error> {
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|e| sqlx::Error::Configuration(Box::new(e)))?;
    connect_to_url(&database_url).await
}

pub async fn connect_to_url(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

pub fn get_migration_sql() -> &'static str {
    include_str!("../../migrations/001_create_users_table.sql")
}

pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let migration_sql = get_migration_sql();
    sqlx::query(migration_sql)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_migration_sql() {
        let sql = get_migration_sql();
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("users"));
        assert!(sql.contains("id"));
        assert!(sql.contains("email"));
        assert!(sql.contains("password_hash"));
    }

    #[test]
    fn test_migration_sql_has_primary_key() {
        let sql = get_migration_sql();
        assert!(sql.contains("PRIMARY KEY"));
    }

    #[tokio::test]
    #[ignore]
    async fn test_connect_with_invalid_url() {
        let result = connect_to_url("invalid://url").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore]
    async fn test_run_migrations() {
        let pool = connect_to_url("postgres://mikrom:mikrom_password@localhost:5432/mikrom_api")
            .await
            .unwrap();
        let result = run_migrations(&pool).await;
        assert!(result.is_ok());
    }
}
