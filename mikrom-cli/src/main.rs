use clap::{CommandFactory, Parser};
use mikrom_cli::application::context::CliContext;
use mikrom_cli::application::dispatcher::dispatch;
use mikrom_cli::commands::{Commands, OutputFormat};
use mikrom_cli::config::Config;
use mikrom_cli::infrastructure::http::client::ReqwestApiClient;
use std::sync::Arc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        long,
        short,
        value_enum,
        default_value_t = OutputFormat::Table,
        global = true,
        help = "Output format: table or json"
    )]
    pub output: OutputFormat,

    #[arg(
        long,
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        help = "Increase verbosity (use -v, -vv, -vvv)"
    )]
    pub verbose: u8,

    #[arg(
        long,
        global = true,
        help = "Disable colored output (also respects NO_COLOR env var)"
    )]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

fn init_tracing(verbosity: u8) {
    let level = match verbosity {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        2 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    init_tracing(cli.verbose);

    if cli.no_color || std::env::var("NO_COLOR").is_ok() {
        yansi::disable();
    }

    // Handle completions before loading config (no API needed)
    if let Commands::Completion { shell } = &cli.command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(*shell, &mut cmd, name, &mut std::io::stdout());
        return Ok(());
    }

    let mut cfg = Config::load()?;
    cfg.validate()?;

    let api_url = cfg.api_url().to_string();
    let token = cfg.token.clone();

    let client = Arc::new(ReqwestApiClient::new(api_url, token)?);
    let ctx = CliContext::new(Arc::new(cfg.clone()), client);

    if let Err(e) = dispatch(&ctx, cli.command, &mut cfg, cli.output).await {
        tracing::error!(error = %e, "Command failed");
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;
    use mikrom_cli::commands::{
        AppCommands, AuthCommands, ConfigCommands, DbCommands, DeploymentCommands, OutputFormat,
        SystemCommands,
    };

    #[test]
    fn test_cli_system_health_command() {
        let cli = Cli::try_parse_from(["mikrom", "system", "health"]).unwrap();
        assert_eq!(cli.output, OutputFormat::Table);
        match cli.command {
            Commands::System(SystemCommands::Health) => {},
            _ => panic!("expected system health"),
        }
    }

    #[test]
    fn test_cli_output_json_global_before_command() {
        let cli = Cli::try_parse_from(["mikrom", "--output", "json", "system", "health"]).unwrap();
        assert_eq!(cli.output, OutputFormat::Json);
        match cli.command {
            Commands::System(SystemCommands::Health) => {},
            _ => panic!("expected system health"),
        }
    }

    #[test]
    fn test_cli_output_json_global_after_command() {
        let cli = Cli::try_parse_from(["mikrom", "system", "health", "--output", "json"]).unwrap();
        assert_eq!(cli.output, OutputFormat::Json);
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

    #[test]
    fn test_cli_app_list_parses() {
        let cli = Cli::try_parse_from(["mikrom", "app", "list"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::List) => {},
            _ => panic!("expected app list"),
        }
    }

    #[test]
    fn test_cli_app_delete_parses_name() {
        let cli = Cli::try_parse_from(["mikrom", "app", "delete", "--name", "svc"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Delete { name, yes: _ }) => assert_eq!(name, "svc"),
            _ => panic!("expected app delete"),
        }
    }

    #[test]
    fn test_cli_app_deploy_parses_name() {
        let cli = Cli::try_parse_from(["mikrom", "app", "deploy", "--name", "svc"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Deploy {
                name,
                cpu,
                memory,
                hypervisor,
            }) => {
                assert_eq!(name, "svc");
                assert!(cpu.is_none());
                assert!(memory.is_none());
                assert!(hypervisor.is_none());
            },
            _ => panic!("expected app deploy"),
        }
    }

    #[test]
    fn test_cli_app_deploy_parses_resources() {
        let cli = Cli::try_parse_from([
            "mikrom", "app", "deploy", "--name", "svc", "--cpu", "3", "--memory", "2G",
        ])
        .unwrap();
        match cli.command {
            Commands::App(AppCommands::Deploy {
                name,
                cpu,
                memory,
                hypervisor,
            }) => {
                assert_eq!(name, "svc");
                assert_eq!(cpu, Some(3));
                assert_eq!(memory, Some(2048));
                assert!(hypervisor.is_none());
            },
            _ => panic!("expected app deploy"),
        }
    }

    #[test]
    fn test_cli_app_activate_parses_app_and_deployment() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "app",
            "activate",
            "--app",
            "svc",
            "--deployment-id",
            "dep-123",
        ])
        .unwrap();
        match cli.command {
            Commands::App(AppCommands::Activate { app, deployment_id }) => {
                assert_eq!(app, "svc");
                assert_eq!(deployment_id, "dep-123");
            },
            _ => panic!("expected app activate"),
        }
    }

    #[test]
    fn test_cli_app_deployments_parses_name() {
        let cli = Cli::try_parse_from(["mikrom", "app", "deployments", "--name", "svc"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Deployments { name }) => assert_eq!(name, "svc"),
            _ => panic!("expected app deployments"),
        }
    }

    #[test]
    fn test_cli_app_secret_parses_name() {
        let cli = Cli::try_parse_from(["mikrom", "app", "secret", "--name", "svc"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Secret { name }) => assert_eq!(name, "svc"),
            _ => panic!("expected app secret"),
        }
    }

    #[test]
    fn test_cli_auth_whoami_parses() {
        let cli = Cli::try_parse_from(["mikrom", "auth", "whoami"]).unwrap();
        match cli.command {
            Commands::Auth(AuthCommands::Whoami) => {},
            _ => panic!("expected auth whoami"),
        }
    }

    #[test]
    fn test_cli_auth_update_parses_names() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "auth",
            "update",
            "--first-name",
            "Ada",
            "--last-name",
            "Lovelace",
        ])
        .unwrap();
        match cli.command {
            Commands::Auth(AuthCommands::Update {
                first_name,
                last_name,
            }) => {
                assert_eq!(first_name.as_deref(), Some("Ada"));
                assert_eq!(last_name.as_deref(), Some("Lovelace"));
            },
            _ => panic!("expected auth update"),
        }
    }

    #[test]
    fn test_cli_deployment_list_parses() {
        let cli = Cli::try_parse_from(["mikrom", "deployment", "list"]).unwrap();
        match cli.command {
            Commands::Deployment(DeploymentCommands::List) => {},
            _ => panic!("expected deployment list"),
        }
    }

    #[test]
    fn test_cli_deployment_lifecycle_commands_parse() {
        for command in ["stop", "pause", "resume", "delete"] {
            let cli = Cli::try_parse_from([
                "mikrom",
                "deployment",
                command,
                "--app",
                "svc",
                "--job-id",
                "job-123",
            ])
            .unwrap();
            match (command, cli.command) {
                ("stop", Commands::Deployment(DeploymentCommands::Stop { app, job_id }))
                | ("pause", Commands::Deployment(DeploymentCommands::Pause { app, job_id }))
                | ("resume", Commands::Deployment(DeploymentCommands::Resume { app, job_id }))
                | (
                    "delete",
                    Commands::Deployment(DeploymentCommands::Delete {
                        app,
                        job_id,
                        yes: _,
                    }),
                ) => {
                    assert_eq!(app, "svc");
                    assert_eq!(job_id, "job-123");
                },
                _ => panic!("expected deployment lifecycle command"),
            }
        }
    }

    #[test]
    fn test_cli_config_show_parses() {
        let cli = Cli::try_parse_from(["mikrom", "config", "show"]).unwrap();
        match cli.command {
            Commands::Config(ConfigCommands::Show) => {},
            _ => panic!("expected config show"),
        }
    }

    #[test]
    fn test_cli_config_set_parses_key_and_value() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "config",
            "set",
            "api-url",
            "http://localhost:5001",
        ])
        .unwrap();
        match cli.command {
            Commands::Config(ConfigCommands::Set { key, value }) => {
                assert_eq!(key, "api-url");
                assert_eq!(value, "http://localhost:5001");
            },
            _ => panic!("expected config set"),
        }
    }

    #[test]
    fn test_cli_app_scale_parses() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "app",
            "scale",
            "--name",
            "my-app",
            "--replicas",
            "3",
            "--auto",
            "true",
            "--max",
            "5",
            "--cpu",
            "75.5",
        ])
        .unwrap();
        match cli.command {
            Commands::App(AppCommands::Scale {
                name,
                replicas,
                auto,
                min,
                max,
                cpu,
                mem,
            }) => {
                assert_eq!(name, "my-app");
                assert_eq!(replicas, Some(3));
                assert_eq!(auto, Some(true));
                assert_eq!(max, Some(5));
                assert_eq!(cpu, Some(75.5));
                assert_eq!(mem, None);
                assert_eq!(min, None);
            },
            _ => panic!("expected app scale"),
        }
    }

    #[test]
    fn test_cli_volume_create_parses_name_and_size() {
        let cli = Cli::try_parse_from([
            "mikrom", "volume", "create", "--name", "data", "--size", "1024",
        ])
        .unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::Create { name, size }) => {
                assert_eq!(name, "data");
                assert_eq!(size, 1024);
            },
            _ => panic!("expected volume create"),
        }
    }

    #[test]
    fn test_cli_volume_list_parses_optional_app() {
        let cli = Cli::try_parse_from(["mikrom", "volume", "list"]).unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::List { app }) => {
                assert!(app.is_none());
            },
            _ => panic!("expected volume list"),
        }

        let cli = Cli::try_parse_from(["mikrom", "volume", "list", "--app", "svc"]).unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::List { app }) => {
                assert_eq!(app.as_deref(), Some("svc"));
            },
            _ => panic!("expected volume list with app"),
        }
    }

    #[test]
    fn test_cli_volume_attach_parses_fields() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "volume",
            "attach",
            "--app",
            "svc",
            "--volume-id",
            "vol-123",
            "--mount",
            "/data",
            "--mode",
            "2",
        ])
        .unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::Attach {
                app,
                volume_id,
                mount,
                mode,
            }) => {
                assert_eq!(app, "svc");
                assert_eq!(volume_id, "vol-123");
                assert_eq!(mount, "/data");
                assert_eq!(mode, 2);
            },
            _ => panic!("expected volume attach"),
        }
    }

    #[test]
    fn test_cli_volume_detach_parses_fields() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "volume",
            "detach",
            "--app",
            "svc",
            "--volume-id",
            "vol-123",
        ])
        .unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::Detach { app, volume_id }) => {
                assert_eq!(app, "svc");
                assert_eq!(volume_id, "vol-123");
            },
            _ => panic!("expected volume detach"),
        }
    }

    #[test]
    fn test_cli_verbose_flag_parses() {
        let cli = Cli::try_parse_from(["mikrom", "-v", "system", "health"]).unwrap();
        assert_eq!(cli.verbose, 1);
    }

    #[test]
    fn test_cli_verbose_double_flag_parses() {
        let cli = Cli::try_parse_from(["mikrom", "-vv", "system", "health"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn test_cli_no_color_flag_parses() {
        let cli = Cli::try_parse_from(["mikrom", "--no-color", "system", "health"]).unwrap();
        assert!(cli.no_color);
    }

    #[test]
    fn test_cli_completion_subcommand_parses() {
        let cli = Cli::try_parse_from(["mikrom", "completion", "bash"]).unwrap();
        match cli.command {
            Commands::Completion { shell } => {
                assert_eq!(shell, clap_complete::Shell::Bash);
            },
            _ => panic!("expected completion"),
        }
    }

    #[test]
    fn test_cli_delete_with_yes_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "app", "delete", "--name", "svc", "--yes"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Delete { name, yes }) => {
                assert_eq!(name, "svc");
                assert!(yes);
            },
            _ => panic!("expected app delete with yes"),
        }
    }

    #[test]
    fn test_cli_volume_delete_with_yes_flag() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "volume",
            "delete",
            "--volume-id",
            "vol-1",
            "--yes",
        ])
        .unwrap();
        match cli.command {
            Commands::Volume(mikrom_cli::commands::VolumeCommands::Delete { volume_id, yes }) => {
                assert_eq!(volume_id, "vol-1");
                assert!(yes);
            },
            _ => panic!("expected volume delete with yes"),
        }
    }

    #[test]
    fn test_cli_deployment_delete_with_yes_flag() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deployment",
            "delete",
            "--app",
            "svc",
            "--job-id",
            "job-1",
            "--yes",
        ])
        .unwrap();
        match cli.command {
            Commands::Deployment(DeploymentCommands::Delete { app, job_id, yes }) => {
                assert_eq!(app, "svc");
                assert_eq!(job_id, "job-1");
                assert!(yes);
            },
            _ => panic!("expected deployment delete with yes"),
        }
    }

    #[test]
    fn test_cli_db_list_parses() {
        let cli = Cli::try_parse_from(["mikrom", "db", "list"]).unwrap();
        match cli.command {
            Commands::Db(DbCommands::List) => {},
            _ => panic!("expected db list"),
        }
    }

    #[test]
    fn test_cli_db_create_parses_defaults() {
        let cli = Cli::try_parse_from(["mikrom", "db", "create", "orders"]).unwrap();
        match cli.command {
            Commands::Db(DbCommands::Create {
                name,
                engine,
                vcpus,
                memory,
                disk,
                settings,
            }) => {
                assert_eq!(name, "orders");
                assert_eq!(engine, "neon");
                assert_eq!(vcpus, 1);
                assert_eq!(memory, "512M");
                assert_eq!(disk, 1024);
                assert!(settings.is_empty());
            },
            _ => panic!("expected db create"),
        }
    }

    #[test]
    fn test_cli_db_delete_parses_yes_flag() {
        let cli = Cli::try_parse_from(["mikrom", "db", "delete", "db-1", "--yes"]).unwrap();
        match cli.command {
            Commands::Db(DbCommands::Delete { id, yes }) => {
                assert_eq!(id, "db-1");
                assert!(yes);
            },
            _ => panic!("expected db delete"),
        }
    }
}
