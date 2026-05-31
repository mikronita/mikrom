#![allow(clippy::all)]

pub mod application;
pub mod domain;
pub mod infrastructure;

pub use application::sync::run_nats_subscriber;
pub use infrastructure::bootstrap::run;
