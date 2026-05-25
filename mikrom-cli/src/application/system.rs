use crate::application::context::CliContext;
use crate::commands::{OutputFormat, SystemCommands};
use crate::domain::error::CliResult;
use crate::infrastructure::ui;
use crate::output::print_json;

pub async fn handle(ctx: &CliContext, cmd: SystemCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        SystemCommands::Health => health(ctx, output).await,
    }
}

async fn health(ctx: &CliContext, output: OutputFormat) -> CliResult<()> {
    let health = ctx.client.health().await?;
    if output == OutputFormat::Json {
        print_json(&health);
        return Ok(());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::models::HealthResponse;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn health_returns_ok_when_api_up() {
        let mut mock = MockApiClient::new();
        mock.expect_health().times(1).returning(|| {
            let mut services = HashMap::new();
            services.insert("API".to_string(), "ONLINE".to_string());
            Ok(HealthResponse {
                status: "ok".to_string(),
                version: "1.0.0".to_string(),
                services,
            })
        });
        let ctx = test_ctx(mock);
        let result = health(&ctx, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn health_renders_table_when_api_up() {
        let mut mock = MockApiClient::new();
        mock.expect_health().times(1).returning(|| {
            let mut services = HashMap::new();
            services.insert("DB".to_string(), "ONLINE".to_string());
            Ok(HealthResponse {
                status: "ok".to_string(),
                version: "2.0.0".to_string(),
                services,
            })
        });
        let ctx = test_ctx(mock);
        let result = health(&ctx, OutputFormat::Table).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn health_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_health().times(1).returning(|| {
            Err(crate::domain::error::CliError::Api {
                status: 503,
                message: "unavailable".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = health(&ctx, OutputFormat::Json).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_routes_health() {
        let mut mock = MockApiClient::new();
        mock.expect_health().times(1).returning(|| {
            Ok(HealthResponse {
                status: "ok".to_string(),
                version: "1.0.0".to_string(),
                services: HashMap::new(),
            })
        });
        let ctx = test_ctx(mock);
        let result = handle(&ctx, SystemCommands::Health, OutputFormat::Json).await;
        assert!(result.is_ok());
    }
}
