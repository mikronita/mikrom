use crate::client::MikromClient;
use crate::commands::SystemCommands;
use crate::ui;
use anyhow::Result;
use yansi::Paint;

pub async fn handle(client: &MikromClient, cmd: SystemCommands) -> Result<()> {
    match cmd {
        SystemCommands::Health => health(client).await,
        SystemCommands::Watch => watch(),
    }
}

async fn health(client: &MikromClient) -> Result<()> {
    let health = client.health().await?;
    ui::step(ui::INFO, &ui::bold_cyan("System Health Status"));
    ui::label_value(ui::INFO, "Status:", &ui::green_label(&health.status));
    ui::label_value(ui::INFO, "Version:", &health.version);

    println!("\n  {}", ui::bold_cyan("Services:"));
    for (name, status) in health.services {
        let status_painted = if status == "ONLINE" {
            Paint::new(&status).green()
        } else {
            Paint::new(&status).red()
        };
        println!("    {:<12} {}", name, status_painted);
    }
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
