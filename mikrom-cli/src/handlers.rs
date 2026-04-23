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
        println!("{} {}", green_label("instance_id:"), vm_id);
    }
    Ok(())
}

pub async fn handle_list_deployments(client: &MikromClient) -> Result<()> {
    let vms = client
        .list_vms()
        .await
        .context("Failed to list deployments")?;
    if vms.is_empty() {
        println!("{}", Paint::new("No deployments found.").yellow());
    } else {
        println!(
            "{} {} {} {} IMAGE",
            bold_cyan("JOB_ID"),
            bold_cyan("STATUS"),
            bold_cyan("APP_NAME"),
            bold_cyan("INSTANCE_ID")
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

pub async fn handle_get_status(client: &MikromClient, job_id: String) -> Result<()> {
    let vm = client
        .get_vm(&job_id)
        .await
        .context("Failed to get instance status")?;
    println!("Instance Detail:");
    println!("  Job ID:       {}", vm.job_id);
    println!("  Status:       {}", vm.status);
    println!("  Host ID:      {}", vm.host_id);
    println!("  Instance ID:  {}", vm.vm_id);
    println!("  Scheduled:    {}", format_timestamp(vm.scheduled_at));
    println!("  Started:      {}", format_timestamp(vm.started_at));
    if vm.stopped_at > 0 {
        println!("  Stopped:      {}", format_timestamp(vm.stopped_at));
    }
    if !vm.error_message.is_empty() {
        println!("  Error:        {}", red_label(&vm.error_message));
    }
    Ok(())
}

pub async fn handle_stop_instance(client: &MikromClient, job_id: String) -> Result<()> {
    let resp = client
        .stop_vm(&job_id)
        .await
        .context("Failed to stop instance")?;
    if resp.success {
        println!(
            "{} {}",
            "Stopped job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        anyhow::bail!("{} {}", red_label("Stop reported failure:"), resp.message);
    }
    Ok(())
}

pub async fn handle_logs(client: &MikromClient, job_id: String, follow: bool) -> Result<()> {
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
                            },
                            Err(e) => {
                                eprintln!("{}: {e}", Paint::new("Stream error").red());
                                break; // Break inner loop to reconnect
                            },
                        }
                    }
                },
                Err(e) => {
                    eprintln!("{}: {e}", Paint::new("Connection failed").red());
                },
            }

            println!(
                "{}",
                Paint::new("--- Connection lost, retrying in 2s... ---").dim()
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    } else {
        let logs = client
            .get_vm_logs(&job_id)
            .await
            .context("Failed to get logs")?;
        println!("{logs}");
    }
    Ok(())
}

pub async fn handle_pause_instance(client: &MikromClient, job_id: String) -> Result<()> {
    let resp = client
        .pause_vm(&job_id)
        .await
        .context("Failed to pause instance")?;
    if resp.success {
        println!(
            "{} {}",
            "Paused job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        anyhow::bail!("{} {}", red_label("Pause reported failure:"), resp.message);
    }
    Ok(())
}

pub async fn handle_resume_instance(client: &MikromClient, job_id: String) -> Result<()> {
    let resp = client
        .resume_vm(&job_id)
        .await
        .context("Failed to resume instance")?;
    if resp.success {
        println!(
            "{} {}",
            "Resumed job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        anyhow::bail!("{} {}", red_label("Resume reported failure:"), resp.message);
    }
    Ok(())
}

pub async fn handle_delete_instance(client: &MikromClient, job_id: String) -> Result<()> {
    let resp = client
        .delete_vm(&job_id)
        .await
        .context("Failed to delete instance")?;
    if resp.success {
        println!(
            "{} {}",
            "Deleted job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        anyhow::bail!("{} {}", red_label("Delete reported failure:"), resp.message);
    }
    Ok(())
}

pub async fn handle_restart_instance(client: &MikromClient, job_id: String) -> Result<()> {
    let resp = client
        .restart_vm(&job_id)
        .await
        .context("Failed to restart instance")?;
    if resp.success {
        println!(
            "{} {}",
            "Restarting job".green(),
            Paint::new(&format!("[{job_id}]")).cyan()
        );
        println!("{} {}", green_label("message:"), resp.message);
    } else {
        anyhow::bail!(
            "{} {}",
            red_label("Restart reported failure:"),
            resp.message
        );
    }
    Ok(())
}

pub async fn handle_metrics(client: &MikromClient, job_id: Option<String>) -> Result<()> {
    match job_id {
        Some(id) => {
            let metrics = client
                .get_vm_metrics(&id)
                .await
                .context("Failed to get instance metrics")?;
            println!("Instance: {id}");
            println!("cpu_usage:    {:.2}%", metrics.cpu_usage);
            println!("memory:     {:.2}%", metrics.memory_usage);
            println!("disk:      {:.2}%", metrics.disk_usage);
            println!("network_rx: {} bytes", metrics.network_rx);
            println!("network_tx: {} bytes", metrics.network_tx);
        },
        None => {
            anyhow::bail!("job_id required for metrics command");
        },
    }
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
                    "{:<38} {:<20} {:<6} {:<20} {:<30}",
                    "ID", "NAME", "PORT", "CREATED", "GIT URL"
                );
                println!("{}", "-".repeat(120));
                for app in apps {
                    let created = &app.created_at[0..10]; // Just the date
                    println!(
                        "{:<38} {:<20} {:<6} {:<20} {:<30}",
                        app.id, app.name, app.port, created, app.git_url
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
