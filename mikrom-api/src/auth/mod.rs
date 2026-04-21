pub mod extractor;
pub mod handlers;
pub mod jwt;

pub use extractor::AuthUser;
pub use handlers::{get_profile, login, register, update_profile};

#[cfg(test)]
pub use jwt::{create_token, verify_token};
