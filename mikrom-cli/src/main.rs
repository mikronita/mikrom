use anyhow::Context;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use yansi::Paint;

fn bold_cyan(s: &str) -> std::string::String {
    Paint::new(s).cyan().bold().to_string()
}

fn green_label(s: &str) -> std::string::String {
    Paint::new(s).green().to_string()
}

fn red_label(s: &str) -> std::string::String {
    Paint::new(s).red().to_string()
}

#[allow(dead_code)]
fn yellow_label(s: &str) -> std::string::String {
    Paint::new(s).yellow().to_string()
}

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

    /// Stream logs from a VM in real-time (SSE)
    Logs {
        /// Job ID of the VM to get logs from
        job_id: String,
    },

    /// Pause a running VM by job ID
    Pause {
        /// Job ID of the VM to pause
        job_id: String,
    },

    /// Resume a paused VM by job ID
    Resume {
        /// Job ID of the VM to resume
        job_id: String,
    },

    /// Delete a VM from the registry by job ID
    Delete {
        /// Job ID of the VM to delete
        job_id: String,
    },

    /// Restart a VM (stop then start)
    Restart {
        /// Job ID of the VM to restart
        job_id: String,
    },

    /// Get host metrics for a specific VM
    Metrics {
        /// Job ID of the VM to get metrics for
        job_id: Option<String>,
    },

    /// Show current user info
    Whoami,

    /// Show config settings
    Config,
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
            println!("{} {}", green_label("job_id:"), resp.job_id);
            println!(
                "{} {}",
                green_label("status:"),
                Paint::new(&resp.status).cyan()
            );
            println!("{} {}", green_label("message:"), resp.message);
            if let Some(host_id) = resp.host_id {
                println!("{} {}", green_label("host_id:"), host_id);
            }
            if let Some(vm_id) = resp.vm_id {
                println!("{} {}", green_label("vm_id:"), vm_id);
            }
        }

        Commands::Vms => {
            let vms = client.list_vms().await.context("Failed to list VMs")?;
            if vms.is_empty() {
                println!("{}", "No VMs found.".yellow());
            } else {
                println!(
                    "{} {} {} {} IMAGE",
                    bold_cyan("JOB_ID"),
                    bold_cyan("STATUS"),
                    bold_cyan("APP_NAME"),
                    bold_cyan("VM_ID")
                );
                println!("{}", "-".repeat(120));
                for vm in vms {
                    let status_painted = match vm.status.as_str() {
                        "Running" => Paint::new(&vm.status).green(),
                        "Scheduled" | "Pending" => Paint::new(&vm.status).yellow(),
                        "Failed" | "Error" => Paint::new(&vm.status).red(),
                        _ => Paint::new(&vm.status),
                    };
                    println!(
                        "{:<38} {:<12} {:<20} {:<38} {}",
                        Paint::new(&vm.job_id).bold(),
                        status_painted,
                        Paint::new(&vm.app_name),
                        Paint::new(&vm.vm_id),
                        Paint::new(&vm.image)
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
            let resp = client.stop_vm(&job_id).await.context("Failed to stop VM")?;
            if resp.success {
                println!(
                    "{} {}",
                    "Stopped job".green(),
                    Paint::new(&format!("[{}]", job_id)).cyan()
                );
                println!("{} {}", green_label("message:"), resp.message);
            } else {
                eprintln!("{} {}", red_label("Stop reported failure:"), resp.message);
                std::process::exit(1);
            }
        }

        Commands::Logs { job_id } => match client.get_vm_logs(&job_id).await {
            Ok(logs) => println!("{}", logs),
            Err(e) => {
                eprintln!("Failed to get logs: {}", e);
                std::process::exit(1);
            }
        },

        Commands::Pause { job_id } => {
            let resp = client
                .pause_vm(&job_id)
                .await
                .context("Failed to pause VM")?;
            if resp.success {
                println!(
                    "{} {}",
                    "Paused job".green(),
                    Paint::new(&format!("[{}]", job_id)).cyan()
                );
                println!("{} {}", green_label("message:"), resp.message);
            } else {
                eprintln!("{} {}", red_label("Pause reported failure:"), resp.message);
                std::process::exit(1);
            }
        }

        Commands::Resume { job_id } => {
            let resp = client
                .resume_vm(&job_id)
                .await
                .context("Failed to resume VM")?;
            if resp.success {
                println!(
                    "{} {}",
                    "Resumed job".green(),
                    Paint::new(&format!("[{}]", job_id)).cyan()
                );
                println!("{} {}", green_label("message:"), resp.message);
            } else {
                eprintln!("{} {}", red_label("Resume reported failure:"), resp.message);
                std::process::exit(1);
            }
        }

        Commands::Delete { job_id } => {
            let resp = client
                .delete_vm(&job_id)
                .await
                .context("Failed to delete VM")?;
            if resp.success {
                println!(
                    "{} {}",
                    "Deleted job".green(),
                    Paint::new(&format!("[{}]", job_id)).cyan()
                );
                println!("{} {}", green_label("message:"), resp.message);
            } else {
                eprintln!("{} {}", red_label("Delete reported failure:"), resp.message);
                std::process::exit(1);
            }
        }

        Commands::Restart { job_id } => {
            let resp = client
                .restart_vm(&job_id)
                .await
                .context("Failed to restart VM")?;
            if resp.success {
                println!(
                    "{} {}",
                    "Restarting job".green(),
                    Paint::new(&format!("[{}]", job_id)).cyan()
                );
                println!("{} {}", green_label("message:"), resp.message);
            } else {
                eprintln!(
                    "{} {}",
                    red_label("Restart reported failure:"),
                    resp.message
                );
                std::process::exit(1);
            }
        }

        Commands::Metrics { job_id } => match job_id {
            Some(id) => {
                let metrics = client
                    .get_vm_metrics(&id)
                    .await
                    .context("Failed to get VM metrics")?;
                println!("VM: {}", id);
                println!("cpu_usage:    {:.2}%", metrics.cpu_usage);
                println!("memory:     {:.2}%", metrics.memory_usage);
                println!("disk:      {:.2}%", metrics.disk_usage);
                println!("network_rx: {} bytes", metrics.network_rx);
                println!("network_tx: {} bytes", metrics.network_tx);
            }
            None => {
                eprintln!("Error: job_id required for metrics command");
                std::process::exit(1);
            }
        },

        Commands::Whoami => {
            let user = client.whoami().await.context("Failed to get user info")?;
            println!("user_id:   {}", user.user_id);
            println!("email:     {}", user.email);
            println!("created_at: {}", user.created_at);
        }

        Commands::Config => {
            println!("api_url: {}", cfg.api_url());
            if cfg.token.is_some() {
                println!("token:    [configured]");
            } else {
                println!("token:    [not configured]");
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
        let cli = Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "stop", "job-1"])
            .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Stop { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected stop command"),
        }
    }

    // ── logs subcommand ───────────────────────────────────────────────────────

    #[test]
    fn test_cli_logs_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "logs", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Logs { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected logs command"),
        }
    }

    #[test]
    fn test_cli_logs_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "logs"]).is_err());
    }

    #[test]
    fn test_cli_logs_with_api_url_flag() {
        let cli = Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "logs", "job-1"])
            .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Logs { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected logs command"),
        }
    }

    // ── pause subcommand ───────────────────────────────────────────────────

    #[test]
    fn test_cli_pause_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "pause", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Pause { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected pause command"),
        }
    }

    #[test]
    fn test_cli_pause_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "pause"]).is_err());
    }

    #[test]
    fn test_cli_pause_with_api_url_flag() {
        let cli = Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "pause", "job-1"])
            .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Pause { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected pause command"),
        }
    }

    // ── resume subcommand ──────────────────────────────────────────────────────

    #[test]
    fn test_cli_resume_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "resume", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Resume { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected resume command"),
        }
    }

    #[test]
    fn test_cli_resume_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "resume"]).is_err());
    }

    #[test]
    fn test_cli_resume_with_api_url_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "resume", "job-1"])
                .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Resume { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected resume command"),
        }
    }

    // ── delete subcommand ─────────────────────────────────────────────────────

    #[test]
    fn test_cli_delete_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "delete", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Delete { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected delete command"),
        }
    }

    #[test]
    fn test_cli_delete_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "delete"]).is_err());
    }

    #[test]
    fn test_cli_delete_with_api_url_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "delete", "job-1"])
                .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Delete { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected delete command"),
        }
    }

    // ── restart subcommand ─────────────────────────────────────────────────────

    #[test]
    fn test_cli_restart_command_parses_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "restart", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Restart { job_id } => assert_eq!(job_id, "job-xyz"),
            _ => panic!("expected restart command"),
        }
    }

    #[test]
    fn test_cli_restart_command_requires_job_id() {
        assert!(Cli::try_parse_from(["mikrom", "restart"]).is_err());
    }

    #[test]
    fn test_cli_restart_with_api_url_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://api:5001", "restart", "job-1"])
                .unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://api:5001"));
        match cli.command {
            Commands::Restart { job_id } => assert_eq!(job_id, "job-1"),
            _ => panic!("expected restart command"),
        }
    }

    // ── metrics subcommand ────────────────────────────────────────────────────

    #[test]
    fn test_cli_metrics_command_parses_optional_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "metrics", "job-xyz"]).unwrap();
        match cli.command {
            Commands::Metrics { job_id } => assert_eq!(job_id.as_deref(), Some("job-xyz")),
            _ => panic!("expected metrics command"),
        }
    }

    #[test]
    fn test_cli_metrics_command_parses_no_job_id() {
        let cli = Cli::try_parse_from(["mikrom", "metrics"]).unwrap();
        match cli.command {
            Commands::Metrics { job_id } => assert!(job_id.is_none()),
            _ => panic!("expected metrics command"),
        }
    }

    // ── whoami subcommand ─────────────────────────────────────────────────

    #[test]
    fn test_cli_whoami_command_parses() {
        let cli = Cli::try_parse_from(["mikrom", "whoami"]).unwrap();
        assert!(matches!(cli.command, Commands::Whoami));
    }

    // ── config subcommand ─────────────────────────────────────────────────

    #[test]
    fn test_cli_config_command_parses() {
        let cli = Cli::try_parse_from(["mikrom", "config"]).unwrap();
        assert!(matches!(cli.command, Commands::Config));
    }

    // ── deploy with all options ──────────────────────────────────────────────────

    #[test]
    fn test_cli_deploy_with_vcpus_only() {
        let cli = Cli::try_parse_from([
            "mikrom", "deploy", "--app", "a", "--image", "i", "--vcpus", "2",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { vcpus, .. } => assert_eq!(vcpus, Some(2)),
            _ => panic!(),
        }
    }

    #[test]
    fn test_cli_deploy_with_memory_only() {
        let cli = Cli::try_parse_from([
            "mikrom", "deploy", "--app", "a", "--image", "i", "--memory", "512",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { memory, .. } => assert_eq!(memory, Some(512)),
            _ => panic!(),
        }
    }

    #[test]
    fn test_cli_deploy_with_disk_only() {
        let cli = Cli::try_parse_from([
            "mikrom", "deploy", "--app", "a", "--image", "i", "--disk", "5000",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { disk, .. } => assert_eq!(disk, Some(5000)),
            _ => panic!(),
        }
    }

    #[test]
    fn test_cli_deploy_with_multiple_env_vars() {
        let cli = Cli::try_parse_from([
            "mikrom", "deploy", "--app", "a", "--image", "i", "--env", "A=1", "--env", "B=2",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { env, .. } => {
                assert_eq!(env.len(), 2);
                assert!(env.contains(&"A=1".to_string()));
                assert!(env.contains(&"B=2".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_cli_deploy_with_env_value_containing_equals() {
        let cli = Cli::try_parse_from([
            "mikrom",
            "deploy",
            "--app",
            "a",
            "--image",
            "i",
            "--env",
            "URL=http://x?a=1&b=2",
        ])
        .unwrap();
        match cli.command {
            Commands::Deploy { env, .. } => {
                assert_eq!(env.len(), 1);
                assert!(env[0].contains("="));
            }
            _ => panic!(),
        }
    }

    // ── health command variants ──────────────────────────────────────────────────

    #[test]
    fn test_cli_health_is_case_sensitive() {
        let cli = Cli::try_parse_from(["mikrom", "health"]).unwrap();
        assert!(matches!(cli.command, Commands::Health));
    }

    #[test]
    fn test_cli_health_with_api_url_flag_after_command() {
        let cli =
            Cli::try_parse_from(["mikrom", "health", "--api-url", "http://late:5001"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://late:5001"));
        assert!(matches!(cli.command, Commands::Health));
    }

    // ── vms command variants ───────���───────────────────────────────────

    #[test]
    fn test_cli_vms_command_no_other_args() {
        let cli = Cli::try_parse_from(["mikrom", "vms"]).unwrap();
        assert!(matches!(cli.command, Commands::Vms));
    }

    #[test]
    fn test_cli_vms_with_api_url_but_no_flag() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://override:5001", "vms"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some("http://override:5001"));
    }

    // ── vm command variants ──────────────────────────────────────────

    #[test]
    fn test_cli_vm_with_special_characters_in_job_id() {
        let cli =
            Cli::try_parse_from(["mikrom", "vm", "job-with-dash_and_underscore.123"]).unwrap();
        match cli.command {
            Commands::Vm { job_id } => assert_eq!(job_id, "job-with-dash_and_underscore.123"),
            _ => panic!(),
        }
    }

    #[test]
    fn test_cli_vm_with_uuid_format_job_id() {
        let cli =
            Cli::try_parse_from(["mikrom", "vm", "550e8400-e29b-41d4-a716-446655440000"]).unwrap();
        match cli.command {
            Commands::Vm { job_id } => {
                assert_eq!(job_id.len(), 36);
            }
            _ => panic!(),
        }
    }

    // ── api_url flag interaction tests ────────────────────────────────────

    #[test]
    fn test_api_url_flag_works_for_all_commands() {
        let url = "http://test:9999";
        let test_cases: Vec<(&str, &str)> = vec![
            ("health", "health"),
            ("vms", "vms"),
            ("vm", "vm j-1"),
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
                "failed for cmd: {}",
                cmd_str
            );
        }
    }

    // ── command ordering ──────────────────────────────────────────────────────

    #[test]
    fn test_cli_command_order_does_not_matter() {
        let cli1 = Cli::try_parse_from(["mikrom", "--api-url", "http://a:5001", "vms"]).unwrap();
        let cli2 = Cli::try_parse_from(["mikrom", "vms", "--api-url", "http://a:5001"]).unwrap();
        assert_eq!(cli1.api_url, cli2.api_url);
    }

    // ── error cases for missing args ─────────────────────────────────────────

    #[test]
    fn test_cli_deploy_missing_both_required_fails() {
        let result = Cli::try_parse_from(["mikrom", "deploy"]);
        assert!(result.is_err());
    }

    // ── edge cases ────────────────────────────────────────────────────────

    #[test]
    fn test_cli_with_empty_string_api_url() {
        let cli = Cli::try_parse_from(["mikrom", "--api-url", "", "vms"]).unwrap();
        assert_eq!(cli.api_url.as_deref(), Some(""));
    }

    #[test]
    fn test_cli_with_localhost_api_url() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "http://localhost:5001", "vms"]).unwrap();
        assert!(cli.api_url.as_deref().unwrap().contains("localhost"));
    }

    #[test]
    fn test_cli_with_https_api_url() {
        let cli =
            Cli::try_parse_from(["mikrom", "--api-url", "https://secure:5001", "vms"]).unwrap();
        assert!(cli.api_url.as_deref().unwrap().starts_with("https"));
    }
}
