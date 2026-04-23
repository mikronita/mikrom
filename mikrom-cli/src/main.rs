mod client;
mod config;
mod dashboard;
mod handlers;

use clap::{Parser, Subcommand};
use client::MikromClient;
use config::Config;
use handlers::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(long, global = true)]
    pub api_url: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Check the health of the Mikrom API
    Health,
    /// Authentication commands
    #[command(subcommand)]
    Auth(AuthCommands),
    /// Deploy a new application
    Deploy {
        #[arg(long)]
        app: String,
        #[arg(long)]
        image: String,
        #[arg(long)]
        vcpus: Option<u32>,
        #[arg(long)]
        memory: Option<u64>,
        #[arg(long)]
        disk: Option<u64>,
        #[arg(long)]
        env: Vec<String>,
    },
    /// List all active deployments
    Deployments,
    /// Get status of a specific deployment
    Status { job_id: String },
    /// Stop a running deployment
    Stop { job_id: String },
    /// Get logs for a deployment
    Logs {
        job_id: String,
        #[arg(long, short)]
        follow: bool,
    },
    /// Pause a running deployment
    Pause { job_id: String },
    /// Resume a paused deployment
    Resume { job_id: String },
    /// Delete a deployment record
    Delete { job_id: String },
    /// Restart a deployment
    Restart { job_id: String },
    /// Get metrics for a deployment or the entire cluster
    Metrics { job_id: Option<String> },
    /// Show information about the current user
    Whoami,
    /// Manage applications
    #[command(subcommand)]
    Apps(AppCommands),
    /// Show current CLI configuration
    Config,
    /// Launch the interactive dashboard
    Dashboard,
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Register a new user
    Register {
        #[arg(long, short)]
        email: String,
        #[arg(long, short)]
        password: String,
    },
    /// Login with existing credentials
    Login {
        #[arg(long, short)]
        email: String,
        #[arg(long, short)]
        password: String,
    },
}

#[derive(Subcommand)]
pub enum AppCommands {
    /// List all applications
    List,
    /// Create a new application
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        git_url: String,
    },
    /// Delete an application
    Delete { app_id: String },
    /// Deploy an application
    Deploy { app_id: String },
    /// Activate a specific deployment for an application (Rollback/Promotion)
    Activate {
        app_id: String,
        deployment_id: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load();

    if let Some(url) = cli.api_url {
        cfg.api_url = Some(url);
    }

    let client = MikromClient::new(cfg.api_url().to_string(), cfg.token.clone());

    match cli.command {
        Commands::Health => handle_health(&client).await?,
        Commands::Auth(auth_cmd) => handle_auth(&client, auth_cmd, &mut cfg).await?,
        Commands::Deploy {
            app,
            image,
            vcpus,
            memory,
            disk,
            env,
        } => handle_deploy(&client, app, image, vcpus, memory, disk, env).await?,
        Commands::Deployments => handle_list_deployments(&client).await?,
        Commands::Status { job_id } => handle_get_status(&client, job_id).await?,
        Commands::Stop { job_id } => handle_stop_instance(&client, job_id).await?,
        Commands::Logs { job_id, follow } => handle_logs(&client, job_id, follow).await?,
        Commands::Pause { job_id } => handle_pause_instance(&client, job_id).await?,
        Commands::Resume { job_id } => handle_resume_instance(&client, job_id).await?,
        Commands::Delete { job_id } => handle_delete_instance(&client, job_id).await?,
        Commands::Restart { job_id } => handle_restart_instance(&client, job_id).await?,
        Commands::Metrics { job_id } => handle_metrics(&client, job_id).await?,
        Commands::Whoami => handle_whoami(&client).await?,
        Commands::Apps(app_cmd) => handle_apps(&client, app_cmd).await?,
        Commands::Config => {
            println!("api_url: {}", cfg.api_url());
            if cfg.token.is_some() {
                println!("token:    [configured]");
            } else {
                println!("token:    [not configured]");
            }
        },
        Commands::Dashboard => {
            dashboard::run(client).await?;
        },
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_vars_empty() {
        assert!(parse_env_vars(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_parse_env_vars_single_pair() {
        let result = parse_env_vars(&["KEY=VALUE".to_string()]).unwrap();
        assert_eq!(result.get("KEY").map(String::as_str), Some("VALUE"));
    }

    #[test]
    fn test_parse_env_vars_multiple_pairs() {
        let result = parse_env_vars(&["PORT=8080".to_string(), "ENV=prod".to_string()]).unwrap();
        assert_eq!(result.get("PORT").map(String::as_str), Some("8080"));
        assert_eq!(result.get("ENV").map(String::as_str), Some("prod"));
    }

    #[test]
    fn test_parse_env_vars_value_with_equals() {
        let result = parse_env_vars(&["URL=http://host/path?a=1&b=2".to_string()]).unwrap();
        assert_eq!(
            result.get("URL").map(String::as_str),
            Some("http://host/path?a=1&b=2")
        );
    }

    #[test]
    fn test_parse_env_vars_missing_equals_returns_err() {
        assert!(parse_env_vars(&["NO_EQUALS".to_string()]).is_err());
    }

    #[test]
    fn test_cli_health_command() {
        let cli = Cli::try_parse_from(["mikrom", "health"]).unwrap();
        assert!(matches!(cli.command, Commands::Health));
    }

    #[test]
    fn test_cli_api_url_flag_before_subcommand() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://remote:5001", "health"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://remote:5001"));
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
    fn test_cli_deploy_minimal_flags() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deploy",
            "--app",
            "my-service",
            "--image",
            "nginx:latest",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { app, image, .. } => {
                assert_eq!(app, "my-service");
                assert_eq!(image, "nginx:latest");
            },
            _ => panic!("expected deploy"),
        }
    }

    #[test]
    fn test_cli_deploy_all_options() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deploy",
            "--app",
            "svc",
            "--image",
            "alpine:3",
            "--vcpus",
            "4",
            "--memory",
            "1024",
            "--disk",
            "2048",
            "--env",
            "PORT=8080",
            "--env",
            "X=y",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy {
                vcpus,
                memory,
                disk,
                env,
                ..
            } => {
                assert_eq!(vcpus, Some(4));
                assert_eq!(memory, Some(1024));
                assert_eq!(disk, Some(2048));
                assert_eq!(env, vec!["PORT=8080", "X=y"]);
            },
            _ => panic!("expected deploy"),
        }
    }

    #[test]
    fn test_api_url_flag_works_for_all_commands() {
        let url = "http://test:9999";
        let test_cases: Vec<(&str, &str)> = vec![
            ("health", "health"),
            ("deployments", "deployments"),
            ("status", "status j-1"),
            ("stop", "stop j-1"),
            ("logs", "logs j-1"),
            ("pause", "pause j-1"),
            ("resume", "resume j-1"),
            ("delete", "delete j-1"),
            ("restart", "restart j-1"),
            ("metrics", "metrics j-1"),
            ("whoami", "whoami"),
            ("config", "config"),
        ];
        for (_, cmd_str) in test_cases {
            let mut args = vec!["mikrom", "--api-url", url];
            args.extend(cmd_str.split_whitespace());
            let cli = Cli::try_parse_from(&args).unwrap();
            assert_eq!(
                cli.api_url.as_deref(),
                Some(url),
                "failed for cmd: {cmd_str}"
            );
        }
    }
}
