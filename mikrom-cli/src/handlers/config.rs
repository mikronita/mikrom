use crate::commands::ConfigCommands;
use crate::config::Config;
use crate::ui;
use anyhow::Result;

pub async fn handle(cmd: ConfigCommands, cfg: &mut Config) -> Result<()> {
    match cmd {
        ConfigCommands::Show => show(cfg),
        ConfigCommands::Set { key, value } => set(cfg, &key, &value).await,
    }
}

fn show(cfg: &Config) -> Result<()> {
    ui::step(ui::INFO, &ui::bold_cyan("CLI Configuration"));
    ui::label_value(ui::SYS, "API URL:", cfg.api_url());
    if cfg.token.is_some() {
        ui::label_value(ui::KEY, "Token:", &ui::green_label("[Configured]"));
    } else {
        ui::label_value(ui::KEY, "Token:", &ui::yellow_label("[Not Set]"));
    }
    Ok(())
}

async fn set(cfg: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        "api-url" | "api_url" => {
            cfg.api_url = Some(value.to_string());
            cfg.save()?;
            ui::step(
                ui::SUCCESS,
                &format!("API URL updated to {}", ui::cyan_label(value)),
            );
        },
        _ => {
            ui::step(ui::ERROR, &format!("Unknown config key: {}", key));
        },
    }
    Ok(())
}
