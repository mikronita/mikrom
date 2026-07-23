use crate::application::context::CliContext;
use crate::commands::{AppCommands, OutputFormat};
use crate::domain::error::CliResult;
use crate::domain::models::ScaleRequest;
use crate::infrastructure::ui;
use crate::output::print_json;
use std::io::{self, IsTerminal, Write};

const CPU_OPTIONS: [u32; 4] = [1, 2, 3, 4];
const MEMORY_OPTIONS: [(&str, u32); 4] = [("512M", 512), ("1G", 1024), ("2G", 2048), ("4G", 4096)];

pub async fn handle(ctx: &CliContext, cmd: AppCommands, output: OutputFormat) -> CliResult<()> {
    match cmd {
        AppCommands::List => list(ctx, output).await,
        AppCommands::Create { name, git_url } => create(ctx, &name, &git_url, output).await,
        AppCommands::Delete { name, yes } => delete(ctx, &name, yes, output).await,
        AppCommands::Deploy {
            name,
            cpu,
            memory,
            hypervisor,
            watch,
        } => {
            deploy(
                ctx,
                &name,
                cpu,
                memory,
                hypervisor.as_deref(),
                watch,
                output,
            )
            .await
        },
        AppCommands::Activate { app, deployment_id } => {
            activate(ctx, &app, &deployment_id, output).await
        },
        AppCommands::Deployments { name } => list_deployments(ctx, &name, output).await,
        AppCommands::Secret { name } => show_secret(ctx, &name, output).await,
        AppCommands::Scale {
            name,
            replicas,
            auto,
            min,
            max,
            cpu,
            mem,
        } => scale(ctx, &name, replicas, auto, min, max, cpu, mem, output).await,
        AppCommands::Logs { name, follow: _ } => {
            ui::info(&format!(
                "Streaming live logs for app '{}' (Ctrl+C to stop)...",
                name
            ));
            ctx.client.stream_app_logs(&name).await
        },
    }
}

async fn list(ctx: &CliContext, output: OutputFormat) -> CliResult<()> {
    let apps = ctx.client.list_apps().await?;
    if output == OutputFormat::Json {
        print_json(&apps);
        return Ok(());
    }

    if apps.is_empty() {
        ui::info("No applications found.");
    } else {
        let rows = apps
            .iter()
            .map(|app| {
                vec![
                    format!("{} {}", ui::APP, ui::bold_cyan(&app.name)),
                    ui::status_label(if app.active_deployment_id.is_some() {
                        "active"
                    } else {
                        "idle"
                    }),
                    app.port.to_string(),
                    if app.autoscaling_enabled {
                        format!(
                            "Auto ({}/{}/{}/{:.1}%/{:.1}%)",
                            app.min_replicas,
                            app.desired_replicas,
                            app.max_replicas,
                            app.cpu_threshold,
                            app.mem_threshold
                        )
                    } else {
                        format!("Fixed ({})", app.desired_replicas)
                    },
                    app.active_deployment_id
                        .as_deref()
                        .unwrap_or("—")
                        .to_string(),
                    app.created_at.as_deref().unwrap_or("—").to_string(),
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            "📦 Registered Applications",
            &[
                "App",
                "Status",
                "Port",
                "Scaling",
                "Active deployment",
                "Created",
            ],
            &rows,
        );
    }
    Ok(())
}

async fn create(
    ctx: &CliContext,
    name: &str,
    git_url: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Creating app {}...", ui::APP, ui::bold_cyan(name)),
        );
    }
    let app = ctx.client.create_app(name, git_url).await?;
    if output == OutputFormat::Json {
        print_json(&app);
        return Ok(());
    }

    ui::success("Application created successfully.");
    ui::table(
        "✨ New Application",
        &["Name", "App ID", "Port", "Hostname"],
        &[vec![
            format!("{} {}", ui::APP, ui::bold_cyan(&app.name)),
            app.id,
            app.port.to_string(),
            app.hostname.unwrap_or_else(|| "—".to_string()),
        ]],
    );
    Ok(())
}

async fn delete(ctx: &CliContext, name: &str, yes: bool, output: OutputFormat) -> CliResult<()> {
    if output == OutputFormat::Table
        && !yes
        && !confirm(&format!(
            "Are you sure you want to delete application '{}'?",
            name
        ))?
    {
        return Err(crate::domain::error::CliError::Cancelled);
    }
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Deleting application {}...",
                ui::APP,
                ui::red_label(name)
            ),
        );
    }
    ctx.client.delete_app(name).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "deleted": true, "app": name }));
        return Ok(());
    }

    ui::step(ui::SUCCESS, &format!("Application {} deleted.", name));
    Ok(())
}

pub fn confirm(prompt: &str) -> CliResult<bool> {
    use std::io::{self, IsTerminal, Write};
    if !io::stdin().is_terminal() {
        return Ok(true);
    }
    print!("{} [y/N]: ", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

async fn deploy(
    ctx: &CliContext,
    name: &str,
    cpu: Option<u32>,
    memory: Option<u32>,
    hypervisor: Option<&str>,
    watch: bool,
    output: OutputFormat,
) -> CliResult<()> {
    let vcpus = match cpu {
        Some(value) => value,
        None => prompt_cpu()?,
    };
    let memory_mib = match memory {
        Some(value) => value,
        None => prompt_memory()?,
    };

    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Triggering deployment for {}...",
                ui::ROCKET,
                ui::bold_cyan(name)
            ),
        );
        ui::label_value(
            ui::INFO,
            "Resources:",
            &format!("{} vCPU, {} MiB RAM", vcpus, memory_mib),
        );
        if let Some(hv) = hypervisor {
            ui::label_value(ui::INFO, "Hypervisor:", hv);
        }
    }
    let resp = ctx
        .client
        .deploy_app_version(name, vcpus, memory_mib, hypervisor.map(|s| s.to_string()))
        .await?;
    if output == OutputFormat::Json {
        print_json(&resp);
        return Ok(());
    }

    let job_id_opt = resp.job_id.clone();
    if let Some(job_id) = &job_id_opt {
        ui::step(
            ui::SUCCESS,
            &format!(
                "{} Deployment started. Job ID: {}",
                ui::ROCKET,
                ui::bold_cyan(job_id)
            ),
        );
    } else if let Some(dep_id) = &resp.deployment_id {
        ui::step(
            ui::SUCCESS,
            &format!(
                "Deployment initiated. Deployment ID: {}",
                ui::bold_cyan(dep_id)
            ),
        );
    }
    ui::label_value(ui::INFO, "Status:", &ui::cyan_label(&resp.status));

    if let (true, Some(job_id)) = (watch, job_id_opt) {
        watch_deployment(ctx, name, &job_id).await?;
    }
    Ok(())
}

async fn watch_deployment(ctx: &CliContext, name: &str, job_id: &str) -> CliResult<()> {
    ui::step(
        ui::WAIT,
        &format!("Watching deployment status for job '{}'...", job_id),
    );
    let mut attempts = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        attempts += 1;
        match ctx.client.get_deployment_status(name, job_id).await {
            Ok(status) => {
                ui::info(&format!("Status [{}s]: {}", attempts * 2, status.status));
                let s = status.status.to_uppercase();
                if s == "RUNNING" || s == "ACTIVE" || s == "HEALTHY" {
                    ui::step(
                        ui::SUCCESS,
                        &format!("Deployment {} is running and healthy!", job_id),
                    );
                    break;
                } else if s == "FAILED" || s == "STOPPED" || s == "ERROR" {
                    ui::error(&format!(
                        "Deployment {} failed with status: {}",
                        job_id, status.status
                    ));
                    break;
                }
            },
            Err(e) => {
                if attempts > 30 {
                    ui::error(&format!("Timed out watching deployment status: {}", e));
                    break;
                }
            },
        }
        if attempts >= 60 {
            ui::step(
                ui::WARN,
                "Watch timeout reached (120s). Deployment is still processing in background.",
            );
            break;
        }
    }
    Ok(())
}

async fn activate(
    ctx: &CliContext,
    app: &str,
    deployment_id: &str,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Activating deployment {} for app {}...",
                ui::DEP,
                ui::bold_cyan(deployment_id),
                ui::bold_cyan(app)
            ),
        );
    }
    ctx.client.activate_deployment(app, deployment_id).await?;
    if output == OutputFormat::Json {
        print_json(
            &serde_json::json!({ "activated": true, "app": app, "deployment_id": deployment_id }),
        );
        return Ok(());
    }

    ui::success(&format!("Deployment {} is now active.", deployment_id));
    Ok(())
}

async fn list_deployments(ctx: &CliContext, name: &str, output: OutputFormat) -> CliResult<()> {
    let deployments = ctx.client.list_app_deployments(name).await?;
    if output == OutputFormat::Json {
        print_json(&deployments);
        return Ok(());
    }

    if deployments.is_empty() {
        ui::info(&format!("No deployments found for app {}.", name));
    } else {
        let rows = deployments
            .iter()
            .map(|dep| {
                vec![
                    format!("{} {}", ui::DEP, dep.id),
                    ui::status_label(&dep.status),
                    dep.image_tag.as_deref().unwrap_or("—").to_string(),
                    dep.created_at.as_deref().unwrap_or("—").to_string(),
                ]
            })
            .collect::<Vec<_>>();
        ui::table(
            &format!("{} Deployment History", ui::bold_cyan(name)),
            &["Deployment", "Status", "Image tag", "Created"],
            &rows,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn scale(
    ctx: &CliContext,
    name: &str,
    replicas: Option<i32>,
    auto: Option<bool>,
    min: Option<i32>,
    max: Option<i32>,
    cpu: Option<f64>,
    mem: Option<f64>,
    output: OutputFormat,
) -> CliResult<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!(
                "{} Configuring scaling for {}...",
                ui::APP,
                ui::bold_cyan(name)
            ),
        );
    }

    let req = ScaleRequest {
        desired_replicas: replicas,
        autoscaling_enabled: auto,
        min_replicas: min,
        max_replicas: max,
        cpu_threshold: cpu,
        mem_threshold: mem,
    };

    ctx.client.scale_app(name, req).await?;

    if output == OutputFormat::Json {
        print_json(&serde_json::json!({ "scaled": true, "app": name }));
        return Ok(());
    }

    ui::success(&format!("Scaling configuration updated for {}.", name));
    Ok(())
}

async fn show_secret(ctx: &CliContext, name: &str, output: OutputFormat) -> CliResult<()> {
    let secret = ctx.client.get_app_secret(name).await?;
    if output == OutputFormat::Json {
        print_json(&serde_json::json!({
            "app": name,
            "github_webhook_secret": secret.as_deref()
        }));
        return Ok(());
    }

    if let Some(s) = secret {
        ui::success(&format!(
            "GitHub Webhook Secret for {}:",
            ui::bold_cyan(name)
        ));
        println!("  {}", s);
    } else {
        ui::info(&format!(
            "No webhook secret configured for app {}.",
            ui::bold_cyan(name)
        ));
    }
    Ok(())
}

fn prompt_cpu() -> CliResult<u32> {
    prompt_choice(
        "Select CPU preset:",
        &[
            ("1 vCPU", CPU_OPTIONS[0]),
            ("2 vCPU", CPU_OPTIONS[1]),
            ("3 vCPU", CPU_OPTIONS[2]),
            ("4 vCPU", CPU_OPTIONS[3]),
        ],
        0,
    )
}

fn prompt_memory() -> CliResult<u32> {
    prompt_choice("Select RAM preset:", &MEMORY_OPTIONS, 0)
}

fn prompt_choice(prompt: &str, options: &[(&str, u32)], default_index: usize) -> CliResult<u32> {
    let default_value = options
        .get(default_index)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| options[0].1);

    if !io::stdin().is_terminal() {
        return Ok(default_value);
    }

    loop {
        ui::info(prompt);
        for (index, (label, _)) in options.iter().enumerate() {
            println!("  {}. {}", index + 1, label);
        }
        print!(
            "Choose [1-{}] (default {}): ",
            options.len(),
            default_index + 1
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default_value);
        }

        if let Ok(choice) = trimmed.parse::<usize>()
            && let Some((_, value)) = options.get(choice.saturating_sub(1))
        {
            return Ok(*value);
        }

        ui::error("Please choose one of the listed options.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::MockApiClient;
    use crate::config::Config;
    use crate::domain::error::CliError;
    use crate::domain::models::AppInfo;
    use std::sync::Arc;

    fn test_ctx(mock: MockApiClient) -> CliContext {
        CliContext::new(Arc::new(Config::default()), Arc::new(mock))
    }

    #[tokio::test]
    async fn list_returns_apps_when_api_ok() {
        let mut mock = MockApiClient::new();
        mock.expect_list_apps().times(1).returning(|| {
            Ok(vec![AppInfo {
                id: "a1".to_string(),
                name: "svc".to_string(),
                git_url: "https://github.com/test/repo".to_string(),
                port: 8080,
                hostname: None,
                active_deployment_id: None,
                desired_replicas: 1,
                min_replicas: 0,
                max_replicas: 1,
                autoscaling_enabled: false,
                cpu_threshold: 80.0,
                mem_threshold: 80.0,
                created_at: None,
            }])
        });
        let ctx = test_ctx(mock);
        let result = list(&ctx, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_calls_api_with_app_name() {
        let mut mock = MockApiClient::new();
        mock.expect_delete_app()
            .with(mockall::predicate::eq("my-app"))
            .times(1)
            .returning(|_| Ok(()));
        let ctx = test_ctx(mock);
        let result = delete(&ctx, "my-app", true, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_propagates_api_error() {
        let mut mock = MockApiClient::new();
        mock.expect_delete_app().times(1).returning(|_| {
            Err(CliError::NotFound {
                resource: "app".to_string(),
                id: "x".to_string(),
            })
        });
        let ctx = test_ctx(mock);
        let result = delete(&ctx, "my-app", true, OutputFormat::Json).await;
        assert!(matches!(result, Err(CliError::NotFound { .. })));
    }
}
