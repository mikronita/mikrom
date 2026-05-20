use crate::client::MikromClient;
use crate::commands::{AppCommands, OutputFormat};
use crate::ui;
use anyhow::Result;
use std::io::{self, IsTerminal, Write};

const CPU_OPTIONS: [u32; 4] = [1, 2, 3, 4];
const MEMORY_OPTIONS: [(&str, u32); 4] = [("512M", 512), ("1G", 1024), ("2G", 2048), ("4G", 4096)];

pub async fn handle(client: &MikromClient, cmd: AppCommands, output: OutputFormat) -> Result<()> {
    match cmd {
        AppCommands::List => list(client, output).await,
        AppCommands::Create { name, git_url } => create(client, &name, &git_url, output).await,
        AppCommands::Delete { name } => delete(client, &name, output).await,
        AppCommands::Deploy { name, cpu, memory } => {
            deploy(client, &name, cpu, memory, output).await
        },
        AppCommands::Activate { app, deployment_id } => {
            activate(client, &app, &deployment_id, output).await
        },
        AppCommands::Deployments { name } => list_deployments(client, &name, output).await,
        AppCommands::Watch { name } => watch(&name),
        AppCommands::Secret { name } => show_secret(client, &name, output).await,
        AppCommands::Scale {
            name,
            replicas,
            auto,
            max,
            cpu,
            mem,
        } => scale(client, &name, replicas, auto, max, cpu, mem, output).await,
    }
}

async fn list(client: &MikromClient, output: OutputFormat) -> Result<()> {
    let apps = client.list_apps().await?;
    if output == OutputFormat::Json {
        return ui::print_json(&apps);
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
                        format!("Auto ({}/{})", app.desired_replicas, app.max_replicas)
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
    client: &MikromClient,
    name: &str,
    git_url: &str,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Creating app {}...", ui::APP, ui::bold_cyan(name)),
        );
    }
    let app = client.create_app(name, git_url).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&app);
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

async fn delete(client: &MikromClient, name: &str, output: OutputFormat) -> Result<()> {
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
    client.delete_app(name).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({ "deleted": true, "app": name }));
    }

    ui::step(ui::SUCCESS, &format!("Application {} deleted.", name));
    Ok(())
}

async fn deploy(
    client: &MikromClient,
    name: &str,
    cpu: Option<u32>,
    memory: Option<u32>,
    output: OutputFormat,
) -> Result<()> {
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
    }
    let resp = client.deploy_app_version(name, vcpus, memory_mib).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&resp);
    }

    if let Some(job_id) = resp.job_id {
        ui::step(
            ui::SUCCESS,
            &format!(
                "{} Deployment started. Job ID: {}",
                ui::ROCKET,
                ui::bold_cyan(&job_id)
            ),
        );
    } else if let Some(dep_id) = resp.deployment_id {
        ui::step(
            ui::SUCCESS,
            &format!(
                "Deployment initiated. Deployment ID: {}",
                ui::bold_cyan(&dep_id)
            ),
        );
    }
    ui::label_value(ui::INFO, "Status:", &ui::cyan_label(&resp.status));
    Ok(())
}

fn prompt_cpu() -> Result<u32> {
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

fn prompt_memory() -> Result<u32> {
    prompt_choice("Select RAM preset:", &MEMORY_OPTIONS, 0)
}

fn prompt_choice(prompt: &str, options: &[(&str, u32)], default_index: usize) -> Result<u32> {
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

async fn activate(
    client: &MikromClient,
    app: &str,
    deployment_id: &str,
    output: OutputFormat,
) -> Result<()> {
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
    client.activate_deployment(app, deployment_id).await?;
    if output == OutputFormat::Json {
        return ui::print_json(
            &serde_json::json!({ "activated": true, "app": app, "deployment_id": deployment_id }),
        );
    }

    ui::success(&format!("Deployment {} is now active.", deployment_id));
    Ok(())
}

async fn list_deployments(client: &MikromClient, name: &str, output: OutputFormat) -> Result<()> {
    let deployments = client.list_app_deployments(name).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&deployments);
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

fn watch(name: &str) -> Result<()> {
    ui::step(
        ui::WATCH,
        &format!(
            "{} Real-time deployment monitoring for {} is planned for a future update.",
            ui::INFO,
            name
        ),
    );
    println!(
        "     Use 'mikrom app deployments {}' to poll manually.",
        name
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn scale(
    client: &MikromClient,
    name: &str,
    replicas: Option<i32>,
    auto: Option<bool>,
    max: Option<i32>,
    cpu: Option<f64>,
    mem: Option<f64>,
    output: OutputFormat,
) -> Result<()> {
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

    let req = crate::client::ScaleRequest {
        desired_replicas: replicas,
        autoscaling_enabled: auto,
        min_replicas: Some(0), // Mandatory scale-to-zero
        max_replicas: max,
        cpu_threshold: cpu,
        mem_threshold: mem,
    };

    client.scale_app(name, req).await?;

    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({ "scaled": true, "app": name }));
    }

    ui::success(&format!("Scaling configuration updated for {}.", name));
    Ok(())
}

async fn show_secret(client: &MikromClient, name: &str, output: OutputFormat) -> Result<()> {
    let secret = client.get_app_secret(name).await?;
    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({
            "app": name,
            "github_webhook_secret": if secret.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(secret) }
        }));
    }

    if secret.is_empty() {
        ui::info(&format!(
            "No webhook secret configured for app {}.",
            ui::bold_cyan(name)
        ));
    } else {
        ui::success(&format!(
            "GitHub Webhook Secret for {}:",
            ui::bold_cyan(name)
        ));
        println!("  {}", secret);
    }
    Ok(())
}
