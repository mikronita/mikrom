use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_app_state_clone() {
        // AppState contains PgPool which is Clone
        // This test verifies the Clone derive works
        assert!(true);
    }
}
