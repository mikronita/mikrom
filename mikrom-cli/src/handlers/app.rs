use crate::client::MikromClient;
use crate::commands::AppCommands;
use crate::ui;
use anyhow::Result;
use yansi::Paint;

pub async fn handle(client: &MikromClient, cmd: AppCommands) -> Result<()> {
    match cmd {
        AppCommands::List => list(client).await,
        AppCommands::Create { name, git_url } => create(client, &name, &git_url).await,
        AppCommands::Delete { name } => delete(client, &name).await,
        AppCommands::Deploy { name } => deploy(client, &name).await,
        AppCommands::Activate { app, deployment_id } => {
            activate(client, &app, &deployment_id).await
        },
        AppCommands::Deployments { name } => list_deployments(client, &name).await,
        AppCommands::Watch { name } => watch(&name),
        AppCommands::Secret { name } => show_secret(client, &name).await,
    }
}

async fn list(client: &MikromClient) -> Result<()> {
    let apps = client.list_apps().await?;
    if apps.is_empty() {
        ui::info("No applications found.");
    } else {
        ui::step(ui::INFO, &ui::bold_cyan("Registered Applications"));
        for app in apps {
            let created = app.created_at.as_deref().unwrap_or("N/A");
            let active = app.active_deployment_id.as_deref().unwrap_or("None");

            println!("\n{} {}", ui::APP, ui::bold_cyan(&app.name));
            ui::label_value(ui::KEY, "APP_ID:", &app.id);
            ui::label_value(ui::PORT, "Port:", &app.port.to_string());
            ui::label_value(ui::DEP, "Active Dep:", active);
            ui::label_value(ui::CLOCK, "Created:", created);
        }
    }
    Ok(())
}

async fn create(client: &MikromClient, name: &str, git_url: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Creating app {}...", ui::APP, ui::bold_cyan(name)),
    );
    let app = client.create_app(name, git_url).await?;
    ui::success("Application created successfully.");
    ui::label_value(ui::APP, "Name:", &ui::bold_cyan(&app.name));
    ui::label_value(ui::KEY, "APP_ID:", &app.id);
    ui::label_value(ui::INFO, "Git URL:", &app.git_url);
    if let Some(host) = app.hostname {
        ui::label_value(ui::INFO, "Domain:", &host);
    }
    Ok(())
}

async fn delete(client: &MikromClient, name: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!(
            "{} Deleting application {}...",
            ui::APP,
            ui::red_label(name)
        ),
    );
    client.delete_app(name).await?;
    ui::step(ui::SUCCESS, &format!("Application {} deleted.", name));
    Ok(())
}

async fn deploy(client: &MikromClient, name: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!(
            "{} Triggering deployment for {}...",
            ui::ROCKET,
            ui::bold_cyan(name)
        ),
    );
    let resp = client.deploy_app_version(name).await?;
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

async fn activate(client: &MikromClient, app: &str, deployment_id: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!(
            "{} Activating deployment {} for app {}...",
            ui::DEP,
            ui::bold_cyan(deployment_id),
            ui::bold_cyan(app)
        ),
    );
    client.activate_deployment(app, deployment_id).await?;
    ui::success(&format!("Deployment {} is now active.", deployment_id));
    Ok(())
}

async fn list_deployments(client: &MikromClient, name: &str) -> Result<()> {
    let deployments = client.list_app_deployments(name).await?;
    if deployments.is_empty() {
        ui::info(&format!("No deployments found for app {}.", name));
    } else {
        ui::step(
            ui::INFO,
            &format!("{} Deployment History", ui::bold_cyan(name)),
        );
        for dep in deployments {
            let status_painted = match dep.status.as_str() {
                "Active" | "Succeeded" | "RUNNING" => Paint::new(&dep.status).green(),
                "Pending" | "Building" | "SCHEDULED" => Paint::new(&dep.status).yellow(),
                "Failed" | "FAILED" => Paint::new(&dep.status).red(),
                _ => Paint::new(&dep.status),
            };
            let created = dep.created_at.as_deref().unwrap_or("N/A");

            println!("\n{} Deployment {}", ui::DEP, ui::bold_cyan(&dep.id));
            ui::label_value(ui::INFO, "Status:", &status_painted.to_string());
            ui::label_value(
                ui::APP,
                "Image Tag:",
                dep.image_tag.as_deref().unwrap_or("N/A"),
            );
            ui::label_value(ui::CLOCK, "Created:", created);
        }
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

async fn show_secret(client: &MikromClient, name: &str) -> Result<()> {
    let secret = client.get_app_secret(name).await?;
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
