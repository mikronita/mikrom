use crate::application::context::CliContext;
use crate::commands::Commands;
use crate::config::Config;
use crate::domain::error::CliResult;

pub async fn dispatch(
    ctx: &CliContext,
    cmd: Commands,
    cfg: &mut Config,
    output: crate::commands::OutputFormat,
) -> CliResult<()> {
    match cmd {
        Commands::Auth(auth_cmd) => {
            crate::application::auth::handle(ctx, auth_cmd, cfg, output).await
        },
        Commands::App(app_cmd) => crate::application::app::handle(ctx, app_cmd, output).await,
        Commands::Deployment(dep_cmd) => {
            crate::application::deployment::handle(ctx, dep_cmd, output).await
        },
        Commands::Config(cfg_cmd) => {
            crate::application::config_cmd::handle(ctx, cfg_cmd, cfg, output).await
        },
        Commands::Volume(vol_cmd) => crate::application::volume::handle(ctx, vol_cmd, output).await,
        Commands::System(sys_cmd) => crate::application::system::handle(ctx, sys_cmd, output).await,
        Commands::Completion { .. } => {
            // Handled in main.rs before dispatch
            Ok(())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::commands::{AuthCommands, ConfigCommands, SystemCommands};
    use crate::config::Config;
    use crate::domain::models::{HealthResponse, WhoamiResponse};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn dispatch_routes_auth_whoami() {
        let mut mock = MockApiClient::new();
        mock.expect_whoami().times(1).returning(|| {
            Ok(WhoamiResponse {
                user_id: "u1".to_string(),
                email: "test@example.com".to_string(),
                role: None,
                first_name: None,
                last_name: None,
                created_at: None,
            })
        });
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = dispatch(
            &ctx,
            Commands::Auth(AuthCommands::Whoami),
            &mut cfg,
            crate::commands::OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatch_routes_system_health() {
        let mut mock = MockApiClient::new();
        mock.expect_health().times(1).returning(|| {
            Ok(HealthResponse {
                status: "ok".to_string(),
                version: "1.0.0".to_string(),
                services: HashMap::new(),
            })
        });
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = dispatch(
            &ctx,
            Commands::System(SystemCommands::Health),
            &mut cfg,
            crate::commands::OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn dispatch_routes_config_show() {
        let mock = MockApiClient::new();
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = dispatch(
            &ctx,
            Commands::Config(ConfigCommands::Show),
            &mut cfg,
            crate::commands::OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
