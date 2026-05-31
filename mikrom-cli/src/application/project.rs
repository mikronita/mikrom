use crate::application::context::CliContext;
use crate::commands::{OutputFormat, ProjectCommands};
use crate::config::Config;
use crate::domain::error::CliResult;
use crate::infrastructure::ui;

pub async fn handle(
    ctx: &CliContext,
    cmd: ProjectCommands,
    cfg: &mut Config,
    format: OutputFormat,
) -> CliResult<()> {
    match cmd {
        ProjectCommands::List => {
            let projects = ctx.client.list_projects().await?;
            if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&projects).unwrap());
            } else {
                let current_tenant = cfg.active_tenant_id();
                let table_data: Vec<Vec<String>> = projects
                    .into_iter()
                    .map(|p| {
                        let active_marker = if Some(&p.tenant_id) == current_tenant {
                            "*"
                        } else {
                            ""
                        };
                        vec![active_marker.to_string(), p.name, p.tenant_id, p.id]
                    })
                    .collect();
                ui::table(
                    "Projects",
                    &["", "NAME", "ID (6-char)", "UUID"],
                    &table_data,
                );
            }
        },
        ProjectCommands::Create { name } => {
            let project = ctx.client.create_project(&name).await?;
            ui::success(&format!(
                "Project created: {} ({})",
                project.name, project.tenant_id
            ));
            ui::info(&format!(
                "Use 'mikrom project switch {}' to start using it.",
                project.tenant_id
            ));
        },
        ProjectCommands::Switch { tenant_id } => {
            cfg.set_active_tenant_id(tenant_id.clone());
            cfg.save()
                .map_err(|e| crate::domain::error::CliError::Io(std::io::Error::other(e)))?;
            ui::success(&format!("Switched to project: {}", tenant_id));
        },
    }
    Ok(())
}
