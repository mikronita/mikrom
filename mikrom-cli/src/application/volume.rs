use crate::application::context::CliContext;
use crate::commands::{OutputFormat, VolumeCommands};
use crate::domain::error::CliResult;
use crate::infrastructure::ui;
use crate::output::print_json;

pub async fn handle(ctx: &CliContext, cmd: VolumeCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        VolumeCommands::List { app } => list(ctx, app, output).await,
        VolumeCommands::Create { name, size } => create(ctx, &name, size, output).await,
        VolumeCommands::Attach {
            app,
            volume_id,
            mount,
            mode,
        } => attach(ctx, &app, &volume_id, &mount, mode, output).await,
        VolumeCommands::Detach { app, volume_id } => detach(ctx, &app, &volume_id, output).await,
        VolumeCommands::Snapshot { volume_id, name } => {
            snapshot(ctx, &volume_id, &name, output).await
        },
        VolumeCommands::Restore {
            volume_id,
            snapshot,
        } => restore(ctx, &volume_id, &snapshot, output).await,
        VolumeCommands::Delete { volume_id, yes } => delete(ctx, &volume_id, yes, output).await,
    }
}

async fn list(ctx: &CliContext, app_name: Option<String>, output: OutputFormat) -> CliResult<()> {
    if let Some(name) = app_name {
        let app = ctx.client.get_app(&name).await?;
        let volumes = ctx.client.list_volumes(&app.id).await?;

        if output == OutputFormat::Json {
            print_json(&volumes);
            return Ok(());
        }

        if volumes.is_empty() {
            ui::info(&format!("No volumes attached to app {}.", name));
        } else {
            let rows = volumes
                .iter()
                .map(|vol| {
                    vec![
                        vol.volume.name.clone(),
                        vol.volume.id.clone(),
                        format!("{} MiB", vol.volume.size_mib),
                        vol.mount_point.clone(),
                        match vol.access_mode {
                            0 => "RWO (Single Node)".to_string(),
                            1 => "RWX (Shared Mesh)".to_string(),
                            2 => "ROX (Shared Read)".to_string(),
                            _ => "Unknown".to_string(),
                        },
                        vol.volume.created_at.clone(),
                    ]
                })
                .collect::<Vec<_>>();
            ui::table(
                &format!("💾 Volumes for {}", ui::bold_cyan(&name)),
                &["Name", "ID", "Size", "Mount", "Mode", "Created"],
                &rows,
            );
        }
    } else {
        let volumes = ctx.client.list_all_volumes().await?;

        if output == OutputFormat::Json {
            print_json(&volumes);
            return Ok(());
        }

        if volumes.is_empty() {
            ui::info("No volumes found.");
        } else {
            let rows = volumes
                .iter()
                .map(|vwa| {
                    let attachments = vwa
                        .attachments
                        .iter()
                        .map(|a| {
                            format!(
                                "{} ({})",
                                a.app_name,
                                match a.access_mode {
                                    0 => "RWO",
                                    1 => "RWX",
                                    2 => "ROX",
                                    _ => "??",
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");

                    vec![
                        vwa.volume.name.clone(),
                        vwa.volume.id.clone(),
                        format!("{} MiB", vwa.volume.size_mib),
                        if attachments.is_empty() {
                            "---".to_string()
                        } else {
                            attachments
                        },
                        vwa.volume.created_at.clone(),
                    ]
                })
                .collect::<Vec<_>>();
            ui::table(
                "💾 All Volumes",
                &["Name", "ID", "Size", "Attached To", "Created"],
                &rows,
            );
        }
    };

    Ok(())
}

async fn create(ctx: &CliContext, name: &str, size: i32, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("Creating volume {}...", ui::bold_cyan(name)),
        );
    }

    let volume = ctx.client.create_volume(name, size).await?;

    if output == OutputFormat::Json {
        print_json(&volume);
        return Ok(());
    }

    ui::success(&format!("Volume created: {} ({})", volume.name, volume.id));
    Ok(())
}

async fn attach(
    ctx: &CliContext,
    app_name: &str,
    volume_id: &str,
    mount_point: &str,
    access_mode: i32,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "Attaching volume {} to {} at {}...",
                ui::bold_cyan(volume_id),
                ui::bold_cyan(app_name),
                ui::bold_cyan(mount_point)
            ),
        );
    }

    let app = ctx.client.get_app(app_name).await?;
    let attachment = ctx
        .client
        .attach_volume(&app.id, volume_id, mount_point, access_mode)
        .await?;

    if output == OutputFormat::Json {
        print_json(&attachment);
        return Ok(());
    }

    ui::success(&format!(
        "Volume attached successfully to {}.",
        ui::bold_cyan(app_name)
    ));
    Ok(())
}

async fn detach(
    ctx: &CliContext,
    app_name: &str,
    volume_id: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "Detaching volume {} from {}...",
                ui::bold_cyan(volume_id),
                ui::bold_cyan(app_name)
            ),
        );
    }

    let app = ctx.client.get_app(app_name).await?;
    ctx.client.detach_volume(&app.id, volume_id).await?;

    if output == OutputFormat::Json {
        print_json(
            &serde_json::json!({ "detached": true, "app": app_name, "volume_id": volume_id }),
        );
        return Ok(());
    }

    ui::success(&format!(
        "Volume detached successfully from {}.",
        ui::bold_cyan(app_name)
    ));
    Ok(())
}

async fn snapshot(
    ctx: &CliContext,
    volume_id: &str,
    name: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "Creating snapshot {} for volume {}...",
                ui::bold_cyan(name),
                ui::bold_cyan(volume_id)
            ),
        );
    }

    let snap = ctx.client.create_volume_snapshot(volume_id, name).await?;

    if output == OutputFormat::Json {
        print_json(&snap);
        return Ok(());
    }

    ui::success(&format!("Snapshot created: {} ({})", snap.name, snap.id));
    Ok(())
}

async fn restore(
    ctx: &CliContext,
    volume_id: &str,
    snapshot_name: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "Restoring volume {} to snapshot {}...",
                ui::bold_cyan(volume_id),
                ui::bold_cyan(snapshot_name)
            ),
        );
    }

    ctx.client
        .restore_volume_snapshot(volume_id, snapshot_name)
        .await?;

    if output == OutputFormat::Json {
        print_json(
            &serde_json::json!({ "restored": true, "volume_id": volume_id, "snapshot": snapshot_name }),
        );
        return Ok(());
    }

    ui::success(&format!(
        "Volume {} restored to snapshot {}.",
        volume_id, snapshot_name
    ));
    Ok(())
}

async fn delete(
    ctx: &CliContext,
    volume_id: &str,
    yes: bool,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table
        && !yes
        && !super::app::confirm(&format!(
            "Are you sure you want to delete volume '{}'?",
            volume_id
        ))?
    {
        return Err(crate::domain::error::CliError::Cancelled);
    }
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("Deleting volume {}...", ui::red_label(volume_id)),
        );
    }

    ctx.client.delete_volume(volume_id).await?;

    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "deleted": true, "volume_id": volume_id }));
        return Ok(());
    }

    ui::success(&format!("Volume {} deleted.", volume_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::error::CliError;
    use crate::domain::models::{AppInfo, Volume};
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn list_uses_get_app_when_app_filter_provided() {
        let mut mock = MockApiClient::new();
        mock.expect_get_app()
            .with(mockall::predicate::eq("svc"))
            .times(1)
            .returning(|_| {
                Ok(AppInfo {
                    id: "a1".to_string(),
                    name: "svc".to_string(),
                    git_url: "https://github.com/test/repo".to_string(),
                    port: 8080,
                    hostname: None,
                    active_deployment_id: None,
                    desired_replicas: 1,
                    min_replicas: 0,
                    max_replicas: 1,
                    autoscaling_enabled: false,
                    cpu_threshold: 80.0,
                    mem_threshold: 80.0,
                    created_at: None,
                })
            });
        mock.expect_list_volumes()
            .with(mockall::predicate::eq("a1"))
            .times(1)
            .returning(|_| Ok(vec![]));
        let ctx = test_ctx(mock);
        let result = list(&ctx, Some("svc".to_string()), OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn list_all_volumes_when_no_app_filter() {
        let mut mock = MockApiClient::new();
        mock.expect_get_app().times(0);
        mock.expect_list_all_volumes()
            .times(1)
            .returning(|| Ok(vec![]));
        let ctx = test_ctx(mock);
        let result = list(&ctx, None, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn create_volume_returns_volume_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_create_volume()
            .with(mockall::predicate::eq("data"), mockall::predicate::eq(1024))
            .times(1)
            .returning(|_, _| {
                Ok(Volume {
                    id: "vol-1".to_string(),
                    name: "data".to_string(),
                    size_mib: 1024,
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = create(&ctx, "data", 1024, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn create_volume_propagates_error() {
        let mut mock = MockApiClient::new();
        mock.expect_create_volume()
            .times(1)
            .returning(|_, _| Err(CliError::Validation("name too long".to_string())));
        let ctx = test_ctx(mock);
        let result = create(&ctx, "data", 1024, OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
    }

    #[tokio::test]
    async fn attach_volume_resolves_app_then_attaches() {
        let mut mock = MockApiClient::new();
        mock.expect_get_app()
            .with(mockall::predicate::eq("svc"))
            .times(1)
            .returning(|_| {
                Ok(AppInfo {
                    id: "a1".to_string(),
                    name: "svc".to_string(),
                    git_url: "".to_string(),
                    port: 8080,
                    hostname: None,
                    active_deployment_id: None,
                    desired_replicas: 1,
                    min_replicas: 0,
                    max_replicas: 1,
                    autoscaling_enabled: false,
                    cpu_threshold: 80.0,
                    mem_threshold: 80.0,
                    created_at: None,
                })
            });
        mock.expect_attach_volume()
            .with(
                mockall::predicate::eq("a1"),
                mockall::predicate::eq("vol-1"),
                mockall::predicate::eq("/data"),
                mockall::predicate::eq(0),
            )
            .times(1)
            .returning(|_, _, _, _| {
                Ok(crate::domain::models::AppVolume {
                    app_id: "a1".to_string(),
                    volume_id: "vol-1".to_string(),
                    mount_point: "/data".to_string(),
                    access_mode: 0,
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = attach(&ctx, "svc", "vol-1", "/data", 0, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn detach_volume_resolves_app_then_detaches() {
        let mut mock = MockApiClient::new();
        mock.expect_get_app()
            .with(mockall::predicate::eq("svc"))
            .times(1)
            .returning(|_| {
                Ok(AppInfo {
                    id: "a1".to_string(),
                    name: "svc".to_string(),
                    git_url: "".to_string(),
                    port: 8080,
                    hostname: None,
                    active_deployment_id: None,
                    desired_replicas: 1,
                    min_replicas: 0,
                    max_replicas: 1,
                    autoscaling_enabled: false,
                    cpu_threshold: 80.0,
                    mem_threshold: 80.0,
                    created_at: None,
                })
            });
        mock.expect_detach_volume()
            .with(
                mockall::predicate::eq("a1"),
                mockall::predicate::eq("vol-1"),
            )
            .times(1)
            .returning(|_, _| Ok(()));
        let ctx = test_ctx(mock);
        let result = detach(&ctx, "svc", "vol-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_volume_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_delete_volume()
            .with(mockall::predicate::eq("vol-1"))
            .times(1)
            .returning(|_| Ok(()));
        let ctx = test_ctx(mock);
        let result = delete(&ctx, "vol-1", true, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_create_volume_snapshot()
            .with(
                mockall::predicate::eq("vol-1"),
                mockall::predicate::eq("snap-1"),
            )
            .times(1)
            .returning(|_, _| {
                Ok(crate::domain::models::VolumeSnapshot {
                    id: "snap-id".to_string(),
                    volume_id: "vol-1".to_string(),
                    name: "snap-1".to_string(),
                    created_at: "2024-01-01T00:00:00Z".to_string(),
                })
            });
        let ctx = test_ctx(mock);
        let result = snapshot(&ctx, "vol-1", "snap-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn restore_calls_api_and_returns_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_restore_volume_snapshot()
            .with(
                mockall::predicate::eq("vol-1"),
                mockall::predicate::eq("snap-1"),
            )
            .times(1)
            .returning(|_, _| Ok(()));
        let ctx = test_ctx(mock);
        let result = restore(&ctx, "vol-1", "snap-1", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_routes_create_command() {
        let mut mock = MockApiClient::new();
        mock.expect_create_volume().times(1).returning(|_, _| {
            Ok(Volume {
                id: "vol-1".to_string(),
                name: "data".to_string(),
                size_mib: 1024,
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            VolumeCommands::Create {
                name: "data".to_string(),
                size: 1024,
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
