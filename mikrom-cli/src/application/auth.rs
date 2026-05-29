use crate::application::context::CliContext;
use crate::commands::{AuthCommands, OutputFormat};
use crate::config::Config;
use crate::domain::error::CliResult;
use crate::infrastructure::ui;
use crate::output::print_json;

pub async fn handle(
    ctx: &CliContext,
    cmd: AuthCommands,
    cfg: &mut Config,
    output: OutputFormat,
) -> CliResult<()> {
    match cmd {
        AuthCommands::Register { email, password } => {
            register(ctx, &email, &password, output).await
        },
        AuthCommands::Login { email, password } => login(ctx, &email, &password, cfg, output).await,
        AuthCommands::Whoami => whoami(ctx, output).await,
        AuthCommands::Update {
            first_name,
            last_name,
        } => update(ctx, first_name, last_name, output).await,
    }
}

async fn register(
    ctx: &CliContext,
    email: &str,
    password: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Registering user {}...", ui::INFO, ui::bold_cyan(email)),
        );
    }
    let resp = ctx.client.register(email, password).await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    ui::success("User created successfully.");
    ui::label_value(ui::KEY, "User ID:", &resp.user.id);
    Ok(())
}

async fn login(
    ctx: &CliContext,
    email: &str,
    password: &str,
    cfg: &mut Config,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Logging in as {}...", ui::KEY, ui::bold_cyan(email)),
        );
    }
    let resp = ctx.client.login(email, password).await?;
    cfg.token = Some(resp.token);
    cfg.save()
        .map_err(|e| crate::domain::error::CliError::Config(e.to_string()))?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "logged_in": true, "email": email }));
        return Ok(());
    }

    ui::step(
        ui::SUCCESS,
        &format!(
            "{} Logged in successfully. Token saved to config.",
            ui::green_label("Welcome!")
        ),
    );
    Ok(())
}

async fn whoami(ctx: &CliContext, output: OutputFormat) -> CliResult<()> {
    let user = ctx.client.whoami().await?;
    if output == OutputFormat::Json {
        print_json(&user);
        return Ok(());
    }

    ui::step(ui::INFO, &ui::bold_cyan("Current User Profile"));
    let name = match (user.first_name.as_ref(), user.last_name.as_ref()) {
        (Some(f), Some(l)) => format!("{} {}", f, l),
        (Some(f), None) => f.to_string(),
        (None, Some(l)) => l.to_string(),
        (None, None) => "N/A".to_string(),
    };
    ui::table(
        "👤 Current User",
        &["Field", "Value"],
        &[
            vec!["Email".to_string(), user.email],
            vec!["User ID".to_string(), user.user_id],
            vec![
                "Role".to_string(),
                user.role.unwrap_or_else(|| "—".to_string()),
            ],
            vec!["Name".to_string(), name],
            vec![
                "Created".to_string(),
                user.created_at.unwrap_or_else(|| "—".to_string()),
            ],
        ],
    );
    Ok(())
}

async fn update(
    ctx: &CliContext,
    first_name: Option<String>,
    last_name: Option<String>,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(ui::WAIT, &format!("{} Updating profile...", ui::SYS));
    }
    let user = ctx.client.update_profile(first_name, last_name).await?;
    if output == OutputFormat::Json {
        print_json(&user);
        return Ok(());
    }

    ui::success("Profile updated successfully.");
    ui::label_value(ui::INFO, "Email:", &user.email);
    if let (Some(f), Some(l)) = (user.first_name, user.last_name) {
        ui::label_value(ui::INFO, "Name:", &format!("{} {}", f, l));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::error::CliError;
    use crate::domain::models::WhoamiResponse;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn whoami_returns_user_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_whoami().times(1).returning(|| {
            Ok(WhoamiResponse {
                user_id: "u1".to_string(),
                email: "test@example.com".to_string(),
                role: Some("User".to_string()),
                first_name: Some("Ada".to_string()),
                last_name: Some("Lovelace".to_string()),
                created_at: None,
            })
        });
        let ctx = test_ctx(mock);
        let result = whoami(&ctx, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn whoami_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_whoami()
            .times(1)
            .returning(|| Err(CliError::Unauthorized("bad token".to_string())));
        let ctx = test_ctx(mock);
        let result = whoami(&ctx, OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::Unauthorized(_))));
    }

    #[tokio::test]
    async fn register_returns_response_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_register().times(1).returning(|_, _| {
            Ok(crate::domain::models::RegisterResponse {
                user: crate::domain::models::RegisterUser {
                    id: "u1".to_string(),
                    email: "test@example.com".to_string(),
                    role: Some("User".to_string()),
                    first_name: None,
                    last_name: None,
                    vpc_ipv6_prefix: Some("fd00:abcd::".to_string()),
                },
                token: "t".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = register(&ctx, "test@example.com", "password", OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn register_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_register()
            .times(1)
            .returning(|_, _| Err(CliError::Validation("email taken".to_string())));
        let ctx = test_ctx(mock);
        let result = register(&ctx, "test@example.com", "password", OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::Validation(_))));
    }

    #[tokio::test]
    async fn login_saves_token_to_config() {
        let mut mock = MockApiClient::new();
        mock.expect_login().times(1).returning(|_, _| {
            Ok(crate::domain::models::LoginResponse {
                token: "secret-jwt".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = login(
            &ctx,
            "test@example.com",
            "password",
            &mut cfg,
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(cfg.token.as_deref(), Some("secret-jwt"));
    }

    #[tokio::test]
    async fn login_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_login()
            .times(1)
            .returning(|_, _| Err(CliError::Unauthorized("bad password".to_string())));
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = login(
            &ctx,
            "test@example.com",
            "password",
            &mut cfg,
            OutputFormat::Json,
        )
        .await;
        assert!(matches!(result, Err(CliError::Unauthorized(_))));
    }

    #[tokio::test]
    async fn update_profile_returns_user_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_update_profile().times(1).returning(|_, _| {
            Ok(WhoamiResponse {
                user_id: "u1".to_string(),
                email: "test@example.com".to_string(),
                role: None,
                first_name: Some("Ada".to_string()),
                last_name: Some("Lovelace".to_string()),
                created_at: None,
            })
        });
        let ctx = test_ctx(mock);
        let result = update(
            &ctx,
            Some("Ada".to_string()),
            Some("Lovelace".to_string()),
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_profile_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_update_profile().times(1).returning(|_, _| {
            Err(CliError::Api {
                status: 500,
                message: "db down".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = update(&ctx, None, None, OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::Api { .. })));
    }

    #[tokio::test]
    async fn handle_routes_login() {
        let mut mock = MockApiClient::new();
        mock.expect_login().times(1).returning(|_, _| {
            Ok(crate::domain::models::LoginResponse {
                token: "t".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let mut cfg = Config::default();
        let result = handle(
            &ctx,
            AuthCommands::Login {
                email: "a@b.com".to_string(),
                password: "pw".to_string(),
            },
            &mut cfg,
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
