use crate::application::context::CliContext;
use crate::commands::{DbCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::domain::models::CreateDatabaseRequest;
use crate::infrastructure::ui;

pub async fn handle(ctx: &CliContext, cmd: DbCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        DbCommands::List => {
            let dbs = ctx.client.list_databases().await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&dbs)?);
            } else {
                let rows = dbs
                    .iter()
                    .map(|db| {
                        vec![
                            db.id.clone(),
                            db.name.clone(),
                            db.engine.clone(),
                            ui::status_label(&db.status),
                            db.vcpus.to_string(),
                            format!("{}M", db.memory_mib),
                            db.created_at.clone(),
                        ]
                    })
                    .collect::<Vec<_>>();
                ui::table(
                    "🗄️ Registered Databases",
                    &[
                        "ID", "Name", "Engine", "Status", "vCPUs", "Memory", "Created",
                    ],
                    &rows,
                );
            }
        },
        DbCommands::Create {
            name,
            engine,
            vcpus,
            memory,
            disk,
            settings,
        } => {
            let memory_mib = match memory.to_ascii_uppercase().as_str() {
                "512M" => 512,
                "1G" => 1024,
                "2G" => 2048,
                "4G" => 4096,
                _ => {
                    return Err(crate::domain::error::CliError::Validation(
                        "Memory must be 512M, 1G, 2G, or 4G".to_string(),
                    ));
                },
            };

            let mut settings_map = std::collections::HashMap::new();
            for s in settings {
                if let Some((key, value)) = s.split_once('=') {
                    settings_map.insert(key.to_string(), value.to_string());
                }
            }

            let req = CreateDatabaseRequest {
                name: name.clone(),
                engine,
                vcpus: Some(vcpus),
                memory_mib: Some(memory_mib),
                disk_mib: Some(disk),
                settings: Some(settings_map),
            };

            let db = ctx.client.create_database(req).await?;
            ui::success(&format!(
                "Database {} created successfully (ID: {})",
                name, db.id
            ));
        },
        DbCommands::Delete { id, yes } => {
            if !yes {
                println!("Are you sure you want to delete database {}? (y/N)", id);
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            ctx.client.delete_database(&id).await?;
            ui::success(&format!("Database {} deleted successfully", id));
        },
        DbCommands::Info { id } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs.into_iter().find(|d| d.name == id || d.id == id);

            if let Some(db) = db {
                if output == OutputFormat::Json {
                    println!("{}", serde_json::to_string_pretty(&db)?);
                } else {
                    ui::step(ui::INFO, &ui::bold_cyan("Database Information:"));
                    ui::label_value(ui::INFO, "ID", &db.id);
                    ui::label_value(ui::APP, "Name", &db.name);
                    ui::label_value(ui::SYS, "Engine", &db.engine);
                    ui::label_value(ui::WATCH, "Status", &ui::status_label(&db.status));
                    ui::label_value(ui::SYS, "vCPUs", &db.vcpus.to_string());
                    ui::label_value(ui::SYS, "Memory", &format!("{}M", db.memory_mib));
                    ui::label_value(ui::SYS, "Disk", &format!("{}M", db.disk_mib));
                    ui::label_value(ui::CLOCK, "Created", &db.created_at);
                }
            } else {
                ui::error("Database not found.");
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::commands::DbCommands;
    use crate::config::Config;
    use crate::domain::error::CliError;
    use crate::domain::models::DatabaseInfo;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn create_parses_settings_and_forwards_defaults() {
        let mut mock = MockApiClient::new();
        mock.expect_create_database().times(1).returning(|req| {
            assert_eq!(req.name, "orders");
            assert_eq!(req.engine, "neon");
            assert_eq!(req.vcpus, Some(2));
            assert_eq!(req.memory_mib, Some(1024));
            assert_eq!(req.disk_mib, Some(4096));
            assert_eq!(
                req.settings
                    .as_ref()
                    .and_then(|settings| settings.get("max_connections")),
                Some(&"200".to_string())
            );

            Ok(DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                status: "pending".to_string(),
                vcpus: 2,
                memory_mib: 1024,
                disk_mib: 4096,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            })
        });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::Create {
                name: "orders".to_string(),
                engine: "neon".to_string(),
                vcpus: 2,
                memory: "1G".to_string(),
                disk: 4096,
                settings: vec!["max_connections=200".to_string()],
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn create_rejects_invalid_memory_sizes() {
        let ctx = test_ctx(MockApiClient::new());
        let err = handle(
            &ctx,
            DbCommands::Create {
                name: "orders".to_string(),
                engine: "neon".to_string(),
                vcpus: 1,
                memory: "8G".to_string(),
                disk: 1024,
                settings: vec![],
            },
            OutputFormat::Json,
        )
        .await
        .unwrap_err();

        match err {
            CliError::Validation(message) => assert!(message.contains("Memory")),
            other => panic!("expected validation error, got {other:?}"),
        }
    }
}
