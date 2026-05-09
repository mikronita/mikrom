use thiserror::Error;

#[derive(Error, Debug)]
pub enum EbpfError {
    #[error("eBPF binary not found")]
    BinaryNotFound,
    #[error("failed to load eBPF program: {0}")]
    LoadError(#[from] aya::EbpfError),
    #[error("failed to initialize eBPF logger: {0}")]
    LoggerError(String),
    #[error("program {0} not found")]
    ProgramNotFound(String),
    #[error("map {0} not found")]
    MapNotFound(String),
    #[error("failed to attach TC filter: {0}")]
    AttachError(#[from] aya::programs::tc::TcError),
    #[error("program error: {0}")]
    ProgramError(#[from] aya::programs::ProgramError),
    #[error("failed to update rules: {0}")]
    MapUpdateError(String),
    #[error("failed to cast program/map: {0}")]
    CastError(String),
}
