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
        Commands::Db(db_cmd) => crate::application::database::handle(ctx, db_cmd, output).await,
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
    use crate::commands::{AuthCommands, ConfigCommands, DbCommands, SystemCommands};
    use crate::config::Config;
    use crate::domain::models::{DatabaseInfo, HealthResponse, WhoamiResponse};
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

    #[tokio::test]
    async fn dispatch_routes_database_list() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = dispatch(
            &ctx,
            Commands::Db(DbCommands::List),
            &mut cfg,
            crate::commands::OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
