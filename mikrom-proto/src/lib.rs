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

pub mod tls;
