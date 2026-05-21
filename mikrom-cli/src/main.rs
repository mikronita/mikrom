use clap::Parser;
use mikrom_cli::client::MikromClient;
use mikrom_cli::commands::{Commands, OutputFormat};
use mikrom_cli::config::Config;
use mikrom_cli::handlers::*;

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

    #[command(subcommand)]
    pub command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load();

    let client = MikromClient::new(cfg.api_url().to_string(), cfg.token.clone());

    match cli.command {
        Commands::Auth(auth_cmd) => handle_auth(&client, auth_cmd, &mut cfg, cli.output).await?,
        Commands::App(app_cmd) => handle_app(&client, app_cmd, cli.output).await?,
        Commands::Deployment(dep_cmd) => handle_deployment(&client, dep_cmd, cli.output).await?,
        Commands::Config(cfg_cmd) => handle_config(cfg_cmd, &mut cfg, cli.output).await?,
        Commands::Volume(vol_cmd) => handle_volume(&client, vol_cmd, cli.output).await?,
        Commands::System(sys_cmd) => handle_system(&client, sys_cmd, cli.output).await?,
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::get_unwrap)]
mod tests {
    use super::*;
    use mikrom_cli::commands::{
        AppCommands, AuthCommands, ConfigCommands, DeploymentCommands, OutputFormat, SystemCommands,
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
            Commands::App(AppCommands::Delete { name }) => assert_eq!(name, "svc"),
            _ => panic!("expected app delete"),
        }
    }

    #[test]
    fn test_cli_app_deploy_parses_name() {
        let cli = Cli::try_parse_from(["mikrom", "app", "deploy", "--name", "svc"]).unwrap();
        match cli.command {
            Commands::App(AppCommands::Deploy { name, cpu, memory }) => {
                assert_eq!(name, "svc");
                assert!(cpu.is_none());
                assert!(memory.is_none());
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
            Commands::App(AppCommands::Deploy { name, cpu, memory }) => {
                assert_eq!(name, "svc");
                assert_eq!(cpu, Some(3));
                assert_eq!(memory, Some(2048));
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
    fn test_cli_deployment_logs_parses_follow() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deployment",
            "logs",
            "--app",
            "svc",
            "--job-id",
            "job-123",
            "--follow",
        ])
        .unwrap();
        match cli.command {
            Commands::Deployment(DeploymentCommands::Logs {
                app,
                job_id,
                follow,
            }) => {
                assert_eq!(app, "svc");
                assert_eq!(job_id, "job-123");
                assert!(follow);
            },
            _ => panic!("expected deployment logs"),
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
                | ("delete", Commands::Deployment(DeploymentCommands::Delete { app, job_id })) => {
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
}
