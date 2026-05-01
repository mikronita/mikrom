#[path = "common_utils.rs"]
mod common_utils;

#[cfg(test)]
mod tests {
    use super::common_utils;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    #[ignore = "requires a running postgres"]
    async fn test_last_write_wins_logic() {
        let test_db = common_utils::TestDb::new().await;
        let pool = test_db.pool().clone();
        let hostname = format!("lww-test-{}.mikrom.local", uuid::Uuid::new_v4());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let past = now - 60;
        let future = now + 60;

        // 1. Insert with "now"
        sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3)) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at WHERE EXCLUDED.updated_at > routes.updated_at")
            .bind(&hostname)
            .bind("http://target-now:8080")
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();

        // 2. Attempt to update with "past" (should be ignored)
        sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3)) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at WHERE EXCLUDED.updated_at > routes.updated_at")
            .bind(&hostname)
            .bind("http://target-past:8080")
            .bind(past)
            .execute(&pool)
            .await
            .unwrap();

        let row: (String,) = sqlx::query_as("SELECT target_url FROM routes WHERE hostname = $1")
            .bind(&hostname)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(
            row.0, "http://target-now:8080",
            "Update from the past should have been ignored"
        );

        // 3. Update with "future" (should succeed)
        sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3)) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at WHERE EXCLUDED.updated_at > routes.updated_at")
            .bind(&hostname)
            .bind("http://target-future:8080")
            .bind(future)
            .execute(&pool)
            .await
            .unwrap();

        let row: (String,) = sqlx::query_as("SELECT target_url FROM routes WHERE hostname = $1")
            .bind(&hostname)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(
            row.0, "http://target-future:8080",
            "Update from the future should have been applied"
        );

        // 4. Test deletion with past timestamp (should be ignored)
        sqlx::query("DELETE FROM routes WHERE hostname = $1 AND updated_at <= TO_TIMESTAMP($2)")
            .bind(&hostname)
            .bind(past)
            .execute(&pool)
            .await
            .unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM routes WHERE hostname = $1")
            .bind(&hostname)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 1,
            "Deletion with past timestamp should have been ignored"
        );

        // 5. Test deletion with future timestamp (should succeed)
        sqlx::query("DELETE FROM routes WHERE hostname = $1 AND updated_at <= TO_TIMESTAMP($2)")
            .bind(&hostname)
            .bind(future + 1)
            .execute(&pool)
            .await
            .unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM routes WHERE hostname = $1")
            .bind(&hostname)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count.0, 0,
            "Deletion with future timestamp should have succeeded"
        );
    }
}
