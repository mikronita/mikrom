use crate::client::MikromClient;
use crate::commands::AuthCommands;
use crate::config::Config;
use crate::ui;
use anyhow::{Context, Result};

pub async fn handle(client: &MikromClient, cmd: AuthCommands, cfg: &mut Config) -> Result<()> {
    match cmd {
        AuthCommands::Register { email, password } => register(client, &email, &password).await,
        AuthCommands::Login { email, password } => login(client, &email, &password, cfg).await,
        AuthCommands::Whoami => whoami(client).await,
        AuthCommands::Update {
            first_name,
            last_name,
        } => update(client, first_name, last_name).await,
    }
}

async fn register(client: &MikromClient, email: &str, password: &str) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Registering user {}...", ui::INFO, ui::bold_cyan(email)),
    );
    let resp = client
        .register(email, password)
        .await
        .context("Registration failed")?;
    ui::success(&resp.message);
    ui::label_value(ui::KEY, "User ID:", &resp.user_id);
    Ok(())
}

async fn login(client: &MikromClient, email: &str, password: &str, cfg: &mut Config) -> Result<()> {
    ui::step(
        ui::WAIT,
        &format!("{} Logging in as {}...", ui::KEY, ui::bold_cyan(email)),
    );
    let resp = client
        .login(email, password)
        .await
        .context("Login failed")?;
    cfg.token = Some(resp.token);
    cfg.save().context("Failed to save config")?;
    ui::step(
        ui::SUCCESS,
        &format!(
            "{} Logged in successfully. Token saved to config.",
            ui::green_label("Welcome!")
        ),
    );
    Ok(())
}

async fn whoami(client: &MikromClient) -> Result<()> {
    let user = client.whoami().await.context("Failed to get user info")?;
    ui::step(ui::INFO, &ui::bold_cyan("Current User Profile"));
    ui::label_value(ui::INFO, "Email:", &user.email);
    ui::label_value(ui::KEY, "User ID:", &user.user_id);
    if let Some(role) = user.role {
        ui::label_value(ui::INFO, "Role:", &role);
    }

    let name = match (user.first_name.as_ref(), user.last_name.as_ref()) {
        (Some(f), Some(l)) => format!("{} {}", f, l),
        (Some(f), None) => f.to_string(),
        (None, Some(l)) => l.to_string(),
        (None, None) => "N/A".to_string(),
    };
    if name != "N/A" {
        ui::label_value(ui::INFO, "Name:", &name);
    }

    ui::label_value(
        ui::CLOCK,
        "Created At:",
        user.created_at.as_deref().unwrap_or("N/A"),
    );
    Ok(())
}

async fn update(
    client: &MikromClient,
    first_name: Option<String>,
    last_name: Option<String>,
) -> Result<()> {
    ui::step(ui::WAIT, &format!("{} Updating profile...", ui::SYS));
    let user = client
        .update_profile(first_name, last_name)
        .await
        .context("Failed to update profile")?;
    ui::success("Profile updated successfully.");
    ui::label_value(ui::INFO, "Email:", &user.email);
    if let (Some(f), Some(l)) = (user.first_name, user.last_name) {
        ui::label_value(ui::INFO, "Name:", &format!("{} {}", f, l));
    }
    Ok(())
}
