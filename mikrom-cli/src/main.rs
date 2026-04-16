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
