#![allow(clippy::large_enum_variant)]

pub mod scheduler {
    include!("mikrom.scheduler.v1.rs");
}

pub mod agent {
    include!("mikrom.agent.v1.rs");
}

pub mod builder {
    include!("mikrom.builder.v1.rs");
}

pub mod router {
    include!("mikrom.router.v1.rs");
}

pub mod id;
pub mod sixpn;
pub mod subjects;
pub mod telemetry;
pub mod tls;
