use crate::client::MikromClient;
use crate::commands::{DeploymentCommands, OutputFormat};
use crate::ui;
use anyhow::Result;

pub async fn handle(
    client: &MikromClient,
    cmd: DeploymentCommands,
    output: OutputFormat,
) -> Result<()> {
    match cmd {
        DeploymentCommands::List => list(client, output).await,
        DeploymentCommands::Status { app, job_id } => status(client, &app, &job_id, output).await,
        DeploymentCommands::Logs {
            app,
            job_id,
            follow,
        } => logs(&app, &job_id, follow),
        DeploymentCommands::Stop { app, job_id } => stop(client, &app, &job_id, output).await,
        DeploymentCommands::Pause { app, job_id } => pause(client, &app, &job_id, output).await,
        DeploymentCommands::Resume { app, job_id } => resume(client, &app, &job_id, output).await,
        DeploymentCommands::Delete { app, job_id } => delete(client, &app, &job_id, output).await,
        DeploymentCommands::Watch => watch(),
    }
}

async fn list(client: &MikromClient, output: OutputFormat) -> Result<()> {
    let deployments = client.list_active_deployments().await?;
    if output == OutputFormat::Json {
        return ui::print_json(&deployments);
    }

    if deployments.is_empty() {
        ui::info("No active deployments found.");
    } else {
        let rows = deployments
            .iter()
            .map(|dep| {
                vec![
                    format!("{} {}", ui::ROCKET, ui::bold_cyan(&dep.app_name)),
                    dep.job_id.clone(),
                    ui::status_label(&dep.status),
                    dep.ipv6_address.as_deref().unwrap_or("—").to_string(),
                    dep.host_id.clone(),
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            "🚀 Live Deployments",
            &["App", "Job", "Status", "IPv6", "Host"],
            &rows,
        );
    }
    Ok(())
}

async fn status(
    client: &MikromClient,
    app: &str,
    job_id: &str,
    output: OutputFormat,
) -> Result<()> {
    let status = client.get_deployment_status(app, job_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&status);
    }

    ui::step(ui::INFO, &ui::bold_cyan("Live Deployment Details"));
    ui::table(
        "🚢 Deployment Status",
        &["Field", "Value"],
        &[
            vec!["App".to_string(), format!("{} {}", ui::APP, app)],
            vec!["Job".to_string(), status.job_id.clone()],
            vec!["Status".to_string(), ui::status_label(&status.status)],
            vec!["Worker".to_string(), status.host_id.clone()],
            vec!["VM".to_string(), status.vm_id.clone()],
            vec![
                "Scheduled".to_string(),
                format_timestamp(status.scheduled_at),
            ],
            vec![
                "Started".to_string(),
                if status.started_at > 0 {
                    format_timestamp(status.started_at)
                } else {
                    "—".to_string()
                },
            ],
        ],
    );
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

async fn stop(client: &MikromClient, app: &str, job_id: &str, output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Stopping deployment {}/{}...", ui::PAUSE, app, job_id),
        );
    }
    client.stop_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "stopped": true, "app": app, "job_id": job_id }),
        );
    }

    ui::success("Deployment stopped successfully.");
    Ok(())
}

async fn pause(client: &MikromClient, app: &str, job_id: &str, output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Pausing deployment {}/{}...", ui::PAUSE, app, job_id),
        );
    }
    client.pause_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "paused": true, "app": app, "job_id": job_id }),
        );
    }

    ui::success("Deployment paused successfully.");
    Ok(())
}

async fn resume(
    client: &MikromClient,
    app: &str,
    job_id: &str,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Resuming deployment {}/{}...", ui::RESUME, app, job_id),
        );
    }
    client.resume_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "resumed": true, "app": app, "job_id": job_id }),
        );
    }

    ui::success("Deployment resumed successfully.");
    Ok(())
}

async fn delete(
    client: &MikromClient,
    app: &str,
    job_id: &str,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Deleting deployment record {}/{}...",
                ui::ERROR,
                app,
                job_id
            ),
        );
    }
    client.delete_deployment_record(app, job_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "deleted": true, "app": app, "job_id": job_id }),
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timestamp_returns_na_for_zero() {
        assert_eq!(format_timestamp(0), "N/A");
    }

    #[test]
    fn format_timestamp_formats_unix_timestamp() {
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn format_timestamp_returns_invalid_for_out_of_range_values() {
        assert_eq!(format_timestamp(i64::MAX), "Invalid");
    }
}
