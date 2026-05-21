use crate::client::MikromClient;
use crate::commands::{OutputFormat, VolumeCommands};
use crate::ui;
use anyhow::{Context, Result};

pub async fn handle(
    client: &MikromClient,
    cmd: VolumeCommands,
    output: OutputFormat,
) -> Result<()> {
    match cmd {
        VolumeCommands::List { app } => list(client, app, output).await,
        VolumeCommands::Create { name, size } => create(client, &name, size, output).await,
        VolumeCommands::Attach {
            app,
            volume_id,
            mount,
            mode,
        } => attach(client, &app, &volume_id, &mount, mode, output).await,
        VolumeCommands::Detach { app, volume_id } => detach(client, &app, &volume_id, output).await,
        VolumeCommands::Snapshot { volume_id, name } => {
            snapshot(client, &volume_id, &name, output).await
        },
        VolumeCommands::Restore {
            volume_id,
            snapshot: snapshot_name,
        } => restore(client, &volume_id, &snapshot_name, output).await,
        VolumeCommands::Delete { volume_id } => delete(client, &volume_id, output).await,
    }
}

async fn list(client: &MikromClient, app_name: Option<String>, output: OutputFormat) -> Result<()> {
    if let Some(name) = app_name {
        let app = find_app_by_name(client, &name).await?;
        let volumes = client.list_volumes(&app.id).await?;

        if output == OutputFormat::Json {
            return ui::print_json(&volumes);
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
        let volumes = client.list_all_volumes().await?;

        if output == OutputFormat::Json {
            return ui::print_json(&volumes);
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

async fn create(client: &MikromClient, name: &str, size: i32, output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("Creating volume {}...", ui::bold_cyan(name)),
        );
    }

    let volume = client.create_volume(name, size).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&volume);
    }

    ui::success(&format!("Volume created: {} ({})", volume.name, volume.id));
    Ok(())
}

async fn attach(
    client: &MikromClient,
    app_name: &str,
    volume_id: &str,
    mount_point: &str,
    access_mode: i32,
    output: OutputFormat,
) -> Result<()> {
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

    let app = find_app_by_name(client, app_name).await?;
    let attachment = client
        .attach_volume(&app.id, volume_id, mount_point, access_mode)
        .await?;

    if output == OutputFormat::Json {
        return ui::print_json(&attachment);
    }

    ui::success(&format!(
        "Volume attached successfully to {}.",
        ui::bold_cyan(app_name)
    ));
    Ok(())
}

async fn detach(
    client: &MikromClient,
    app_name: &str,
    volume_id: &str,
    output: OutputFormat,
) -> Result<()> {
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

    let app = find_app_by_name(client, app_name).await?;
    client.detach_volume(&app.id, volume_id).await?;

    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "detached": true, "app": app_name, "volume_id": volume_id }),
        );
    }

    ui::success(&format!(
        "Volume detached successfully from {}.",
        ui::bold_cyan(app_name)
    ));
    Ok(())
}

async fn snapshot(
    client: &MikromClient,
    volume_id: &str,
    name: &str,
    output: OutputFormat,
) -> Result<()> {
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

    let snap = client.create_volume_snapshot(volume_id, name).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&snap);
    }

    ui::success(&format!("Snapshot created: {} ({})", snap.name, snap.id));
    Ok(())
}

async fn restore(
    client: &MikromClient,
    volume_id: &str,
    snapshot_name: &str,
    output: OutputFormat,
) -> Result<()> {
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

    client
        .restore_volume_snapshot(volume_id, snapshot_name)
        .await?;

    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "restored": true, "volume_id": volume_id, "snapshot": snapshot_name }),
        );
    }

    ui::success(&format!(
        "Volume {} restored to snapshot {}.",
        volume_id, snapshot_name
    ));
    Ok(())
}

async fn delete(client: &MikromClient, volume_id: &str, output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("Deleting volume {}...", ui::red_label(volume_id)),
        );
    }

    client.delete_volume(volume_id).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({ "deleted": true, "volume_id": volume_id }));
    }

    ui::success(&format!("Volume {} deleted.", volume_id));
    Ok(())
}

async fn find_app_by_name(client: &MikromClient, name: &str) -> Result<crate::client::AppInfo> {
    let apps = client.list_apps().await?;
    apps.into_iter()
        .find(|a| a.name == name)
        .context(format!("Application '{}' not found", name))
}
