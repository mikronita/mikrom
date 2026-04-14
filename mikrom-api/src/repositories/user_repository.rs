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

pub trait UserRepository: Send + Sync {
    fn find_by_email(
        &self,
        email: &str,
    ) -> impl std::future::Future<Output = Result<Option<User>, DbError>> + Send + '_;
    fn create(
        &self,
        user: NewUser,
    ) -> impl std::future::Future<Output = Result<sqlx::types::Uuid, DbError>> + Send + '_;
    fn count_by_email(
        &self,
        email: &str,
    ) -> impl std::future::Future<Output = Result<i64, DbError>> + Send + '_;
}
