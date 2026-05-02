use crate::client::MikromClient;
use crate::commands::DeploymentCommands;
use crate::ui;
use anyhow::Result;
use yansi::Paint;

pub async fn handle(client: &MikromClient, cmd: DeploymentCommands) -> Result<()> {
    match cmd {
        DeploymentCommands::List => list(client).await,
        DeploymentCommands::Status { app, job_id } => status(client, &app, &job_id).await,
        DeploymentCommands::Logs {
            app,
            job_id,
            follow,
        } => logs(&app, &job_id, follow),
        DeploymentCommands::Stop { app, job_id } => stop(client, &app, &job_id).await,
        DeploymentCommands::Pause { app, job_id } => pause(client, &app, &job_id).await,
        DeploymentCommands::Resume { app, job_id } => resume(client, &app, &job_id).await,
        DeploymentCommands::Delete { app, job_id } => delete(client, &app, &job_id).await,
        DeploymentCommands::Watch => watch(),
    }
}

async fn list(client: &MikromClient) -> Result<()> {
    let deployments = client.list_active_deployments().await?;
    if deployments.is_empty() {
        ui::info("No active deployments found.");
    } else {
        ui::step(ui::INFO, &ui::bold_cyan("Live Deployments (Jobs)"));
        for dep in deployments {
            let status_painted = match dep.status.as_str() {
                "Running" | "RUNNING" => Paint::new(&dep.status).green(),
                "Pending" | "Building" | "SCHEDULED" => Paint::new(&dep.status).yellow(),
                "Failed" | "FAILED" => Paint::new(&dep.status).red(),
                _ => Paint::new(&dep.status),
            };
            println!("\n{} {}", ui::ROCKET, ui::bold_cyan(&dep.app_name));
            ui::label_value(ui::DEP, "Job ID:", &dep.job_id);
            ui::label_value(ui::INFO, "Status:", &status_painted.to_string());
            ui::label_value(ui::APP, "Image:", &dep.image);
            ui::label_value(ui::SYS, "Host ID:", &dep.host_id);
        }
    }
    Ok(())
}

async fn status(client: &MikromClient, app: &str, job_id: &str) -> Result<()> {
    let status = client.get_deployment_status(app, job_id).await?;
    ui::step(ui::INFO, &ui::bold_cyan("Live Deployment Details"));
    ui::label_value(ui::APP, "App Name:", app);
    ui::label_value(ui::DEP, "Job ID:", &status.job_id);
    ui::label_value(ui::INFO, "Status:", &ui::cyan_label(&status.status));
    ui::label_value(ui::SYS, "Worker ID:", &status.host_id);
    ui::label_value(ui::SYS, "VM ID:", &status.vm_id);
    ui::label_value(
        ui::CLOCK,
        "Scheduled:",
        &format_timestamp(status.scheduled_at),
    );
    if status.started_at > 0 {
        ui::label_value(ui::CLOCK, "Started:", &format_timestamp(status.started_at));
    }
    if !status.error_message.is_empty() {
        ui::label_value(ui::ERROR, "Error:", &ui::red_label(&status.error_message));
    }
    Ok(())
}

fn logs(app: &str, job_id: &str, follow: bool) -> Result<()> {
    if follow {
        ui::step(
            ui::WATCH,
            &format!("{} Tailing logs for {}/{}...", ui::INFO, app, job_id),
        );
        println!("     (Log streaming via SSE is currently under development)");
    } else {
        ui::step(
            ui::INFO,
            &format!("{} Fetching logs for {}/{}...", ui::INFO, app, job_id),
        );
        println!("     (Log retrieval is currently under development)");
    }
    Ok(())
}

async fn stop(client: &MikromClient, app: &str, job_id: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Stopping deployment {}/{}...", ui::PAUSE, app, job_id),
    );
    client.stop_deployment(app, job_id).await?;
    ui::success("Deployment stopped successfully.");
    Ok(())
}

async fn pause(client: &MikromClient, app: &str, job_id: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Pausing deployment {}/{}...", ui::PAUSE, app, job_id),
    );
    client.pause_deployment(app, job_id).await?;
    ui::success("Deployment paused successfully.");
    Ok(())
}

async fn resume(client: &MikromClient, app: &str, job_id: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Resuming deployment {}/{}...", ui::RESUME, app, job_id),
    );
    client.resume_deployment(app, job_id).await?;
    ui::success("Deployment resumed successfully.");
    Ok(())
}

async fn delete(client: &MikromClient, app: &str, job_id: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!(
            "{} Deleting deployment record {}/{}...",
            ui::ERROR,
            app,
            job_id
        ),
    );
    client.delete_deployment_record(app, job_id).await?;
    ui::success("Deployment record deleted successfully.");
    Ok(())
}

fn watch() -> Result<()> {
    ui::step(
        ui::WATCH,
        &format!(
            "{} Global cluster event monitoring is planned for a future update.",
            ui::INFO
        ),
    );
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
