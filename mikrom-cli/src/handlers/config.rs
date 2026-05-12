use crate::commands::{ConfigCommands, OutputFormat};
use crate::config::Config;
use crate::ui;
use anyhow::Result;

pub async fn handle(cmd: ConfigCommands, cfg: &mut Config, output: OutputFormat) -> Result<()> {
    match cmd {
        ConfigCommands::Show => show(cfg, output),
        ConfigCommands::Set { key, value } => set(cfg, &key, &value, output).await,
    }
}

fn show(cfg: &Config, output: OutputFormat) -> Result<()> {
    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({
            "api_url": cfg.api_url(),
            "token_configured": cfg.token.is_some()
        }));
    }

    ui::step(ui::INFO, &ui::bold_cyan("CLI Configuration"));
    ui::table(
        "🛠️ Configuration",
        &["Key", "Value"],
        &[
            vec!["API URL".to_string(), cfg.api_url().to_string()],
            vec![
                "Token".to_string(),
                if cfg.token.is_some() {
                    ui::green_label("Configured")
                } else {
                    ui::yellow_label("Not set")
                },
            ],
        ],
    );
    Ok(())
}

async fn set(cfg: &mut Config, key: &str, value: &str, output: OutputFormat) -> Result<()> {
    match key {
        "api-url" | "api_url" => {
            cfg.api_url = Some(value.to_string());
            cfg.save()?;
            if output == OutputFormat::Json {
                return ui::print_json(&serde_json::json!({ "updated": true, "api_url": value }));
            }
            ui::step(
                ui::SUCCESS,
                &format!("API URL updated to {}", ui::cyan_label(value)),
            );
        },
        _ => {
            if output == OutputFormat::Json {
                return ui::print_json(
                    &serde_json::json!({ "updated": false, "error": format!("Unknown config key: {key}") }),
                );
            }
            ui::step(ui::ERROR, &format!("Unknown config key: {}", key));
        },
    }
    Ok(())
}
