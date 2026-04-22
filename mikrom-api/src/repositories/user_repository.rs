use async_trait::async_trait;
use sqlx::types::Uuid;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, sqlx::Type, Default)]
#[sqlx(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    #[default]
    User,
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Not found")]
    NotFound,

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal repository error: {0}")]
    Internal(String),
}

/// Object-safe async repository trait.  All implementations must derive
/// `Send + Sync`; `#[async_trait]` boxes the returned futures automatically.
#[mockall::automock]
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DbError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, DbError>;
    async fn create(&self, user: NewUser) -> Result<Uuid, DbError>;
    async fn count_by_email(&self, email: &str) -> Result<i64, DbError>;
    async fn update_profile(
        &self,
        id: Uuid,
        first_name: Option<String>,
        last_name: Option<String>,
    ) -> Result<User, DbError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user() -> User {
        User {
            id: Uuid::new_v4(),
            email: "test@example.com".to_string(),
            password_hash: "hashed_password".to_string(),
            role: UserRole::User,
            first_name: None,
            last_name: None,
        }
    }

    struct FoundUserRepo(User);
    #[async_trait]
    impl UserRepository for FoundUserRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Ok(Some(self.0.clone()))
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<User>, DbError> {
            Ok(Some(self.0.clone()))
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Ok(self.0.id)
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Ok(1)
        }
        async fn update_profile(
            &self,
            _id: Uuid,
            _first_name: Option<String>,
            _last_name: Option<String>,
        ) -> Result<User, DbError> {
            Ok(self.0.clone())
        }
    }

    struct EmptyRepo;
    #[async_trait]
    impl UserRepository for EmptyRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<User>, DbError> {
            Ok(None)
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Ok(Uuid::new_v4())
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Ok(0)
        }
        async fn update_profile(
            &self,
            _id: Uuid,
            _first_name: Option<String>,
            _last_name: Option<String>,
        ) -> Result<User, DbError> {
            Err(DbError::NotFound)
        }
    }

    struct ErrorRepo;
    #[async_trait]
    impl UserRepository for ErrorRepo {
        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DbError> {
            Err(DbError::Internal("simulated find error".to_string()))
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<User>, DbError> {
            Err(DbError::Internal("simulated find_id error".to_string()))
        }
        async fn create(&self, _user: NewUser) -> Result<Uuid, DbError> {
            Err(DbError::Internal("simulated insert error".to_string()))
        }
        async fn count_by_email(&self, _email: &str) -> Result<i64, DbError> {
            Err(DbError::Internal("simulated count error".to_string()))
        }
        async fn update_profile(
            &self,
            _id: Uuid,
            _first_name: Option<String>,
            _last_name: Option<String>,
        ) -> Result<User, DbError> {
            Err(DbError::Internal("simulated update error".to_string()))
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
        assert!(err.to_string().contains("simulated find error"));
    }

    #[tokio::test]
    async fn test_mock_create_returns_uuid_on_success() {
        let user = sample_user();
        let id = FoundUserRepo(user.clone())
            .create(NewUser {
                email: "new@example.com".to_string(),
                password_hash: "hash".to_string(),
                role: UserRole::User,
                first_name: None,
                last_name: None,
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
                role: UserRole::User,
                first_name: None,
                last_name: None,
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
                role: UserRole::User,
                first_name: None,
                last_name: None,
            })
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
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

    #[test]
    fn test_user_clone_preserves_all_fields() {
        let user = sample_user();
        let cloned = user.clone();
        assert_eq!(cloned.id, user.id);
        assert_eq!(cloned.email, user.email);
        assert_eq!(cloned.password_hash, user.password_hash);
        assert_eq!(cloned.role, user.role);
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
            role: UserRole::User,
            first_name: None,
            last_name: None,
        };
        let s = format!("{:?}", new_user);
        assert!(s.contains("NewUser"));
        assert!(s.contains("a@b.com"));
    }
}
