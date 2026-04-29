use crate::client::MikromClient;
use crate::config::Config;
use anyhow::{Context, Result};
use yansi::Paint;

// Emojis for better UX
const SUCCESS: &str = "✅";
const ERROR: &str = "❌";
const INFO: &str = "ℹ️";
const WAIT: &str = "⏳";
const ROCKET: &str = "🚀";
const PAUSE: &str = "⏸️";
const RESUME: &str = "▶️";
const KEY: &str = "🔑";
const APP: &str = "📦";
const DEP: &str = "🚢";
const SYS: &str = "⚙️";
const WATCH: &str = "👀";
const CLOCK: &str = "🕒";
const PORT: &str = "🔌";

fn bold_cyan(s: &str) -> String {
    Paint::new(s).cyan().bold().to_string()
}

fn green_label(s: &str) -> String {
    Paint::new(s).green().to_string()
}

fn red_label(s: &str) -> String {
    Paint::new(s).red().bold().to_string()
}

// ── Auth Handlers ──────────────────────────────────────────────────────────

pub async fn handle_auth(
    client: &MikromClient,
    cmd: crate::commands::AuthCommands,
    cfg: &mut Config,
) -> Result<()> {
    match cmd {
        crate::commands::AuthCommands::Register { email, password } => {
            println!(
                "{} {} Registering user {}...",
                WAIT,
                INFO,
                bold_cyan(&email)
            );
            let resp = client
                .register(&email, &password)
                .await
                .context("Registration failed")?;
            println!("{} {} {}", SUCCESS, green_label("Success:"), resp.message);
            println!("   {} User ID: {}", KEY, resp.user_id);
        },
        crate::commands::AuthCommands::Login { email, password } => {
            println!("{} {} Logging in as {}...", WAIT, KEY, bold_cyan(&email));
            let resp = client
                .login(&email, &password)
                .await
                .context("Login failed")?;
            cfg.token = Some(resp.token);
            cfg.save().context("Failed to save config")?;
            println!(
                "{} {} Logged in successfully. Token saved to config.",
                SUCCESS,
                green_label("Welcome!")
            );
        },
        crate::commands::AuthCommands::Whoami => {
            let user = client.whoami().await.context("Failed to get user info")?;
            println!("{} {}", INFO, bold_cyan("Current User Profile"));
            println!("  {} Email:      {}", INFO, user.email);
            println!("  {} User ID:    {}", KEY, user.user_id);
            if let Some(role) = user.role {
                println!("  {} Role:       {}", INFO, role);
            }
            if let (Some(f), Some(l)) = (user.first_name.as_ref(), user.last_name.as_ref()) {
                println!("  {} Name:       {} {}", INFO, f, l);
            } else if let Some(f) = user.first_name.as_ref() {
                println!("  {} First Name: {}", INFO, f);
            } else if let Some(l) = user.last_name.as_ref() {
                println!("  {} Last Name:  {}", INFO, l);
            }
            println!(
                "  {} Created At: {}",
                CLOCK,
                user.created_at.as_deref().unwrap_or("N/A")
            );
        },
        crate::commands::AuthCommands::Update {
            first_name,
            last_name,
        } => {
            println!("{} {} Updating profile...", WAIT, SYS);
            let user = client
                .update_profile(first_name, last_name)
                .await
                .context("Failed to update profile")?;
            println!(
                "{} {} Profile updated successfully.",
                SUCCESS,
                green_label("Success:")
            );
            println!("  {} Email:   {}", INFO, user.email);
            if let (Some(f), Some(l)) = (user.first_name.as_ref(), user.last_name.as_ref()) {
                println!("  {} Name:    {} {}", INFO, f, l);
            }
        },
    }
    Ok(())
}

// ── App Handlers ───────────────────────────────────────────────────────────

pub async fn handle_app(client: &MikromClient, cmd: crate::commands::AppCommands) -> Result<()> {
    match cmd {
        crate::commands::AppCommands::List => {
            let apps = client.list_apps().await?;
            if apps.is_empty() {
                println!("{} No applications found.", INFO);
            } else {
                println!("{} {}", INFO, bold_cyan("Registered Applications"));
                for app in apps {
                    let created = app.created_at.as_deref().unwrap_or("N/A");
                    let active = app.active_deployment_id.as_deref().unwrap_or("None");

                    println!("\n{} {}", APP, bold_cyan(&app.name));
                    println!("  {} APP_ID:     {}", KEY, app.id);
                    println!("  {} Port:       {}", PORT, app.port);
                    println!("  {} Active Dep: {}", DEP, active);
                    println!("  {} Created:    {}", CLOCK, created);
                }
            }
        },
        crate::commands::AppCommands::Create { name, git_url } => {
            println!("{} {} Creating app {}...", WAIT, APP, bold_cyan(&name));
            let app = client.create_app(&name, &git_url).await?;
            println!(
                "{} {} Application created successfully.",
                SUCCESS,
                green_label("Success:")
            );
            println!("  {} Name:       {}", APP, bold_cyan(&app.name));
            println!("  {} APP_ID:     {}", KEY, app.id);
            println!("  {} Git URL:    {}", INFO, app.git_url);
            if let Some(host) = app.hostname {
                println!("  {} Domain:     {}", INFO, host);
            }
        },
        crate::commands::AppCommands::Delete { name } => {
            println!(
                "{} {} Deleting application {}...",
                WAIT,
                APP,
                red_label(&name)
            );
            client.delete_app(&name).await?;
            println!("{} Application {} deleted.", SUCCESS, name);
        },
        crate::commands::AppCommands::Deploy { name } => {
            println!(
                "{} {} Triggering deployment for {}...",
                WAIT,
                ROCKET,
                bold_cyan(&name)
            );
            let resp = client.deploy_app_version(&name).await?;
            if let Some(job_id) = resp.job_id {
                println!(
                    "{} {} Deployment started. Job ID: {}",
                    SUCCESS,
                    ROCKET,
                    bold_cyan(&job_id)
                );
            } else if let Some(dep_id) = resp.deployment_id {
                println!(
                    "{} Deployment initiated. Deployment ID: {}",
                    SUCCESS,
                    bold_cyan(&dep_id)
                );
            }
            println!("  {} Status:     {}", INFO, Paint::new(&resp.status).cyan());
        },
        crate::commands::AppCommands::Activate { app, deployment_id } => {
            println!(
                "{} {} Activating deployment {} for app {}...",
                WAIT,
                DEP,
                bold_cyan(&deployment_id),
                bold_cyan(&app)
            );
            client.activate_deployment(&app, &deployment_id).await?;
            println!(
                "{} {} Deployment {} is now active.",
                SUCCESS,
                green_label("Success:"),
                deployment_id
            );
        },
        crate::commands::AppCommands::Deployments { name } => {
            let deployments = client.list_app_deployments(&name).await?;
            if deployments.is_empty() {
                println!("{} No deployments found for app {}.", INFO, name);
            } else {
                println!("{} {} Deployment History", INFO, bold_cyan(&name));
                for dep in deployments {
                    let status_painted = match dep.status.as_str() {
                        "Active" | "Succeeded" | "RUNNING" => Paint::new(&dep.status).green(),
                        "Pending" | "Building" | "SCHEDULED" => Paint::new(&dep.status).yellow(),
                        "Failed" | "FAILED" => Paint::new(&dep.status).red(),
                        _ => Paint::new(&dep.status),
                    };
                    let created = dep.created_at.as_deref().unwrap_or("N/A");

                    println!("\n{} Deployment {}", DEP, bold_cyan(&dep.id));
                    println!("  {} Status:     {}", INFO, status_painted);
                    println!(
                        "  {} Image Tag:  {}",
                        APP,
                        dep.image_tag.as_deref().unwrap_or("N/A")
                    );
                    println!("  {} Created:    {}", CLOCK, created);
                }
            }
        },
        crate::commands::AppCommands::Watch { name } => {
            println!(
                "{} {} Real-time deployment monitoring for {} is planned for a future update.",
                WATCH, INFO, name
            );
            println!(
                "     Use 'mikrom app deployments {}' to poll manually.",
                name
            );
        },
    }
    Ok(())
}

// ── Deployment Handlers ────────────────────────────────────────────────────

pub async fn handle_deployment(
    client: &MikromClient,
    cmd: crate::commands::DeploymentCommands,
) -> Result<()> {
    match cmd {
        crate::commands::DeploymentCommands::List => {
            let deployments = client.list_active_deployments().await?;
            if deployments.is_empty() {
                println!("{} No active deployments found.", INFO);
            } else {
                println!("{} {}", INFO, bold_cyan("Live Deployments (Jobs)"));
                for dep in deployments {
                    let status_painted = match dep.status.as_str() {
                        "Running" | "RUNNING" => Paint::new(&dep.status).green(),
                        "Pending" | "Building" | "SCHEDULED" => Paint::new(&dep.status).yellow(),
                        "Failed" | "FAILED" => Paint::new(&dep.status).red(),
                        _ => Paint::new(&dep.status),
                    };
                    println!("\n{} {}", ROCKET, bold_cyan(&dep.app_name));
                    println!("  {} Job ID:     {}", DEP, dep.job_id);
                    println!("  {} Status:     {}", INFO, status_painted);
                    println!("  {} Image:      {}", APP, dep.image);
                    println!("  {} Host ID:    {}", SYS, dep.host_id);
                }
            }
        },
        crate::commands::DeploymentCommands::Status { app, job_id } => {
            let status = client.get_deployment_status(&app, &job_id).await?;
            println!("{} {}", INFO, bold_cyan("Live Deployment Details"));
            println!("  {} App Name:    {}", APP, app);
            println!("  {} Job ID:      {}", DEP, status.job_id);
            println!(
                "  {} Status:      {}",
                INFO,
                Paint::new(&status.status).cyan()
            );
            println!("  {} Worker ID:   {}", SYS, status.host_id);
            println!("  {} VM ID:       {}", SYS, status.vm_id);
            println!(
                "  {} Scheduled:   {}",
                CLOCK,
                format_timestamp(status.scheduled_at)
            );
            if status.started_at > 0 {
                println!(
                    "  {} Started:     {}",
                    CLOCK,
                    format_timestamp(status.started_at)
                );
            }
            if !status.error_message.is_empty() {
                println!(
                    "  {} Error:       {}",
                    ERROR,
                    red_label(&status.error_message)
                );
            }
        },
        crate::commands::DeploymentCommands::Logs {
            app,
            job_id,
            follow,
        } => {
            if follow {
                println!("{} {} Tailing logs for {}/{}...", WATCH, INFO, app, job_id);
                println!("     (Log streaming via SSE is currently under development)");
            } else {
                println!("{} {} Fetching logs for {}/{}...", INFO, INFO, app, job_id);
                println!("     (Log retrieval is currently under development)");
            }
        },
        crate::commands::DeploymentCommands::Stop { app, job_id } => {
            println!(
                "{} {} Stopping deployment {}/{}...",
                WAIT, PAUSE, app, job_id
            );
            client.stop_deployment(&app, &job_id).await?;
            println!("{} Deployment stopped successfully.", SUCCESS);
        },
        crate::commands::DeploymentCommands::Pause { app, job_id } => {
            println!(
                "{} {} Pausing deployment {}/{}...",
                WAIT, PAUSE, app, job_id
            );
            client.pause_deployment(&app, &job_id).await?;
            println!("{} Deployment paused successfully.", SUCCESS);
        },
        crate::commands::DeploymentCommands::Resume { app, job_id } => {
            println!(
                "{} {} Resuming deployment {}/{}...",
                WAIT, RESUME, app, job_id
            );
            client.resume_deployment(&app, &job_id).await?;
            println!("{} Deployment resumed successfully.", SUCCESS);
        },
        crate::commands::DeploymentCommands::Delete { app, job_id } => {
            println!(
                "{} {} Deleting deployment record {}/{}...",
                WAIT, ERROR, app, job_id
            );
            client.delete_deployment_record(&app, &job_id).await?;
            println!("{} Deployment record deleted successfully.", SUCCESS);
        },
        crate::commands::DeploymentCommands::Watch => {
            println!(
                "{} {} Global cluster event monitoring is planned for a future update.",
                WATCH, INFO
            );
        },
    }
    Ok(())
}

// ── Config Handlers ────────────────────────────────────────────────────────

pub async fn handle_config(cmd: crate::commands::ConfigCommands, cfg: &mut Config) -> Result<()> {
    match cmd {
        crate::commands::ConfigCommands::Show => {
            println!("{} {}", INFO, bold_cyan("CLI Configuration"));
            println!("  {} API URL:     {}", SYS, cfg.api_url());
            if cfg.token.is_some() {
                println!(
                    "  {} Token:       {}",
                    KEY,
                    Paint::new("[Configured]").green()
                );
            } else {
                println!(
                    "  {} Token:       {}",
                    KEY,
                    Paint::new("[Not Set]").yellow()
                );
            }
        },
        crate::commands::ConfigCommands::Set { key, value } => match key.as_str() {
            "api-url" | "api_url" => {
                cfg.api_url = Some(value.clone());
                cfg.save()?;
                println!(
                    "{} API URL updated to {}",
                    SUCCESS,
                    Paint::new(&value).cyan()
                );
            },
            _ => {
                println!("{} Unknown config key: {}", ERROR, key);
            },
        },
    }
    Ok(())
}

// ── System Handlers ────────────────────────────────────────────────────────

pub async fn handle_system(
    client: &MikromClient,
    cmd: crate::commands::SystemCommands,
) -> Result<()> {
    match cmd {
        crate::commands::SystemCommands::Health => {
            let health = client.health().await?;
            println!("{} {}", INFO, bold_cyan("System Health Status"));
            println!(
                "  {} Status:      {}",
                INFO,
                Paint::new(&health.status).green()
            );
            println!("  {} Version:     {}", INFO, health.version);
            println!("\n  {}", bold_cyan("Services:"));
            for (name, status) in health.services {
                let status_painted = if status == "ONLINE" {
                    Paint::new(&status).green()
                } else {
                    Paint::new(&status).red()
                };
                println!("    {:<12} {}", name, status_painted);
            }
        },
        crate::commands::SystemCommands::Watch => {
            println!(
                "{} {} Real-time system health dashboard is planned for a future update.",
                WATCH, INFO
            );
        },
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn format_timestamp(ts: i64) -> String {
    if ts == 0 {
        return "N/A".to_string();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Invalid".to_string())
}
