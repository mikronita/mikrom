use crate::client::MikromClient;
use crate::commands::{AuthCommands, OutputFormat};
use crate::config::Config;
use crate::ui;
use anyhow::{Context, Result};

pub async fn handle(
    client: &MikromClient,
    cmd: AuthCommands,
    cfg: &mut Config,
    output: OutputFormat,
) -> Result<()> {
    match cmd {
        AuthCommands::Register { email, password } => {
            register(client, &email, &password, output).await
        },
        AuthCommands::Login { email, password } => {
            login(client, &email, &password, cfg, output).await
        },
        AuthCommands::Whoami => whoami(client, output).await,
        AuthCommands::Update {
            first_name,
            last_name,
        } => update(client, first_name, last_name, output).await,
    }
}

async fn register(
    client: &MikromClient,
    email: &str,
    password: &str,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Registering user {}...", ui::INFO, ui::bold_cyan(email)),
        );
    }
    let resp = client
        .register(email, password)
        .await
        .context("Registration failed")?;
    if output == OutputFormat::Json {
        return ui::print_json(&resp);
    }

    ui::success(&resp.message);
    ui::label_value(ui::KEY, "User ID:", &resp.user_id);
    Ok(())
}

async fn login(
    client: &MikromClient,
    email: &str,
    password: &str,
    cfg: &mut Config,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(
            ui::WAIT,
            &format!("{} Logging in as {}...", ui::KEY, ui::bold_cyan(email)),
        );
    }
    let resp = client
        .login(email, password)
        .await
        .context("Login failed")?;
    cfg.token = Some(resp.token);
    cfg.save().context("Failed to save config")?;
    if output == OutputFormat::Json {
        return ui::print_json(&serde_json::json!({ "logged_in": true, "email": email }));
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

async fn whoami(client: &MikromClient, output: OutputFormat) -> Result<()> {
    let user = client.whoami().await.context("Failed to get user info")?;
    if output == OutputFormat::Json {
        return ui::print_json(&user);
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
    client: &MikromClient,
    first_name: Option<String>,
    last_name: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    if output == OutputFormat::Table {
        ui::step(ui::WAIT, &format!("{} Updating profile...", ui::SYS));
    }
    let user = client
        .update_profile(first_name, last_name)
        .await
        .context("Failed to update profile")?;
    if output == OutputFormat::Json {
        return ui::print_json(&user);
    }

    ui::success("Profile updated successfully.");
    ui::label_value(ui::INFO, "Email:", &user.email);
    if let (Some(f), Some(l)) = (user.first_name, user.last_name) {
        ui::label_value(ui::INFO, "Name:", &format!("{} {}", f, l));
    }
    Ok(())
}
