#[allow(clippy::all)]
#[allow(warnings)]
pub mod scheduler {
    include!("mikrom.scheduler.v1.rs");
    pub use scheduler_service_client::SchedulerServiceClient;
}

#[allow(clippy::all)]
#[allow(warnings)]
pub mod agent {
    include!("mikrom.agent.v1.rs");
    pub use agent_service_client::AgentServiceClient;
}

pub mod tls;
