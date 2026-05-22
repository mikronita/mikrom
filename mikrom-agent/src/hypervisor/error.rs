use thiserror::Error;

#[derive(Error, Debug)]
pub enum HypervisorError {
    #[error("VM not found: {0}")]
    VmNotFound(String),
    #[error("Failed to start VM: {0}")]
    StartFailed(String),
    #[error("Failed to stop VM: {0}")]
    StopFailed(String),
    #[error("Hypervisor process error: {0}")]
    ProcessError(String),
    #[error("Hypervisor API error on {path}: {msg}")]
    ApiError { path: String, msg: String },
    #[error("Timed out waiting for socket: {0}")]
    SocketTimeout(String),
}
