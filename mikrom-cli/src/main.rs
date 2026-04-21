use anyhow::Context;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use yansi::Paint;

fn bold_cyan(s: &str) -> String {
    Paint::new(s).cyan().bold().to_string()
}

fn green_label(s: &str) -> String {
    Paint::new(s).green().to_string()
}

fn red_label(s: &str) -> String {
    Paint::new(s).red().bold().to_string()
}

mod client;
mod config;
mod dashboard;

use client::MikromClient;
use config::Config;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, global = true)]
    api_url: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
    /// List all running VMs
    Vms,
    /// Get status of a specific VM
    Vm { job_id: String },
    /// Stop a running VM
    Stop { job_id: String },
    /// Get logs for a VM
    Logs {
        job_id: String,
        #[arg(long, short)]
        follow: bool,
    },
    /// Pause a running VM
    Pause { job_id: String },
    /// Resume a paused VM
    Resume { job_id: String },
    /// Delete a VM and its resources
    Delete { job_id: String },
    /// Restart a VM
    Restart { job_id: String },
    /// Get metrics for a VM or the entire host
    Metrics { job_id: Option<String> },
    /// Show information about the current user
    Whoami,
    /// Show current CLI configuration
    Config,
    /// Launch the interactive dashboard
    Dashboard,
}

#[derive(Subcommand)]
enum AuthCommands {
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
        Commands::Vms => handle_list_vms(&client).await?,
        Commands::Vm { job_id } => handle_get_vm(&client, job_id).await?,
        Commands::Stop { job_id } => handle_stop_vm(&client, job_id).await?,
        Commands::Logs { job_id, follow } => handle_logs(&client, job_id, follow).await?,
        Commands::Pause { job_id } => handle_pause_vm(&client, job_id).await?,
        Commands::Resume { job_id } => handle_resume_vm(&client, job_id).await?,
        Commands::Delete { job_id } => handle_delete_vm(&client, job_id).await?,
        Commands::Restart { job_id } => handle_restart_vm(&client, job_id).await?,
        Commands::Metrics { job_id } => handle_metrics(&client, job_id).await?,
        Commands::Whoami => handle_whoami(&client).await?,
        Commands::Config => {
            println!("api_url: {}", cfg.api_url());
            if cfg.token.is_some() {
                println!("token:    [configured]");
            } else {
                println!("token:    [not configured]");
            }
        }
        Commands::Dashboard => {
            dashboard::run(client).await?;
        }
    }

    Ok(())
}

async fn handle_health(client: &MikromClient) -> anyhow::Result<()> {
    let resp = client.health().await.context("Health check failed")?;
    println!("status:  {}", resp.status);
    println!("version: {}", resp.version);
    Ok(())
}

async fn handle_auth(
    client: &MikromClient,
    cmd: AuthCommands,
    cfg: &mut Config,
) -> anyhow::Result<()> {
    match cmd {
        AuthCommands::Register { email, password } => {
            let resp = client
                .register(&email, &password)
                .await
                .context("Registration failed")?;
            println!("{}", resp.message);
            println!("user_id: {}", resp.user_id);
        }
        AuthCommands::Login { email, password } => {
            let resp = client
                .login(&email, &password)
                .await
                .context("Login failed")?;
            cfg.token = Some(resp.token);
            cfg.save().context("Failed to save config")?;
            println!("Logged in. Token saved to ~/.config/mikrom/config.toml");
        }
    }
    Ok(())
}

async fn handle_deploy(
    client: &MikromClient,
    app: String,
    image: String,
    vcpus: Option<u32>,
    memory: Option<u64>,
    disk: Option<u64>,
    env: Vec<String>,
) -> anyhow::Result<()> {
    let env_map = parse_env_vars(&env).context("Invalid --env value (expected KEY=VALUE)")?;
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
    Ok(())
}

async fn handle_list_vms(client: &MikromClient) -> anyhow::Result<()> {
    let vms = client.list_vms().await.context("Failed to list VMs")?;
    if vms.is_empty() {
        println!("{}", Paint::new("No VMs found.").yellow());
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
    Ok(())
}

async fn handle_get_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
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
    Ok(())
}

async fn handle_stop_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
    let resp = client.stop_vm(&job_id).await.context("Failed to stop VM")?;
    if resp.success {
        println!(
            "{} {}",
            "Stopped job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        eprintln!("{} {}", red_label("Stop reported failure:"), resp.message);
        std::process::exit(1);
    }
    Ok(())
}

async fn handle_logs(client: &MikromClient, job_id: String, follow: bool) -> anyhow::Result<()> {
    if follow {
        use futures_util::StreamExt;
        println!(
            "{}",
            Paint::new("--- Attaching to log stream (auto-reconnect enabled) ---").dim()
        );

        loop {
            let stream_result = client.stream_vm_logs(&job_id).await;

            match stream_result {
                Ok(stream) => {
                    tokio::pin!(stream);
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(line) => {
                                if !line.is_empty() {
                                    println!("{line}");
                                }
                            }
                            Err(e) => {
                                eprintln!("{}: {e}", Paint::new("Stream error").red());
                                break; // Break inner loop to reconnect
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{}: {e}", Paint::new("Connection failed").red());
                }
            }

            println!(
                "{}",
                Paint::new("--- Connection lost, retrying in 2s... ---").dim()
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    } else {
        match client.get_vm_logs(&job_id).await {
            Ok(logs) => println!("{logs}"),
            Err(e) => {
                eprintln!("Failed to get logs: {e}");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

async fn handle_pause_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
    let resp = client
        .pause_vm(&job_id)
        .await
        .context("Failed to pause VM")?;
    if resp.success {
        println!(
            "{} {}",
            "Paused job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        eprintln!("{} {}", red_label("Pause reported failure:"), resp.message);
        std::process::exit(1);
    }
    Ok(())
}

async fn handle_resume_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
    let resp = client
        .resume_vm(&job_id)
        .await
        .context("Failed to resume VM")?;
    if resp.success {
        println!(
            "{} {}",
            "Resumed job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        eprintln!("{} {}", red_label("Resume reported failure:"), resp.message);
        std::process::exit(1);
    }
    Ok(())
}

async fn handle_delete_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
    let resp = client
        .delete_vm(&job_id)
        .await
        .context("Failed to delete VM")?;
    if resp.success {
        println!(
            "{} {}",
            "Deleted job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        eprintln!("{} {}", red_label("Delete reported failure:"), resp.message);
        std::process::exit(1);
    }
    Ok(())
}

async fn handle_restart_vm(client: &MikromClient, job_id: String) -> anyhow::Result<()> {
    let resp = client
        .restart_vm(&job_id)
        .await
        .context("Failed to restart VM")?;
    if resp.success {
        println!(
            "{} {}",
            "Restarting job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
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
    Ok(())
}

async fn handle_metrics(client: &MikromClient, job_id: Option<String>) -> anyhow::Result<()> {
    match job_id {
        Some(id) => {
            let metrics = client
                .get_vm_metrics(&id)
                .await
                .context("Failed to get VM metrics")?;
            println!("VM: {id}");
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
    }
    Ok(())
}

async fn handle_whoami(client: &MikromClient) -> anyhow::Result<()> {
    let user = client.whoami().await.context("Failed to get user info")?;
    println!("user_id:   {}", user.user_id);
    println!("email:     {}", user.email);
    println!("created_at: {}", user.created_at);
    Ok(())
}

fn parse_env_vars(env: &[String]) -> anyhow::Result<HashMap<String, String>> {
    env.iter()
        .map(|s| {
            let (key, val) = s
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("'{s}' is not KEY=VALUE"))?;
            Ok((key.to_string(), val.to_string()))
        })
        .collect()
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
            }
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
                "failed for cmd: {cmd_str}"
            );
        }
    }
}
