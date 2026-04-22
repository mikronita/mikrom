use anyhow::{Context, Result};
use git2::Repository;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{error, info, instrument};

pub struct AppBuilder {
    registry: String,
}

impl AppBuilder {
    pub fn new(registry: String) -> Self {
        Self { registry }
    }

    #[instrument(skip(self, git_url))]
    pub async fn build_image(
        &self,
        app_id: &str,
        git_url: &str,
        image_name: &str,
        tag: &str,
    ) -> Result<String> {
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let repo_path = temp_dir.path();

        info!(git_url = %git_url, path = ?repo_path, "Cloning repository");

        // Use git2 for cloning
        Repository::clone(git_url, repo_path).context("Failed to clone repository")?;

        let registry_base = self
            .registry
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let registry_host = registry_base.split('/').next().unwrap_or(registry_base);
        let full_image_tag = format!("{}/{}:{}", registry_base, image_name, tag);

        info!(image_tag = %full_image_tag, "Starting docker build");

        // Optional: Docker login if credentials exist in env
        if let (Ok(user), Ok(pass)) = (
            std::env::var("REGISTRY_USER"),
            std::env::var("REGISTRY_PASS"),
        ) {
            info!("Authenticating with registry {}...", registry_host);
            let mut child = Command::new("docker")
                .args(["login", registry_host, "-u", &user, "--password-stdin"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(pass.as_bytes()).await?;
                stdin.flush().await?;
            }

            let login_status = child.wait().await?;
            if !login_status.success() {
                error!("Docker login failed for {}", registry_host);
            }
        }

        let output = Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg(&full_image_tag)
            .arg(repo_path)
            .output()
            .await
            .context("Failed to execute docker build command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(stderr = %stderr, "Docker build failed");
            return Err(anyhow::anyhow!("Docker build failed: {}", stderr));
        }

        info!(image_tag = %full_image_tag, "Docker build successful, pushing to registry...");

        let push_status = Command::new("docker")
            .args(["push", &full_image_tag])
            .status()
            .await
            .context("Failed to execute docker push command")?;

        if !push_status.success() {
            return Err(anyhow::anyhow!("Docker push failed for {}", full_image_tag));
        }

        Ok(full_image_tag)
    }
}
