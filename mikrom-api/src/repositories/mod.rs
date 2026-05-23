pub mod app_repository;
pub mod github_repository;
pub mod user_repository;
pub mod volume_repository;

// Re-export implementations from infrastructure/db for backward compatibility.
pub use crate::infrastructure::db::{
    PostgresAppRepository, PostgresGithubRepository, PostgresUserRepository,
    PostgresVolumeRepository,
};

pub use app_repository::{
    AppRepository, CreateAppParams, Deployment, GitMetadata, MockAppRepository, NewDeployment,
    SecurityRule, UpdateDeploymentParams,
};
pub use github_repository::{GithubRepository, MockGithubRepository};
pub use user_repository::{DbError, MockUserRepository, NewUser, User, UserRepository, UserRole};
pub use volume_repository::{
    CreateSnapshotParams, CreateVolumeParams, MockVolumeRepository, VolumeAccessMode,
    VolumeRepository,
};
