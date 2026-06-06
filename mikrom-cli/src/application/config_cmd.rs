use crate::application::context::CliContext;
use crate::commands::{ConfigCommands, OutputFormat};
use crate::config::Config;
use crate::domain::error::CliResult;
use crate::infrastructure::ui;
use crate::output::print_json;

pub async fn handle(
    _ctx: &CliContext,
    cmd: ConfigCommands,
    cfg: &mut Config,
    output: OutputFormat,
) -> CliResult<()> {
    match cmd {
        ConfigCommands::Show => show(cfg, output),
        ConfigCommands::Set { key, value } => set(cfg, &key, &value, output).await,
    }
}

fn show(cfg: &Config, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({
            "api_url": cfg.api_url(),
            "token_configured": cfg.token.is_some(),
            "active_project_slug": cfg.active_project_slug(),
        }));
        return Ok(());
    }

    ui::step(ui::INFO, &ui::bold_cyan("CLI Configuration"));
    ui::table(
        "🛠️ Configuration",
        &["Key", "Value"],
        &[
            vec!["API URL".to_string(), cfg.api_url().to_string()],
            vec![
                "Active project".to_string(),
                cfg.active_project_slug()
                    .cloned()
                    .unwrap_or_else(|| "Not set".to_string()),
            ],
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

async fn set(cfg: &mut Config, key: &str, value: &str, output: OutputFormat) -> CliResult<()> {
    match key {
        "api-url" | "api_url" => {
            cfg.api_url = Some(value.to_string());
            cfg.save()
                .map_err(|e| crate::domain::error::CliError::Config(e.to_string()))?;
            if output == OutputFormat::Json {
                print_json(&serde_json::json!({ "updated": true, "api_url": value }));
                return Ok(());
            }
            ui::step(
                ui::SUCCESS,
                &format!("API URL updated to {}", ui::cyan_label(value)),
            );
        },
        "active-project"
        | "active_project"
        | "active-project-slug"
        | "active_project_slug"
        | "active-tenant-id"
        | "active_tenant_id" => {
            cfg.set_active_project_slug(value.to_string());
            cfg.save()
                .map_err(|e| crate::domain::error::CliError::Config(e.to_string()))?;
            if output == OutputFormat::Json {
                print_json(&serde_json::json!({ "updated": true, "active_project_slug": value }));
                return Ok(());
            }
            ui::step(
                ui::SUCCESS,
                &format!("Active project updated to {}", ui::cyan_label(value)),
            );
        },
        _ => {
            if output == OutputFormat::Json {
                print_json(
                    &serde_json::json!({ "updated": false, "error": format!("Unknown config key: {key}") }),
                );
                return Ok(());
            }
            ui::step(ui::ERROR, &format!("Unknown config key: {}", key));
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[test]
    fn show_outputs_json() {
        let cfg = Config {
            api_url: Some("http://localhost:5001".to_string()),
            token: Some("tok".to_string()),
            active_tenant_id: None,
            ..Default::default()
        };
        let result = show(&cfg, OutputFormat::Json);
        assert!(result.is_ok());
    }

    #[test]
    fn show_outputs_table() {
        let cfg = Config::default();
        let result = show(&cfg, OutputFormat::Table);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn set_api_url_updates_config() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let _ctx = test_ctx(mock);
        let result = set(&mut cfg, "api-url", "http://new:5001", OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(cfg.api_url.as_deref(), Some("http://new:5001"));
    }

    #[tokio::test]
    async fn set_api_url_with_underscore_key() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let _ctx = test_ctx(mock);
        let result = set(&mut cfg, "api_url", "http://new:5001", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn set_unknown_key_returns_ok_in_json_mode() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let _ctx = test_ctx(mock);
        let result = set(&mut cfg, "unknown", "val", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn set_unknown_key_returns_ok_in_table_mode() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let _ctx = test_ctx(mock);
        let result = set(&mut cfg, "unknown", "val", OutputFormat::Table).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_routes_show() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let ctx = test_ctx(mock);
        let result = handle(&ctx, ConfigCommands::Show, &mut cfg, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn handle_routes_set() {
        let mut cfg = Config::default();
        let mock = MockApiClient::new();
        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            ConfigCommands::Set {
                key: "api-url".to_string(),
                value: "http://x".to_string(),
            },
            &mut cfg,
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
