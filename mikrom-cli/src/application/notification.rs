use crate::application::context::CliContext;
use crate::commands::{NotificationCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::infrastructure::ui;

pub async fn handle(ctx: &CliContext, cmd: NotificationCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        NotificationCommands::List { unread_only, limit, offset } => {
            let resp = ctx.client.list_user_notifications(unread_only, limit, offset).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                if resp.unread_count > 0 {
                    ui::step(ui::WARN, &ui::bold_cyan(&format!("You have {} unread notification(s)!", resp.unread_count)));
                } else {
                    ui::step(ui::SUCCESS, "No unread notifications.");
                }

                let rows = resp.notifications
                    .iter()
                    .map(|n| {
                        vec![
                            n.id.clone(),
                            n.title.clone(),
                            n.body.clone(),
                            if n.is_read { "Read".to_string() } else { "UNREAD".to_string() },
                            n.created_at.clone(),
                        ]
                    })
                    .collect::<Vec<_>>();
                ui::table(
                    "🔔 Platform Notifications",
                    &["ID", "Title", "Body", "Status", "Created At"],
                    &rows,
                );
            }
        },
        NotificationCommands::Read { id } => {
            ctx.client.mark_user_notification_read(&id).await?;
            ui::success(&format!("Notification {} marked as read", id));
        },
        NotificationCommands::ReadAll => {
            ctx.client.mark_all_user_notifications_read().await?;
            ui::success("All notifications marked as read");
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::models::{Notification, NotificationListResponse};
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn notification_list_displays_notifications() {
        let mut mock = MockApiClient::new();
        mock.expect_list_user_notifications()
            .times(1)
            .returning(|unread_only, limit, offset| {
                assert!(unread_only);
                assert_eq!(limit, Some(10));
                assert_eq!(offset, None);
                Ok(NotificationListResponse {
                    notifications: vec![Notification {
                        id: "notif-123".to_string(),
                        user_id: "user-1".to_string(),
                        tenant_id: None,
                        kind: "info".to_string(),
                        title: "Test Alert".to_string(),
                        body: "This is a test notification".to_string(),
                        route: "/".to_string(),
                        entity_name: None,
                        resource_id: None,
                        metadata: serde_json::Value::Null,
                        created_at: "2026-01-01T00:00:00Z".to_string(),
                        read_at: None,
                        is_read: false,
                    }],
                    unread_count: 1,
                    has_more: false,
                    next_offset: 0,
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            NotificationCommands::List {
                unread_only: true,
                limit: Some(10),
                offset: None,
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn notification_read_marks_as_read() {
        let mut mock = MockApiClient::new();
        mock.expect_mark_user_notification_read()
            .times(1)
            .returning(|id| {
                assert_eq!(id, "notif-123");
                Ok(())
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            NotificationCommands::Read {
                id: "notif-123".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn notification_read_all_marks_all_as_read() {
        let mut mock = MockApiClient::new();
        mock.expect_mark_all_user_notifications_read()
            .times(1)
            .returning(|| Ok(()));

        let ctx = test_ctx(mock);
        let result = handle(&ctx, NotificationCommands::ReadAll, OutputFormat::Json).await;
        assert!(result.is_ok());
    }
}
