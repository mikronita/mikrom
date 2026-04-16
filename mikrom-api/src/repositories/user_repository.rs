use async_trait::async_trait;
use sqlx::types::Uuid;

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
}

#[derive(Debug)]
pub struct NewUser {
    pub email: String,
    pub password_hash: String,
}

#[derive(Debug)]
pub struct DbError {
    pub message: String,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DbError {}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        DbError {
            message: err.to_string(),
        }
    }
}

/// Object-safe async repository trait.  All implementations must derive
/// `Send + Sync`; `#[async_trait]` boxes the returned futures automatically.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DbError>;
    async fn create(&self, user: NewUser) -> Result<Uuid, DbError>;
    async fn count_by_email(&self, email: &str) -> Result<i64, DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DbError ──────────────────────────────────────────────────────────────

    #[test]
    fn test_db_error_display() {
        let err = DbError {
            message: "connection refused".to_string(),
        };
        assert_eq!(format!("{}", err), "connection refused");
    }

    #[test]
    fn test_db_error_debug_format() {
        let err = DbError {
            message: "oops".to_string(),
        };
        let s = format!("{:?}", err);
        assert!(s.contains("oops"));
    }

    #[test]
    fn test_db_error_implements_error_trait() {
        let err = DbError {
            message: "test error".to_string(),
        };
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_db_error_from_sqlx_row_not_found() {
        let sqlx_err = sqlx::Error::RowNotFound;
        let expected_msg = sqlx_err.to_string();
        let err = DbError::from(sqlx_err);
        assert_eq!(err.message, expected_msg);
    }

    #[test]
    fn test_db_error_from_sqlx_pool_timed_out() {
        let err = DbError::from(sqlx::Error::PoolTimedOut);
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_db_error_from_sqlx_pool_closed() {
        let err = DbError::from(sqlx::Error::PoolClosed);
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_db_error_message_preserved_in_display() {
        let msg = "unique constraint violated on column email";
        let err = DbError {
            message: msg.to_string(),
        };
        assert!(format!("{}", err).contains(msg));
    }

    // ── Manual UserRepository mocks ───────────────────────────────────────────

    fn sample_user() -> User {
        User {
            id: Uuid::new_v4(),
            email: "test@example.com".to_string(),
            password_hash: "$2b$12$hashedpassword".to_string(),
        }
    }

    struct FoundUserRepo(User);
    #[async_trait]
    impl UserRepository for FoundUserRepo {
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

    struct EmptyRepo;
    #[async_trait]
    impl UserRepository for EmptyRepo {
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

    struct ErrorRepo;
    #[async_trait]
    impl UserRepository for ErrorRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Err(DbError {
                message: "simulated find error".to_string(),
            })
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Err(DbError {
                message: "simulated insert error".to_string(),
            })
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Err(DbError {
                message: "simulated count error".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn test_mock_find_by_email_returns_user_when_found() {
        let user = sample_user();
        let repo = FoundUserRepo(user.clone());
        let result: Result<Option<User>, DbError> = repo.find_by_email("test@example.com").await;
        let found = result.unwrap().unwrap();
        assert_eq!(found.id, user.id);
        assert_eq!(found.email, user.email);
        assert_eq!(found.password_hash, user.password_hash);
    }

    #[tokio::test]
    async fn test_mock_find_by_email_returns_none_when_not_found() {
        let result: Result<Option<User>, DbError> =
            EmptyRepo.find_by_email("nobody@example.com").await;
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_find_by_email_propagates_db_error() {
        let err: DbError = ErrorRepo.find_by_email("x@x.com").await.unwrap_err();
        assert!(err.message.contains("simulated find error"));
    }

    #[tokio::test]
    async fn test_mock_create_returns_uuid_on_success() {
        let user = sample_user();
        let id = FoundUserRepo(user.clone())
            .create(NewUser {
                email: "new@example.com".to_string(),
                password_hash: "hash".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(id, user.id);
    }

    #[tokio::test]
    async fn test_mock_create_new_uuid_on_empty_repo() {
        let id = EmptyRepo
            .create(NewUser {
                email: "new@example.com".to_string(),
                password_hash: "hash".to_string(),
            })
            .await
            .unwrap();
        assert!(!id.is_nil());
    }

    #[tokio::test]
    async fn test_mock_create_propagates_db_error() {
        let result = ErrorRepo
            .create(NewUser {
                email: "x@x.com".to_string(),
                password_hash: "hash".to_string(),
            })
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("simulated insert error")
        );
    }

    #[tokio::test]
    async fn test_mock_count_by_email_returns_one_when_found() {
        let count: i64 = FoundUserRepo(sample_user())
            .count_by_email("test@example.com")
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_mock_count_by_email_returns_zero_when_not_found() {
        let count: i64 = EmptyRepo
            .count_by_email("nobody@example.com")
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_mock_count_by_email_propagates_db_error() {
        let result: Result<i64, DbError> = ErrorRepo.count_by_email("x@x.com").await;
        assert!(result.is_err());
    }

    // ── User / NewUser struct tests ───────────────────────────────────────────

    #[test]
    fn test_user_clone_preserves_all_fields() {
        let user = sample_user();
        let cloned = user.clone();
        assert_eq!(cloned.id, user.id);
        assert_eq!(cloned.email, user.email);
        assert_eq!(cloned.password_hash, user.password_hash);
    }

    #[test]
    fn test_user_debug_format() {
        let user = sample_user();
        let s = format!("{:?}", user);
        assert!(s.contains("User"));
        assert!(s.contains("test@example.com"));
    }

    #[test]
    fn test_new_user_debug_format() {
        let new_user = NewUser {
            email: "a@b.com".to_string(),
            password_hash: "hashed".to_string(),
        };
        let s = format!("{:?}", new_user);
        assert!(s.contains("NewUser"));
        assert!(s.contains("a@b.com"));
    }
}
