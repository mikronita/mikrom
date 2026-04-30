#![allow(clippy::large_enum_variant)]

pub mod scheduler {
    include!("mikrom.scheduler.v1.rs");
    pub use scheduler_service_client::SchedulerServiceClient;
    pub use scheduler_service_server::{SchedulerService, SchedulerServiceServer};
}

pub mod agent {
    include!("mikrom.agent.v1.rs");
    pub use agent_service_client::AgentServiceClient;
    pub use agent_service_server::{AgentService, AgentServiceServer};
}

pub mod builder {
    include!("mikrom.builder.v1.rs");
    pub use builder_service_client::BuilderServiceClient;
    pub use builder_service_server::{BuilderService, BuilderServiceServer};
}

pub mod router {
    include!("mikrom.router.v1.rs");
}

pub mod subjects;
pub mod telemetry;
pub mod tls;
