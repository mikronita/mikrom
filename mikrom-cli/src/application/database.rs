use crate::application::context::CliContext;
use crate::commands::{DbCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::domain::models::CreateDatabaseRequest;
use crate::infrastructure::ui;

fn create_database_success_message(name: &str, db: &crate::domain::models::DatabaseInfo) -> String {
    format!(
        "Database {} created successfully (ID: {}). PostgreSQL {}. Initial status: {}",
        name,
        db.id,
        db.postgres_version,
        ui::status_label(&db.status)
    )
}

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
                            format!("PostgreSQL {}", db.postgres_version),
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
                        "ID", "Name", "Engine", "Version", "Status", "vCPUs", "Memory", "Created",
                    ],
                    &rows,
                );
            }
        },
        DbCommands::Create {
            name,
            engine,
            version,
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
                } else {
                    return Err(crate::domain::error::CliError::Validation(format!(
                        "Invalid setting format '{}'. Expected key=value",
                        s
                    )));
                }
            }

            let req = CreateDatabaseRequest {
                name: name.clone(),
                engine,
                postgres_version: version,
                vcpus: Some(vcpus),
                memory_mib: Some(memory_mib),
                disk_mib: Some(disk),
                settings: Some(settings_map),
            };

            let db = ctx.client.create_database(req).await?;
            ui::success(&create_database_success_message(&name, &db));
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
                    ui::label_value(
                        ui::SYS,
                        "Version",
                        &format!("PostgreSQL {}", db.postgres_version),
                    );
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
        DbCommands::Connection { id } => {
            let info = ctx.client.get_database_connection_info(&id).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                ui::step(ui::INFO, &ui::bold_cyan("Database Connection:"));
                ui::label_value(ui::APP, "ID", &info.database_id);
                ui::label_value(ui::APP, "Name", &info.database_name);
                ui::label_value(ui::KEY, "User", &info.database_user);
                ui::label_value(ui::PORT, "Host", &info.database_host);
                ui::label_value(ui::PORT, "Port", &info.database_port.to_string());
                ui::label_value(ui::SYS, "SSH Host", &info.ssh_host);
                ui::label_value(ui::SYS, "SSH Port", &info.ssh_port.to_string());
                ui::step(ui::WAIT, "SSH tunnel command:");
                println!("  {}", info.ssh_tunnel_command);
                ui::step(ui::WAIT, "psql command:");
                println!("  {}", info.psql_command);
            }
        },
        DbCommands::Branches { id } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let branches = ctx.client.list_database_branches(&db.id).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&branches)?);
            } else {
                let rows = branches
                    .iter()
                    .map(|b| {
                        vec![
                            b.database_name.clone(),
                            b.branch_name.clone(),
                            b.neon_tenant_id.clone().unwrap_or_default(),
                            b.neon_timeline_id.clone().unwrap_or_default(),
                            ui::status_label(&b.status),
                            if b.is_current {
                                "Yes".to_string()
                            } else {
                                "No".to_string()
                            },
                        ]
                    })
                    .collect::<Vec<_>>();
                ui::table(
                    &format!("🌿 Branches for database {}", db.name),
                    &[
                        "Database",
                        "Branch",
                        "Neon Tenant ID",
                        "Neon Timeline ID",
                        "Status",
                        "Current",
                    ],
                    &rows,
                );
            }
        },
        DbCommands::Backup { id } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let backup = ctx.client.get_database_backups(&db.id).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&backup)?);
            } else {
                ui::step(
                    ui::INFO,
                    &ui::bold_cyan(&format!("Backup Details for Database: {}", db.name)),
                );
                ui::label_value(ui::INFO, "Database ID", &backup.database_id);
                ui::label_value(ui::SYS, "Backup Strategy", &backup.backup_strategy);
                ui::label_value(ui::WATCH, "Recovery Mode", &backup.recovery_mode);
                ui::label_value(
                    ui::SYS,
                    "Retention Valid",
                    &if backup.retention_valid {
                        "Yes".to_string()
                    } else {
                        "No".to_string()
                    },
                );
                ui::label_value(
                    ui::PORT,
                    "Neon Tenant ID",
                    &backup.neon_tenant_id.unwrap_or_default(),
                );
                ui::label_value(
                    ui::PORT,
                    "Neon Timeline ID",
                    &backup.neon_timeline_id.unwrap_or_default(),
                );
                ui::label_value(
                    ui::SYS,
                    "Tenant Gen",
                    &backup
                        .tenant_gen
                        .map(|g| g.to_string())
                        .unwrap_or_else(|| "1".to_string()),
                );
                ui::label_value(ui::WATCH, "Status", &ui::status_label(&backup.status));
                ui::label_value(ui::CLOCK, "Created At", &backup.created_at);
                ui::label_value(ui::CLOCK, "Updated At", &backup.updated_at);
            }
        },
        DbCommands::Snapshots { id } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let resp = ctx.client.list_database_snapshots(&db.id).await?;
            if output == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                if !resp.success {
                    ui::error(&resp.message);
                }
                let rows = resp
                    .snapshots
                    .iter()
                    .map(|s| {
                        let size_str = if s.size_bytes >= 1024 * 1024 * 1024 {
                            format!(
                                "{:.1} GiB",
                                s.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                            )
                        } else if s.size_bytes >= 1024 * 1024 {
                            format!("{:.1} MiB", s.size_bytes as f64 / (1024.0 * 1024.0))
                        } else {
                            format!("{:.1} KiB", s.size_bytes as f64 / 1024.0)
                        };
                        let datetime = chrono::DateTime::from_timestamp(s.created_at, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_else(|| s.created_at.to_string());
                        vec![
                            s.id.clone(),
                            s.name.clone(),
                            ui::status_label(&s.vm_status),
                            size_str,
                            datetime,
                        ]
                    })
                    .collect::<Vec<_>>();
                ui::table(
                    &format!("📸 Snapshots for database {}", db.name),
                    &["ID", "Name", "VM Status", "Size", "Created At"],
                    &rows,
                );
            }
        },
        DbCommands::SnapshotCreate { id, name } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let resp = ctx.client.create_database_snapshot(&db.id, &name).await?;
            if resp.success {
                ui::success(&format!("Snapshot '{}' created: {}", name, resp.message));
            } else {
                ui::error(&format!("Failed to create snapshot: {}", resp.message));
            }
        },
        DbCommands::SnapshotRestore { id, snapshot } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let resp = ctx
                .client
                .restore_database_snapshot(&db.id, &snapshot)
                .await?;
            if resp.success {
                ui::success(&format!(
                    "Database restored to snapshot '{}': {}",
                    snapshot, resp.message
                ));
            } else {
                ui::error(&format!("Failed to restore snapshot: {}", resp.message));
            }
        },
        DbCommands::SnapshotDelete { id, snapshot } => {
            let dbs = ctx.client.list_databases().await?;
            let db = dbs
                .into_iter()
                .find(|d| d.name == id || d.id == id)
                .ok_or_else(|| {
                    crate::domain::error::CliError::Validation(format!(
                        "Database '{}' not found",
                        id
                    ))
                })?;
            let resp = ctx
                .client
                .delete_database_snapshot(&db.id, &snapshot)
                .await?;
            if resp.success {
                ui::success(&format!(
                    "Snapshot '{}' deleted: {}",
                    snapshot, resp.message
                ));
            } else {
                ui::error(&format!("Failed to delete snapshot: {}", resp.message));
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
    use crate::domain::models::{DatabaseConnectionInfo, DatabaseInfo};
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
            assert_eq!(req.postgres_version, 16);
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
                postgres_version: 16,
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
                version: 16,
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
    async fn create_uses_returned_status_in_success_message() {
        let db = DatabaseInfo {
            id: "db-1".to_string(),
            name: "orders".to_string(),
            engine: "neon".to_string(),
            postgres_version: 16,
            status: "pending".to_string(),
            vcpus: 1,
            memory_mib: 512,
            disk_mib: 1024,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(
            create_database_success_message("orders", &db),
            format!(
                "Database orders created successfully (ID: db-1). PostgreSQL 16. Initial status: {}",
                ui::status_label("pending")
            )
        );
    }

    #[tokio::test]
    async fn connection_prints_commands_and_forwards_request() {
        let mut mock = MockApiClient::new();
        mock.expect_get_database_connection_info()
            .times(1)
            .returning(|db_id| {
                assert_eq!(db_id, "db-1");
                Ok(DatabaseConnectionInfo {
                    database_id: "db-1".to_string(),
                    database_name: "orders".to_string(),
                    database_user: "cloud_admin".to_string(),
                    database_host: "127.0.0.1".to_string(),
                    database_port: 5432,
                    ssh_host: "fd00::1".to_string(),
                    ssh_user: "mikrom".to_string(),
                    ssh_port: 22,
                    ssh_tunnel_command: "ssh -N -L 5432:127.0.0.1:5432 mikrom@[fd00::1]"
                        .to_string(),
                    psql_command:
                        "psql \"host=127.0.0.1 port=5432 user=cloud_admin dbname=orders\""
                            .to_string(),
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::Connection {
                id: "db-1".to_string(),
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
                version: 16,
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

    #[tokio::test]
    async fn create_rejects_settings_without_equals_sign() {
        let ctx = test_ctx(MockApiClient::new());
        let err = handle(
            &ctx,
            DbCommands::Create {
                name: "orders".to_string(),
                engine: "neon".to_string(),
                version: 16,
                vcpus: 1,
                memory: "1G".to_string(),
                disk: 1024,
                settings: vec!["max_connections".to_string()],
            },
            OutputFormat::Json,
        )
        .await
        .unwrap_err();

        match err {
            CliError::Validation(message) => {
                assert!(message.contains("Invalid setting format"))
            },
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn branches_resolves_id_and_lists_branches() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_list_database_branches()
            .times(1)
            .returning(|db_id| {
                assert_eq!(db_id, "db-1");
                Ok(vec![crate::domain::models::DatabaseBranchInfo {
                    database_id: "db-1".to_string(),
                    database_name: "orders".to_string(),
                    branch_name: "main".to_string(),
                    neon_tenant_id: Some("tenant-1".to_string()),
                    neon_timeline_id: Some("timeline-1".to_string()),
                    tenant_gen: Some(1),
                    status: "ready".to_string(),
                    is_current: true,
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                }])
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::Branches {
                id: "orders".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn backup_resolves_id_and_returns_details() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_get_database_backups()
            .times(1)
            .returning(|db_id| {
                assert_eq!(db_id, "db-1");
                Ok(crate::domain::models::DatabaseBackupInfo {
                    database_id: "db-1".to_string(),
                    database_name: "orders".to_string(),
                    backup_strategy: "continuous".to_string(),
                    recovery_mode: "pitr".to_string(),
                    retention_valid: true,
                    neon_tenant_id: Some("tenant-1".to_string()),
                    neon_timeline_id: Some("timeline-1".to_string()),
                    tenant_gen: Some(1),
                    status: "active".to_string(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::Backup {
                id: "orders".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshots_resolves_id_and_lists_snapshots() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_list_database_snapshots()
            .times(1)
            .returning(|db_id| {
                assert_eq!(db_id, "db-1");
                Ok(crate::domain::models::DatabaseSnapshotListResponse {
                    success: true,
                    message: "Ok".to_string(),
                    snapshots: vec![crate::domain::models::DatabaseSnapshot {
                        id: "snap-1".to_string(),
                        name: "my-snap".to_string(),
                        created_at: 1717891200,
                        size_bytes: 1024 * 1024,
                        vm_status: "running".to_string(),
                    }],
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::Snapshots {
                id: "orders".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_create_resolves_id_and_creates_snapshot() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_create_database_snapshot()
            .times(1)
            .returning(|db_id, name| {
                assert_eq!(db_id, "db-1");
                assert_eq!(name, "my-snap");
                Ok(crate::domain::models::DatabaseSnapshotActionResponse {
                    success: true,
                    message: "created".to_string(),
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::SnapshotCreate {
                id: "orders".to_string(),
                name: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_restore_resolves_id_and_restores_snapshot() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_restore_database_snapshot()
            .times(1)
            .returning(|db_id, snap_name| {
                assert_eq!(db_id, "db-1");
                assert_eq!(snap_name, "my-snap");
                Ok(crate::domain::models::DatabaseSnapshotActionResponse {
                    success: true,
                    message: "restored".to_string(),
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::SnapshotRestore {
                id: "orders".to_string(),
                snapshot: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn snapshot_delete_resolves_id_and_deletes_snapshot() {
        let mut mock = MockApiClient::new();
        mock.expect_list_databases().times(1).returning(|| {
            Ok(vec![DatabaseInfo {
                id: "db-1".to_string(),
                name: "orders".to_string(),
                engine: "neon".to_string(),
                postgres_version: 16,
                status: "running".to_string(),
                vcpus: 1,
                memory_mib: 512,
                disk_mib: 1024,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            }])
        });
        mock.expect_delete_database_snapshot()
            .times(1)
            .returning(|db_id, snap_name| {
                assert_eq!(db_id, "db-1");
                assert_eq!(snap_name, "my-snap");
                Ok(crate::domain::models::DatabaseSnapshotActionResponse {
                    success: true,
                    message: "deleted".to_string(),
                })
            });

        let ctx = test_ctx(mock);
        let result = handle(
            &ctx,
            DbCommands::SnapshotDelete {
                id: "orders".to_string(),
                snapshot: "my-snap".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_ok());
    }
}
