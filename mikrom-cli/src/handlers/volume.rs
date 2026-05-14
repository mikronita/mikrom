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
        VolumeCommands::List { app } => list(client, &app, output).await,
        VolumeCommands::Create { app, name, size } => {
            create(client, &app, &name, size, output).await
        },
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

async fn list(client: &MikromClient, app_name: &str, output: OutputFormat) -> Result<()> {
    let app = find_app_by_name(client, app_name).await?;
    let volumes = client.list_volumes(&app.id).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&volumes);
    }

    if volumes.is_empty() {
        ui::info(&format!("No volumes found for app {}.", app_name));
    } else {
        let rows = volumes
            .iter()
            .map(|vol| {
                vec![
                    vol.name.clone(),
                    vol.id.clone(),
                    format!("{} MiB", vol.size_mib),
                    vol.pool_name.clone(),
                    vol.created_at.clone(),
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            &format!("💾 Volumes for {}", ui::bold_cyan(app_name)),
            &["Name", "ID", "Size", "Pool", "Created"],
            &rows,
        );
    }
    Ok(())
}

async fn create(
    client: &MikromClient,
    app_name: &str,
    name: &str,
    size: i32,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "Creating volume {} for {}...",
                ui::bold_cyan(name),
                ui::bold_cyan(app_name)
            ),
        );
    }

    let app = find_app_by_name(client, app_name).await?;
    let volume = client.create_volume(&app.id, name, size).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&volume);
    }

    ui::success(&format!("Volume created: {} ({})", volume.name, volume.id));
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
