use anyhow::Context;
use clap::{Parser, Subcommand};
use std::collections::HashMap;

mod client;
mod config;

use client::MikromClient;
use config::Config;

#[derive(Parser)]
#[command(
    name = "mikrom",
    about = "CLI for the mikrom orchestration platform",
    version
)]
struct Cli {
    /// API base URL (overrides config, env: MIKROM_API_URL)
    #[arg(long, env = "MIKROM_API_URL", global = true)]
    api_url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check the health of the mikrom API
    Health,

    /// Authentication commands
    #[command(subcommand)]
    Auth(AuthCommands),

    /// Deploy an application
    Deploy {
        /// Application name
        #[arg(long)]
        app: String,
        /// Container image (e.g. nginx:latest)
        #[arg(long)]
        image: String,
        /// Number of vCPUs (default: 1)
        #[arg(long)]
        vcpus: Option<u32>,
        /// Memory in MiB (default: 256)
        #[arg(long)]
        memory: Option<u64>,
        /// Disk size in MiB (default: 1024)
        #[arg(long)]
        disk: Option<u64>,
        /// Environment variable (repeatable, format: KEY=VALUE)
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env: Vec<String>,
    },

    /// List all deployed VMs for the current user
    Vms,

    /// Show status of a specific VM by job ID
    Vm {
        /// Job ID returned by the deploy command
        job_id: String,
    },

    /// Stop a running VM by job ID
    Stop {
        /// Job ID of the VM to stop
        job_id: String,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Register a new account
    Register {
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: String,
    },
    /// Log in and save the session token to ~/.config/mikrom/config.toml
    Login {
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: String,
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
        Commands::Health => {
            let resp = client.health().await.context("Health check failed")?;
            println!("status:  {}", resp.status);
            println!("version: {}", resp.version);
        }

        Commands::Auth(AuthCommands::Register { email, password }) => {
            let resp = client
                .register(&email, &password)
                .await
                .context("Registration failed")?;
            println!("{}", resp.message);
            println!("user_id: {}", resp.user_id);
        }

        Commands::Auth(AuthCommands::Login { email, password }) => {
            let resp = client
                .login(&email, &password)
                .await
                .context("Login failed")?;
            cfg.token = Some(resp.token);
            cfg.save().context("Failed to save config")?;
            println!("Logged in. Token saved to ~/.config/mikrom/config.toml");
        }

        Commands::Deploy {
            app,
            image,
            vcpus,
            memory,
            disk,
            env,
        } => {
            let env_map =
                parse_env_vars(&env).context("Invalid --env value (expected KEY=VALUE)")?;
            let resp = client
                .deploy(&app, &image, vcpus, memory, disk, env_map)
                .await
                .context("Deploy failed")?;
            println!("job_id:  {}", resp.job_id);
            println!("status:  {}", resp.status);
            println!("message: {}", resp.message);
            if let Some(host_id) = resp.host_id {
                println!("host_id: {}", host_id);
            }
            if let Some(vm_id) = resp.vm_id {
                println!("vm_id:   {}", vm_id);
            }
        }

        Commands::Vms => {
            let vms = client.list_vms().await.context("Failed to list VMs")?;
            if vms.is_empty() {
                println!("No VMs found.");
            } else {
                println!(
                    "{:<38} {:<12} {:<20} {:<38} IMAGE",
                    "JOB_ID", "STATUS", "APP_NAME", "VM_ID"
                );
                println!("{}", "-".repeat(120));
                for vm in vms {
                    println!(
                        "{:<38} {:<12} {:<20} {:<38} {}",
                        vm.job_id, vm.status, vm.app_name, vm.vm_id, vm.image
                    );
                }
            }
        }

        Commands::Vm { job_id } => {
            let vm = client
                .get_vm(&job_id)
                .await
                .context("Failed to get VM status")?;
            println!("job_id:       {}", vm.job_id);
            println!("status:       {}", vm.status);
            println!("host_id:      {}", vm.host_id);
            println!("vm_id:        {}", vm.vm_id);
            println!("scheduled_at: {}", vm.scheduled_at);
            println!("started_at:   {}", vm.started_at);
            if vm.stopped_at > 0 {
                println!("stopped_at:   {}", vm.stopped_at);
            }
            if !vm.error_message.is_empty() {
                println!("error:        {}", vm.error_message);
            }
        }

        Commands::Stop { job_id } => {
            let resp = client
                .stop_vm(&job_id)
                .await
                .context("Failed to stop VM")?;
            if resp.success {
                println!("Stopped job {}.", job_id);
                println!("message: {}", resp.message);
            } else {
                eprintln!("Stop reported failure: {}", resp.message);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn parse_env_vars(pairs: &[String]) -> anyhow::Result<HashMap<String, String>> {
    pairs
        .iter()
        .map(|s| {
            let (key, val) = s
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("'{}' is not KEY=VALUE", s))?;
            Ok((key.to_string(), val.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_env_vars ───────────────────────────────────────────────────────────

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
        // Only splits on the first '=' — the rest of the value is preserved.
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

    // ── CLI parsing ──────────────────────────────────────────────────────────────

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
            }
            _ => panic!("expected auth register"),
        }
    }

    #[test]
    fn test_cli_auth_login_parses_email_and_password() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "auth",
            "login",
            "--email",
            "user@example.com",
            "--password",
            "pass",
        ])
        .unwrap();
        match cli.command {
            Commands::Auth(AuthCommands::Login { email, password }) => {
                assert_eq!(email, "user@example.com");
                assert_eq!(password, "pass");
            }
            _ => panic!("expected auth login"),
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
            Commands::Deploy {
                app,
                image,
                vcpus,
                memory,
                disk,
                env,
            } => {
                assert_eq!(app, "my-service");
                assert_eq!(image, "nginx:latest");
                assert!(vcpus.is_none());
                assert!(memory.is_none());
                assert!(disk.is_none());
                assert!(env.is_empty());
            }
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
            }
            _ => panic!("expected deploy"),
        }
    }

    #[test]
    fn test_cli_deploy_missing_app_fails() {
        assert!(Cli::try_parse_from(["mikrom", "deploy", "--image", "nginx"]).is_err());
    }

    #[test]
    fn test_cli_deploy_missing_image_fails() {
        assert!(Cli::try_parse_from(["mikrom", "deploy", "--app", "svc"]).is_err());
    }

    // ── vms / vm subcommands ─────────────────────────────────────────────────

    #[test]
    fn test_cli_vms_command_parses() {
        let cli = Cli::try_parse_from(["mikrom", "vms"]).unwrap();
        assert!(matches!(cli.command, Commands::Vms));
    }

    #[test]
    fn test_cli_vm_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "vm", "job-abc-123"]).unwrap();
        match cli.command {
            Commands::Vm { job_id } => assert_eq!(job_id, "job-abc-123"),
            _ => panic!("expected vm command"),
        }
    }

    #[test]
    fn test_cli_vm_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "vm"]).is_err());
    }

    #[test]
    fn test_cli_vms_with_api_url_flag() {
        let cli = Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "vms"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        assert!(matches!(cli.command, Commands::Vms));
    }

    #[test]
    fn test_cli_vm_with_api_url_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "vm", "job-1"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Vm { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected vm command"),
        }
    }

    // ── stop subcommand ───────────────────────────────────────────────────────

    #[test]
    fn test_cli_stop_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "stop", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Stop { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected stop command"),
        }
    }

    #[test]
    fn test_cli_stop_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "stop"]).is_err());
    }

    #[test]
    fn test_cli_stop_with_api_url_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "stop", "job-1"])
                .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Stop { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected stop command"),
        }
    }
}
