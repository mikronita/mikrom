use anyhow::{Context, Result};
use git2::Repository;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{error, info, instrument, warn};

pub struct AppBuilder {
    registry: String,
}

impl AppBuilder {
    pub fn new(registry: String, _buildpack_builder: String) -> Self {
        Self { registry }
    }

    #[instrument(skip(self, git_url))]
    pub async fn build_image(
        &self,
        app_id: &str,
        git_url: &str,
        image_name: &str,
        tag: &str,
    ) -> Result<(String, u32)> {
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

        // 1. Authenticate if needed
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
                drop(stdin); // Explicitly close stdin
            }

            let output = child.wait_with_output().await?;
            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                warn!(
                    registry = %registry_host,
                    error = %err_msg.trim(),
                    "Docker login failed. Build will continue assuming host is already authenticated."
                );
            } else {
                info!("Docker login successful for {}", registry_host);
            }
        }

        // 2. Decide build strategy: Dockerfile or Buildpacks
        let dockerfile_path = repo_path.join("Dockerfile");
        if dockerfile_path.exists() {
            info!(image_tag = %full_image_tag, "Dockerfile detected, using docker build");
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
        } else {
            info!(
                image_tag = %full_image_tag,
                "No Dockerfile found, using Railpack (railpack build)"
            );

            let output = Command::new("railpack")
                .arg("build")
                .arg(repo_path)
                .arg("--name")
                .arg(&full_image_tag)
                .env("BUILDKIT_HOST", "docker-container://buildkit")
                .output()
                .await
                .context("Failed to execute railpack build command. Is railpack CLI installed?")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!(stderr = %stderr, "Railpack build failed");
                return Err(anyhow::anyhow!("Railpack failed: {}", stderr));
            }
        }

        // 3. Detect exposed ports from image
        let exposed_port = self.detect_exposed_port(&full_image_tag).await.unwrap_or(0);
        if exposed_port > 0 {
            info!(port = %exposed_port, "Detected exposed port from image");
        }

        info!(image_tag = %full_image_tag, "Build successful, pushing to registry...");

        let push_status = Command::new("docker")
            .args(["push", &full_image_tag])
            .status()
            .await
            .context("Failed to execute docker push command")?;

        if !push_status.success() {
            return Err(anyhow::anyhow!("Docker push failed for {}", full_image_tag));
        }

        Ok((full_image_tag, exposed_port))
    }

    async fn detect_exposed_port(&self, image_tag: &str) -> Option<u32> {
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{json .Config.ExposedPorts}}",
                image_tag,
            ])
            .output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        self.parse_exposed_ports(&raw)
    }

    fn parse_exposed_ports(&self, raw_json: &str) -> Option<u32> {
        if raw_json == "null" || raw_json.is_empty() {
            return None;
        }

        // ExposedPorts is a map like {"80/tcp": {}, "3000/tcp": {}}
        let ports: serde_json::Value = serde_json::from_str(raw_json).ok()?;
        let ports_map = ports.as_object()?;

        // Pick the first port we find
        for key in ports_map.keys() {
            // "80/tcp" -> "80"
            if let Some(port) = key.split('/').next().and_then(|s| s.parse::<u32>().ok()) {
                return Some(port);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_exposed_ports() {
        let builder = AppBuilder::new("localhost:5000".into(), "any".into());

        // Basic case
        assert_eq!(builder.parse_exposed_ports("{\"80/tcp\":{}}"), Some(80));

        // Multiple ports (should pick one)
        let multi = builder.parse_exposed_ports("{\"80/tcp\":{},\"443/tcp\":{}}");
        assert!(multi == Some(80) || multi == Some(443));

        // Different format
        assert_eq!(builder.parse_exposed_ports("{\"3000/udp\":{}}"), Some(3000));

        // Null/Empty cases
        assert_eq!(builder.parse_exposed_ports("null"), None);
        assert_eq!(builder.parse_exposed_ports("{}"), None);
        assert_eq!(builder.parse_exposed_ports(""), None);

        // Invalid JSON
        assert_eq!(builder.parse_exposed_ports("invalid"), None);
    }
}
