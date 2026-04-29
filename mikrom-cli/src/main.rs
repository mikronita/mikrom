use clap::Parser;
use mikrom_cli::client::MikromClient;
use mikrom_cli::commands::Commands;
use mikrom_cli::config::Config;
use mikrom_cli::handlers::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load();

    let client = MikromClient::new(cfg.api_url().to_string(), cfg.token.clone());

    match cli.command {
        Commands::Auth(auth_cmd) => handle_auth(&client, auth_cmd, &mut cfg).await?,
        Commands::App(app_cmd) => handle_app(&client, app_cmd).await?,
        Commands::Deployment(dep_cmd) => handle_deployment(&client, dep_cmd).await?,
        Commands::Config(cfg_cmd) => handle_config(cfg_cmd, &mut cfg).await?,
        Commands::System(sys_cmd) => handle_system(&client, sys_cmd).await?,
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;
    use mikrom_cli::commands::{AppCommands, AuthCommands, DeploymentCommands, SystemCommands};

    #[test]
    fn test_cli_system_health_command() {
        let cli = Cli::try_parse_from(["mikrom", "system", "health"]).unwrap();
        match cli.command {
            Commands::System(SystemCommands::Health) => {},
            _ => panic!("expected system health"),
        }
    }

    #[test]
    fn test_cli_auth_register_parses_email_and_password() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "auth",
            "register",
            "--email",
            "user@example.com",
            "--password",
            "secret123",
        ])
        .unwrap();
        match cli.command {
            Commands::Auth(AuthCommands::Register { email, password }) => {
                assert_eq!(email, "user@example.com");
                assert_eq!(password, "secret123");
            },
            _ => panic!("expected auth register"),
        }
    }

    #[test]
    fn test_cli_app_create() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "app",
            "create",
            "--name",
            "my-service",
            "--git-url",
            "https://github.com/user/repo",
        ])
        .unwrap();
        match cli.command {
            Commands::App(AppCommands::Create { name, git_url }) => {
                assert_eq!(name, "my-service");
                assert_eq!(git_url, "https://github.com/user/repo");
            },
            _ => panic!("expected app create"),
        }
    }

    #[test]
    fn test_cli_deployment_status() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deployment",
            "status",
            "--app",
            "svc",
            "--job-id",
            "job-123",
        ])
        .unwrap();
        match cli.command {
            Commands::Deployment(DeploymentCommands::Status { app, job_id }) => {
                assert_eq!(app, "svc");
                assert_eq!(job_id, "job-123");
            },
            _ => panic!("expected deployment status"),
        }
    }
}
