use crate::application::context::CliContext;
use crate::commands::{PatCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::infrastructure::ui;

pub async fn handle(ctx: &CliContext, cmd: PatCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        PatCommands::List => {
            let tokens = ctx.client.list_personal_access_tokens().await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&tokens)?);
            } else {
                let rows = tokens
                    .iter()
                    .map(|token| {
                        vec![
                            token.id.clone(),
                            token.name.clone(),
                            format!("...{}", token.token_last_four),
                            token.created_at.clone(),
                            token.last_used_at.clone().unwrap_or_else(|| "Never".to_string()),
                        ]
                    })
                    .collect::<Vec<_>>();
                ui::table(
                    "🔑 Personal Access Tokens",
                    &["ID", "Name", "Suffix", "Created", "Last Used"],
                    &rows,
                );
            }
        },
        PatCommands::Create { name } => {
            if name.trim().is_empty() {
                return Err(crate::domain::error::CliError::Validation(
                    "Token name cannot be empty".to_string(),
                ));
            }
            let resp = ctx.client.create_personal_access_token(&name).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                ui::success(&format!("Token '{}' created successfully!", resp.details.name));
                ui::step(ui::INFO, &ui::bold_cyan("Your new Personal Access Token:"));
                println!("  {}", resp.token);
                ui::step(ui::WAIT, &ui::yellow_label("WARNING: Make sure to copy this token now. You won't be able to see it again!"));
            }
        },
        PatCommands::Revoke { id, yes } => {
            if !yes {
                println!("Are you sure you want to revoke token {}? (y/N)", id);
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            ctx.client.revoke_personal_access_token(&id).await?;
            ui::success(&format!("Token {} revoked successfully", id));
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::models::{PersonalAccessToken, CreatedTokenResponse};
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn pat_list_displays_tokens() {
        let mut mock = MockApiClient::new();
        mock.expect_list_personal_access_tokens()
            .times(1)
            .returning(|| {
                Ok(vec![PersonalAccessToken {
                    id: "token-1".to_string(),
                    user_id: "user-1".to_string(),
                    name: "my-token".to_string(),
                    token_last_four: "abcd".to_string(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    last_used_at: Some("2026-01-02T00:00:00Z".to_string()),
                }])
            });

        let ctx = test_ctx(mock);
        let result = handle(&ctx, PatCommands::List, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn pat_create_sends_request_and_shows_token() {
        let mut mock = MockApiClient::new();
        mock.expect_create_personal_access_token()
            .times(1)
            .returning(|name| {
                assert_eq!(name, "my-new-token");
                Ok(CreatedTokenResponse {
                    token: "mikrom_pat_fullsecrettoken".to_string(),
                    details: PersonalAccessToken {
                        id: "token-123".to_string(),
                        user_id: "user-1".to_string(),
                        name: "my-new-token".to_string(),
                        token_last_four: "token".to_string(),
                        created_at: "2026-01-01T00:00:00Z".to_string(),
                        last_used_at: None,
                    },
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            PatCommands::Create {
                name: "my-new-token".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn pat_revoke_sends_delete_request_with_yes() {
        let mut mock = MockApiClient::new();
        mock.expect_revoke_personal_access_token()
            .times(1)
            .returning(|token_id| {
                assert_eq!(token_id, "token-123");
                Ok(())
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            PatCommands::Revoke {
                id: "token-123".to_string(),
                yes: true,
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }
}
