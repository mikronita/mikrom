use sqlx::Row;

#[tokio::test]
async fn api_database_does_not_include_scheduler_tables() {
    let Ok(test_db) = mikrom_api::test_utils::TestDb::try_new().await else {
        eprintln!("Skipping DB isolation test: database unavailable");
        return;
    };
    let pool = test_db.pool();

    let rows = sqlx::query(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_all(pool)
    .await
    .unwrap();

    let table_names = rows
        .into_iter()
        .map(|row| row.get::<String, _>(0))
        .collect::<Vec<_>>();

    assert!(table_names.contains(&"apps".to_string()));
    assert!(table_names.contains(&"deployments".to_string()));
    assert!(!table_names.contains(&"jobs".to_string()));
    assert!(!table_names.contains(&"workers".to_string()));
}

#[tokio::test]
async fn api_database_contains_tenant_tables() {
    let Ok(test_db) = mikrom_api::test_utils::TestDb::try_new().await else {
        eprintln!("Skipping DB isolation test: database unavailable");
        return;
    };
    let pool = test_db.pool();

    let rows = sqlx::query(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_all(pool)
    .await
    .unwrap();

    let table_names = rows
        .into_iter()
        .map(|row| row.get::<String, _>(0))
        .collect::<Vec<_>>();

    assert!(table_names.contains(&"tenants".to_string()));
    assert!(table_names.contains(&"tenant_members".to_string()));
}
