// Thin compatibility layer re-exporting domain definitions.
// These types have moved to `crate::domain` as part of the layered architecture refactor.
pub use crate::domain::error::DomainError as DbError;
pub use crate::domain::user::{MockUserRepository, NewUser, User, UserRepository, UserRole};
