use crate::application::context::CliContext;
use crate::commands::{DeploymentCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::infrastructure::ui;
use crate::output::{format_timestamp, print_json};

pub async fn handle(
    ctx: &CliContext,
    cmd: DeploymentCommands,
    output: OutputFormat,
) -> CliResult<()> {
    match cmd {
        DeploymentCommands::List => list(ctx, output).await,
        DeploymentCommands::Status { app, job_id } => status(ctx, &app, &job_id, output).await,
        DeploymentCommands::Logs {
            app,
            job_id,
            follow: _,
        } => {
            ui::info(&format!(
                "Streaming live logs for job '{}' of app '{}' (Ctrl+C to stop)...",
                job_id, app
            ));
            ctx.client.stream_deployment_logs(&app, &job_id).await
        },
        DeploymentCommands::Stop { app, job_id } => stop(ctx, &app, &job_id, output).await,
        DeploymentCommands::Pause { app, job_id } => pause(ctx, &app, &job_id, output).await,
        DeploymentCommands::Resume { app, job_id } => resume(ctx, &app, &job_id, output).await,
        DeploymentCommands::Delete { app, job_id, yes } => {
            delete(ctx, &app, &job_id, yes, output).await
        },
        DeploymentCommands::Snapshots { app, job_id } => {
            snapshots(ctx, &app, &job_id, output).await
        },
        DeploymentCommands::SnapshotCreate { app, job_id, name } => {
            snapshot_create(ctx, &app, &job_id, &name, output).await
        },
        DeploymentCommands::SnapshotRestore {
            app,
            job_id,
            snapshot,
        } => snapshot_restore(ctx, &app, &job_id, &snapshot, output).await,
        DeploymentCommands::SnapshotDelete {
            app,
            job_id,
            snapshot,
        } => snapshot_delete(ctx, &app, &job_id, &snapshot, output).await,
    }
}

async fn snapshots(
    ctx: &CliContext,
    app: &str,
    job_id: &str,
    output: OutputFormat,
) -> CliResult<()> {
    let resp = ctx.client.list_vm_snapshots(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    if !resp.success {
        ui::error(&resp.message);
    }

    if resp.snapshots.is_empty() {
        ui::info("No VM snapshots found.");
    } else {
        let rows = resp
            .snapshots
            .iter()
            .map(|s| {
                let size_str = if s.size_bytes >= 1024 * 1024 * 1024 {
                    format!(
                        "{:.1} GiB",
                        s.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                    )
                } else if s.size_bytes >= 1024 * 1024 {
                    format!("{:.1} MiB", s.size_bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{:.1} KiB", s.size_bytes as f64 / 1024.0)
                };
                let datetime = chrono::DateTime::from_timestamp(s.created_at, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| s.created_at.to_string());
                vec![
                    s.id.clone(),
                    s.name.clone(),
                    ui::status_label(&s.vm_status),
                    size_str,
                    datetime,
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            &format!("📸 VM Snapshots for {}/{}", app, job_id),
            &["ID", "Name", "VM Status", "Size", "Created At"],
            &rows,
        );
    }
    Ok(())
}

async fn snapshot_create(
    ctx: &CliContext,
    app: &str,
    job_id: &str,
    name: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Creating VM snapshot '{}' for {}/{}...",
                ui::ROCKET,
                name,
                app,
                job_id
            ),
        );
    }
    let resp = ctx.client.create_vm_snapshot(app, job_id, name).await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    if resp.success {
        ui::success(&format!(
            "VM Snapshot '{}' created successfully: {}",
            name, resp.message
        ));
    } else {
        ui::error(&format!("Failed to create VM snapshot: {}", resp.message));
    }
    Ok(())
}

async fn snapshot_restore(
    ctx: &CliContext,
    app: &str,
    job_id: &str,
    snapshot: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Restoring VM {}/{} to snapshot '{}'...",
                ui::RESUME,
                app,
                job_id,
                snapshot
            ),
        );
    }
    let resp = ctx
        .client
        .restore_vm_snapshot(app, job_id, snapshot)
        .await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    if resp.success {
        ui::success(&format!(
            "VM restored to snapshot '{}' successfully: {}",
            snapshot, resp.message
        ));
    } else {
        ui::error(&format!("Failed to restore VM snapshot: {}", resp.message));
    }
    Ok(())
}

async fn snapshot_delete(
    ctx: &CliContext,
    app: &str,
    job_id: &str,
    snapshot: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Deleting VM snapshot '{}' for {}/{}...",
                ui::ERROR,
                snapshot,
                app,
                job_id
            ),
        );
    }
    let resp = ctx.client.delete_vm_snapshot(app, job_id, snapshot).await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    if resp.success {
        ui::success(&format!(
            "VM Snapshot '{}' deleted successfully: {}",
            snapshot, resp.message
        ));
    } else {
        ui::error(&format!("Failed to delete VM snapshot: {}", resp.message));
    }
    Ok(())
}

async fn list(ctx: &CliContext, output: OutputFormat) -> CliResult<()> {
    let deployments = ctx.client.list_active_deployments().await?;
    if output == OutputFormat::Json {
        print_json(&deployments);
        return Ok(());
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
                    dep.hypervisor
                        .as_deref()
                        .unwrap_or("unspecified")
                        .to_string(),
                    dep.ipv6_address.as_deref().unwrap_or("—").to_string(),
                    dep.host_id.clone(),
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            "🚀 Live Deployments",
            &["App", "Job", "Status", "Hypervisor", "IPv6", "Host"],
            &rows,
        );
    }
    Ok(())
}

async fn status(ctx: &CliContext, app: &str, job_id: &str, output: OutputFormat) -> CliResult<()> {
    let status = ctx.client.get_deployment_status(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&status);
        return Ok(());
    }

    ui::step(ui::INFO, &ui::bold_cyan("Live Deployment Details"));
    ui::table(
        "🚢 Deployment Status",
        &["Field", "Value"],
        &[
            vec!["App".to_string(), format!("{} {}", ui::APP, app)],
            vec!["Job".to_string(), status.job_id.clone()],
            vec!["Status".to_string(), ui::status_label(&status.status)],
            vec![
                "Hypervisor".to_string(),
                status
                    .hypervisor
                    .as_deref()
                    .unwrap_or("unspecified")
                    .to_string(),
            ],
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

async fn stop(ctx: &CliContext, app: &str, job_id: &str, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Stopping deployment {}/{}...", ui::PAUSE, app, job_id),
        );
    }
    ctx.client.stop_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "stopped": true, "app": app, "job_id": job_id }));
        return Ok(());
    }

    ui::success("Deployment stopped successfully.");
    Ok(())
}

async fn pause(ctx: &CliContext, app: &str, job_id: &str, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Pausing deployment {}/{}...", ui::PAUSE, app, job_id),
        );
    }
    ctx.client.pause_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "paused": true, "app": app, "job_id": job_id }));
        return Ok(());
    }

    ui::success("Deployment paused successfully.");
    Ok(())
}

async fn resume(ctx: &CliContext, app: &str, job_id: &str, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Resuming deployment {}/{}...", ui::RESUME, app, job_id),
        );
    }
    ctx.client.resume_deployment(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "resumed": true, "app": app, "job_id": job_id }));
        return Ok(());
    }

    ui::success("Deployment resumed successfully.");
    Ok(())
}

async fn delete(
    ctx: &CliContext,
    app: &str,
    job_id: &str,
    yes: bool,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table
        && !yes
        && !super::app::confirm(&format!(
            "Are you sure you want to delete deployment record '{}/{}'?",
            app, job_id
        ))?
    {
        return Err(crate::domain::error::CliError::Cancelled);
    }
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
    ctx.client.delete_deployment_record(app, job_id).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "deleted": true, "app": app, "job_id": job_id }));
        return Ok(());
    }

    ui::success("Deployment record deleted successfully.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::error::CliError;
    use crate::domain::models::{LiveDeploymentInfo, LiveDeploymentStatus};
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn list_returns_deployments_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_list_active_deployments()
            .times(1)
            .returning(|| {
                Ok(vec![LiveDeploymentInfo {
                    job_id: "job-1".to_string(),
                    app_name: "svc".to_string(),
                    image: "nginx".to_string(),
                    status: "RUNNING".to_string(),
                    host_id: "host-1".to_string(),
                    ipv6_address: Some("fd00::1".to_string()),
                    hypervisor: Some("firecracker".to_string()),
                }])
            });
        let ctx = test_ctx(mock);
        let result = list(&ctx, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn list_empty_shows_info_in_table_mode() {
        let mut mock = MockApiClient::new();
        mock.expect_list_active_deployments()
            .times(1)
            .returning(|| Ok(vec![]));
        let ctx = test_ctx(mock);
        let result = list(&ctx, OutputFormat::Table).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn status_returns_deployment_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_get_deployment_status()
            .times(1)
            .returning(|_, _| {
                Ok(LiveDeploymentStatus {
                    job_id: "job-1".to_string(),
                    status: "RUNNING".to_string(),
                    host_id: "host-1".to_string(),
                    vm_id: "vm-1".to_string(),
                    scheduled_at: 1_700_000_000,
                    started_at: 1_700_000_010,
                    error_message: "".to_string(),
                    ipv6_address: Some("fd00::1".to_string()),
                    hypervisor: Some("firecracker".to_string()),
                })
            });
        let ctx = test_ctx(mock);
        let result = status(&ctx, "svc", "job-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn status_with_error_message_renders_table() {
        let mut mock = MockApiClient::new();
        mock.expect_get_deployment_status()
            .times(1)
            .returning(|_, _| {
                Ok(LiveDeploymentStatus {
                    job_id: "job-1".to_string(),
                    status: "FAILED".to_string(),
                    host_id: "host-1".to_string(),
                    vm_id: "vm-1".to_string(),
                    scheduled_at: 1_700_000_000,
                    started_at: 0,
                    error_message: "OOM killed".to_string(),
                    ipv6_address: None,
                    hypervisor: None,
                })
            });
        let ctx = test_ctx(mock);
        let result = status(&ctx, "svc", "job-1", OutputFormat::Table).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn stop_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_stop_deployment()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
            )
            .times(1)
            .returning(|_, _| Ok(serde_json::json!({"stopped": true})));
        let ctx = test_ctx(mock);
        let result = stop(&ctx, "svc", "job-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn pause_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_pause_deployment()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
            )
            .times(1)
            .returning(|_, _| Ok(serde_json::json!({"paused": true})));
        let ctx = test_ctx(mock);
        let result = pause(&ctx, "svc", "job-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn resume_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_resume_deployment()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
            )
            .times(1)
            .returning(|_, _| Ok(serde_json::json!({"resumed": true})));
        let ctx = test_ctx(mock);
        let result = resume(&ctx, "svc", "job-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_delete_deployment_record()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
            )
            .times(1)
            .returning(|_, _| Ok(serde_json::json!({"deleted": true})));
        let ctx = test_ctx(mock);
        let result = delete(&ctx, "svc", "job-1", true, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn stop_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_stop_deployment().times(1).returning(|_, _| {
            Err(CliError::NotFound {
                resource: "deployment".to_string(),
                id: "job-1".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = stop(&ctx, "svc", "job-1", OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::NotFound { .. })));
    }

    #[tokio::test]
    async fn handle_routes_list_command() {
        let mut mock = MockApiClient::new();
        mock.expect_list_active_deployments()
            .times(1)
            .returning(|| Ok(vec![]));
        let ctx = test_ctx(mock);
        let result = handle(&ctx, DeploymentCommands::List, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_routes_status_command() {
        let mut mock = MockApiClient::new();
        mock.expect_get_deployment_status()
            .times(1)
            .returning(|_, _| {
                Ok(LiveDeploymentStatus {
                    job_id: "job-1".to_string(),
                    status: "RUNNING".to_string(),
                    host_id: "host-1".to_string(),
                    vm_id: "vm-1".to_string(),
                    scheduled_at: 0,
                    started_at: 0,
                    error_message: "".to_string(),
                    ipv6_address: None,
                    hypervisor: None,
                })
            });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DeploymentCommands::Status {
                app: "svc".to_string(),
                job_id: "job-1".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshots_calls_api_and_lists_snapshots() {
        let mut mock = MockApiClient::new();
        mock.expect_list_vm_snapshots()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
            )
            .times(1)
            .returning(|_, _| {
                Ok(crate::domain::models::DeploymentSnapshotListResponse {
                    success: true,
                    message: "ok".to_string(),
                    snapshots: vec![crate::domain::models::DeploymentSnapshot {
                        id: "snap-123".to_string(),
                        name: "my-snap".to_string(),
                        created_at: 1717891200,
                        size_bytes: 1024 * 1024,
                        vm_status: "running".to_string(),
                    }],
                })
            });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DeploymentCommands::Snapshots {
                app: "svc".to_string(),
                job_id: "job-1".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_create_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_create_vm_snapshot()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
                mockall::predicate::eq("my-snap"),
            )
            .times(1)
            .returning(|_, _, _| {
                Ok(crate::domain::models::DeploymentSnapshotActionResponse {
                    success: true,
                    message: "created".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DeploymentCommands::SnapshotCreate {
                app: "svc".to_string(),
                job_id: "job-1".to_string(),
                name: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_restore_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_restore_vm_snapshot()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
                mockall::predicate::eq("my-snap"),
            )
            .times(1)
            .returning(|_, _, _| {
                Ok(crate::domain::models::DeploymentSnapshotActionResponse {
                    success: true,
                    message: "restored".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DeploymentCommands::SnapshotRestore {
                app: "svc".to_string(),
                job_id: "job-1".to_string(),
                snapshot: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_delete_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_delete_vm_snapshot()
            .with(
                mockall::predicate::eq("svc"),
                mockall::predicate::eq("job-1"),
                mockall::predicate::eq("my-snap"),
            )
            .times(1)
            .returning(|_, _, _| {
                Ok(crate::domain::models::DeploymentSnapshotActionResponse {
                    success: true,
                    message: "deleted".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DeploymentCommands::SnapshotDelete {
                app: "svc".to_string(),
                job_id: "job-1".to_string(),
                snapshot: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
