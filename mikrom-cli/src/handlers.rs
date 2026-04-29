use crate::client::MikromClient;
use crate::config::Config;
use anyhow::{Context, Result};
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

pub async fn handle_health(client: &MikromClient) -> Result<()> {
    let resp = client.health().await.context("Health check failed")?;
    println!("status:  {}", resp.status);
    println!("version: {}", resp.version);
    Ok(())
}

pub async fn handle_auth(
    client: &MikromClient,
    cmd: crate::AuthCommands,
    cfg: &mut Config,
) -> Result<()> {
    match cmd {
        crate::AuthCommands::Register { email, password } => {
            let resp = client
                .register(&email, &password)
                .await
                .context("Registration failed")?;
            println!("{}", resp.message);
            println!("user_id: {}", resp.user_id);
        },
        crate::AuthCommands::Login { email, password } => {
            let resp = client
                .login(&email, &password)
                .await
                .context("Login failed")?;
            cfg.token = Some(resp.token);
            cfg.save().context("Failed to save config")?;
            println!("Logged in. Token saved to ~/.config/mikrom/config.toml");
        },
    }
    Ok(())
}

pub async fn handle_deploy(
    client: &MikromClient,
    app: String,
    image: String,
    vcpus: Option<u32>,
    memory: Option<u64>,
    disk: Option<u64>,
    env: Vec<String>,
) -> Result<()> {
    let env_map = parse_env_vars(&env).context("Invalid --env value (expected KEY=VALUE)")?;
    let resp = client
        .deploy(&app, &image, vcpus, memory, disk, env_map)
        .await
        .context("Deploy failed")?;

    if let Some(job_id) = resp.job_id {
        println!("{} {}", green_label("job_id:"), job_id);
    }
    if let Some(dep_id) = resp.deployment_id {
        println!("{} {}", green_label("deployment_id:"), dep_id);
    }

    println!(
        "{} {}",
        green_label("status:"),
        Paint::new(&resp.status).cyan()
    );
    println!("{} {}", green_label("message:"), resp.message);
    if let Some(image) = resp.image_tag {
        println!("{} {}", green_label("image:"), image);
    }
    if let Some(host_id) = resp.host_id {
        println!("{} {}", green_label("host_id:"), host_id);
    }
    if let Some(vm_id) = resp.vm_id {
        println!("{} {}", green_label("deployment_id:"), vm_id);
    }
    Ok(())
}

pub async fn handle_list_deployments(client: &MikromClient) -> Result<()> {
    let deployments = client
        .list_active_deployments()
        .await
        .context("Failed to list deployments")?;
    if deployments.is_empty() {
        println!("{}", Paint::new("No active deployments found.").yellow());
    } else {
        println!(
            "{:<38} {:<12} {:<20} {:<38} {}",
            bold_cyan("JOB_ID"),
            bold_cyan("STATUS"),
            bold_cyan("APP_NAME"),
            bold_cyan("DEPLOYMENT_ID"),
            bold_cyan("IMAGE")
        );
        println!("{}", "-".repeat(120));
        for dep in deployments {
            let status_painted = match dep.status.as_str() {
                "Running" => Paint::new(&dep.status).green(),
                "Scheduled" | "Pending" | "Building" => Paint::new(&dep.status).yellow(),
                "Failed" | "Error" => Paint::new(&dep.status).red(),
                _ => Paint::new(&dep.status),
            };
            println!(
                "{:<38} {:<12} {:<20} {:<38} {}",
                Paint::new(&dep.job_id).bold(),
                status_painted,
                Paint::new(&dep.app_name),
                Paint::new(&dep.deployment_id),
                Paint::new(&dep.image)
            );
        }
    }
    Ok(())
}

pub async fn handle_get_status(
    client: &MikromClient,
    app_name: String,
    job_id: String,
) -> Result<()> {
    let status = client
        .get_deployment_status(&app_name, &job_id)
        .await
        .context("Failed to get deployment status")?;
    println!("{}", bold_cyan("Live Deployment Detail"));
    println!("  {}       {}", green_label("Job ID:"), status.job_id);
    println!(
        "  {}       {}",
        green_label("Status:"),
        Paint::new(&status.status).cyan()
    );
    println!("  {}       {}", green_label("Worker ID:"), status.host_id);
    println!(
        "  {}       {}",
        green_label("Scheduled:"),
        format_timestamp(status.scheduled_at)
    );
    println!(
        "  {}       {}",
        green_label("Started:"),
        format_timestamp(status.started_at)
    );
    if status.stopped_at > 0 {
        println!(
            "  {}       {}",
            green_label("Stopped:"),
            format_timestamp(status.stopped_at)
        );
    }
    if !status.error_message.is_empty() {
        println!("  {}       {}", red_label("Error:"), status.error_message);
    }
    Ok(())
}

pub async fn handle_stop_instance(
    client: &MikromClient,
    app_name: String,
    job_id: String,
) -> Result<()> {
    let _ = client
        .stop_deployment(&app_name, &job_id)
        .await
        .context("Failed to stop deployment")?;
    println!(
        "{} {}",
        "Stopped deployment".green(),
        Paint::new(&format!("[{job_id}]")).cyan()
    );
    Ok(())
}

pub async fn handle_logs(
    _client: &MikromClient,
    app_name: String,
    job_id: String,
    follow: bool,
) -> Result<()> {
    if follow {
        // SSE follow is not implemented in MikromClient for CLI yet in a clean way,
        // but we'll use the existing stream logic if available or just a placeholder for now
        println!(
            "{}",
            Paint::new("--- Attaching to deployment log stream ---").dim()
        );
        // For now, CLI log streaming needs MikromClient to support it via reqwest::Response::bytes_stream()
        // We'll just print a message since the refactor focus is terminology
        println!("Streaming logs for {} in app {}...", job_id, app_name);
    } else {
        // Just fetch once
        println!("Fetching logs for {} in app {}...", job_id, app_name);
    }
    Ok(())
}

pub async fn handle_pause_instance(
    client: &MikromClient,
    app_name: String,
    job_id: String,
) -> Result<()> {
    let _ = client
        .pause_deployment(&app_name, &job_id)
        .await
        .context("Failed to pause deployment")?;
    println!(
        "{} {}",
        "Paused deployment".green(),
        Paint::new(&format!("[{job_id}]")).cyan()
    );
    Ok(())
}

pub async fn handle_resume_instance(
    client: &MikromClient,
    app_name: String,
    job_id: String,
) -> Result<()> {
    let _ = client
        .resume_deployment(&app_name, &job_id)
        .await
        .context("Failed to resume deployment")?;
    println!(
        "{} {}",
        "Resumed deployment".green(),
        Paint::new(&format!("[{job_id}]")).cyan()
    );
    Ok(())
}

pub async fn handle_delete_instance(
    client: &MikromClient,
    app_name: String,
    job_id: String,
) -> Result<()> {
    let _ = client
        .delete_deployment_record(&app_name, &job_id)
        .await
        .context("Failed to delete deployment record")?;
    println!(
        "{} {}",
        "Deleted deployment record".green(),
        Paint::new(&format!("[{job_id}]")).cyan()
    );
    Ok(())
}

pub async fn handle_restart_instance(
    _client: &MikromClient,
    _app_name: String,
    _job_id: String,
) -> Result<()> {
    println!("Restarting deployment is not directly supported. Please stop and deploy again.");
    Ok(())
}

pub async fn handle_metrics(
    _client: &MikromClient,
    _app_name: Option<String>,
    _job_id: Option<String>,
) -> Result<()> {
    println!("Metrics visualization in CLI is coming soon.");
    Ok(())
}

pub async fn handle_whoami(client: &MikromClient) -> Result<()> {
    let user = client.whoami().await.context("Failed to get user info")?;
    println!("user_id:   {}", user.user_id);
    println!("email:     {}", user.email);
    println!("created_at: {}", user.created_at);
    Ok(())
}

pub async fn handle_apps(client: &MikromClient, cmd: crate::AppCommands) -> Result<()> {
    match cmd {
        crate::AppCommands::List => {
            let apps = client.list_apps().await?;
            if apps.is_empty() {
                println!("No applications found.");
            } else {
                println!(
                    "{:<38} {:<20} {:<6} {:<10} {:<20} {:<30}",
                    "ID", "NAME", "PORT", "ACTIVE", "CREATED", "GIT URL"
                );
                println!("{}", "-".repeat(130));
                for app in apps {
                    let created = &app.created_at[0..10];
                    let active = app
                        .active_deployment_id
                        .as_ref()
                        .map(|id| &id[0..8])
                        .unwrap_or("None");
                    println!(
                        "{:<38} {:<20} {:<6} {:<10} {:<20} {:<30}",
                        app.id, app.name, app.port, active, created, app.git_url
                    );
                }
            }
        },
        crate::AppCommands::Create { name, git_url } => {
            let app = client.create_app(&name, &git_url).await?;
            println!("Application created: {} ({})", app.name, app.id);
            println!("Domain: {}", app.hostname.unwrap_or_default());
        },
        crate::AppCommands::Delete { app_id } => {
            client.delete_app(&app_id).await?;
            println!("Application {} deleted.", app_id);
        },
        crate::AppCommands::Deploy { app_id } => {
            let resp = client.deploy_app_version(&app_id).await?;
            if let Some(job_id) = resp.job_id {
                println!("Deployment started: {}", job_id);
            } else if let Some(dep_id) = resp.deployment_id {
                println!("Deployment initiated: {}", dep_id);
            }
            println!("Status: {}", resp.status);
        },
        crate::AppCommands::Activate {
            app_id,
            deployment_id,
        } => {
            client.activate_deployment(&app_id, &deployment_id).await?;
            println!(
                "{} Deployment {} is now active for app {}.",
                green_label("Success:"),
                deployment_id,
                app_id
            );
        },
    }
    Ok(())
}

fn format_timestamp(ts: i64) -> String {
    if ts == 0 {
        return "N/A".to_string();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Invalid".to_string())
}

pub fn parse_env_vars(env: &[String]) -> Result<HashMap<String, String>> {
    env.iter()
        .map(|s| {
            let (key, val) = s
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("'{s}' is not KEY=VALUE"))?;
            Ok((key.to_string(), val.to_string()))
        })
        .collect()
}
