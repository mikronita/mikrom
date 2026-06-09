pub mod pipeline;

use clap::{Parser, Subcommand};
use eyre::Result;
use pipeline::MikromCi;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Local Dagger runner for the Mikrom Rust workspace"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Run the full local validation pipeline.
    Validate,
    /// Run the quick smoke pipeline.
    Smoke,
    /// Run the fast validation pipeline.
    Fast,
    /// Run the full validation pipeline.
    Full,
    /// Run the frontend validation pipeline.
    App,
    /// Run the frontend end-to-end suite.
    AppE2e,
    /// Run `cargo fmt --all -- --check`.
    Fmt,
    /// Run `cargo clippy` over the workspace.
    Clippy,
    /// Run workspace library tests.
    Test,
    /// Run opt-in external integration tests that require NATS or `PostgreSQL` fixtures.
    ExternalTests,
    /// Build the Rust workspace in release mode.
    Build,
    /// Check the eBPF target independently.
    Ebpf,
    /// Build the service images from their Dockerfiles.
    Images,
    /// Publish service images to an OCI registry.
    Publish,
    /// Run full validation and then publish service images.
    PublishRelease,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Validate);

    dagger_sdk::connect(|client| async move {
        let pipeline = MikromCi::new(&client);

        match command {
            Command::Validate | Command::Full => pipeline.validate_full().await?,
            Command::Smoke => pipeline.validate_smoke().await?,
            Command::Fast => pipeline.validate_fast().await?,
            Command::App => pipeline.app_validate().await?,
            Command::AppE2e => pipeline.app_e2e().await?,
            Command::Fmt => pipeline.fmt_check().await?,
            Command::Clippy => pipeline.clippy_check().await?,
            Command::Test => pipeline.workspace_tests().await?,
            Command::ExternalTests => pipeline.workspace_external_tests().await?,
            Command::Build => pipeline.release_build().await?,
            Command::Ebpf => pipeline.ebpf_check().await?,
            Command::Images => pipeline.build_service_images().await?,
            Command::Publish => pipeline.publish_service_images().await?,
            Command::PublishRelease => pipeline.publish_release().await?,
        }

        Ok(())
    })
    .await?;

    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
