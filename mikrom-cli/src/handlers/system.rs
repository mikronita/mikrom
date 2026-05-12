use crate::client::MikromClient;
use crate::commands::{OutputFormat, SystemCommands};
use crate::ui;
use anyhow::Result;

pub async fn handle(
    client: &MikromClient,
    cmd: SystemCommands,
    output: OutputFormat,
) -> Result<()> {
    match cmd {
        SystemCommands::Health => health(client, output).await,
        SystemCommands::Watch => watch(),
    }
}

async fn health(client: &MikromClient, output: OutputFormat) -> Result<()> {
    let health = client.health().await?;
    if output == OutputFormat::Json {
        return ui::print_json(&health);
    }

    ui::step(ui::INFO, &ui::bold_cyan("System Health Status"));
    ui::table(
        "⚙️ API",
        &["Field", "Value"],
        &[
            vec!["Status".to_string(), ui::status_label(&health.status)],
            vec!["Version".to_string(), health.version],
        ],
    );
    let rows = health
        .services
        .iter()
        .map(|(name, status)| vec![name.clone(), ui::status_label(status)])
        .collect::<Vec<_>>();
    ui::table("🧩 Services", &["Service", "Status"], &rows);
    Ok(())
}

fn watch() -> Result<()> {
    ui::step(
        ui::WATCH,
        &format!(
            "{} Real-time system health dashboard is planned for a future update.",
            ui::INFO
        ),
    );
    Ok(())
}
