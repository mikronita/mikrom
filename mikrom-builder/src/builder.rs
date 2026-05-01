use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;
use std::process::Stdio;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, instrument};

pub struct AppBuilder {
    registry: String,
}

#[allow(dead_code)]
pub struct BuildResult {
    pub image_tag: String,
    pub exposed_port: u32,
    pub git_commit_hash: String,
    pub git_commit_message: String,
    pub git_branch: String,
}

#[derive(Clone, Debug)]
pub struct GitMetadata {
    pub hash: String,
    pub message: String,
    pub branch: String,
}

impl AppBuilder {
    pub fn new(registry: String, _buildpack_builder: String) -> Self {
        Self { registry }
    }

    #[instrument(skip(self, metadata_tx))]
    pub async fn build_image(
        &self,
        app_id: &str,
        git_url: &str,
        image_name: &str,
        tag: &str,
        metadata_tx: Option<tokio::sync::mpsc::Sender<GitMetadata>>,
    ) -> Result<BuildResult> {
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let repo_path = temp_dir.path();

        // 1. Git Clone & Metadata
        info!(app_id = %app_id, git_url = %git_url, "Cloning repository");
        let repo = Repository::clone(git_url, repo_path).context("Failed to clone repository")?;
        let metadata = Self::extract_git_metadata(&repo)?;

        if let Some(tx) = metadata_tx {
            let _ = tx.send(metadata.clone()).await;
        }

        // 2. Prepare build environment
        let full_image_tag = self.format_image_tag(image_name, tag);
        Self::apply_prebuild_fixes(repo_path)?;

        // 3. Build Strategy
        let dockerfile_path = repo_path.join("Dockerfile");
        if dockerfile_path.exists() {
            self.run_docker_build(repo_path, &full_image_tag).await?;
        } else {
            self.run_railpack_build(repo_path, &full_image_tag).await?;
        }

        // 4. Post-build: Port detection & Registry push
        let exposed_port = self
            .detect_exposed_port(&full_image_tag)
            .await
            .unwrap_or(8080);
        self.push_to_registry(&full_image_tag).await?;

        Ok(BuildResult {
            image_tag: full_image_tag,
            exposed_port,
            git_commit_hash: metadata.hash,
            git_commit_message: metadata.message,
            git_branch: metadata.branch,
        })
    }

    fn format_image_tag(&self, image_name: &str, tag: &str) -> String {
        let registry_base = self
            .registry
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        format!("{}/{}:{}", registry_base, image_name, tag)
    }

    async fn run_docker_build(&self, repo_path: &Path, image_tag: &str) -> Result<()> {
        info!(image_tag = %image_tag, "Dockerfile detected, using docker build");
        let mut child = Command::new("docker")
            .args(["build", "-t", image_tag, "."])
            .current_dir(repo_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn docker build")?;

        Self::log_output(&mut child).await?;

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("Docker build failed with status {}", status);
        }
        Ok(())
    }

    async fn run_railpack_build(&self, repo_path: &Path, image_tag: &str) -> Result<()> {
        info!(image_tag = %image_tag, "No Dockerfile found, using Railpack");
        let buildkit_host = std::env::var("BUILDKIT_HOST")
            .unwrap_or_else(|_| "docker-container://mikromrust-buildkit-1".to_string());

        let mut child = Command::new("railpack")
            .args(["build", ".", "--name", image_tag])
            .current_dir(repo_path)
            .env("BUILDKIT_HOST", buildkit_host)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn railpack build")?;

        Self::log_output(&mut child).await?;

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("Railpack build failed with status {}", status);
        }
        Ok(())
    }

    async fn push_to_registry(&self, image_tag: &str) -> Result<()> {
        info!(image_tag = %image_tag, "Pushing to registry...");
        let mut child = Command::new("docker")
            .args(["push", image_tag])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn docker push")?;

        Self::log_output(&mut child).await?;

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("Docker push failed for {}", image_tag);
        }
        Ok(())
    }

    /// Safely logs stdout and stderr concurrently without data loss.
    async fn log_output(child: &mut tokio::process::Child) -> Result<()> {
        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let stderr = child.stderr.take().context("Failed to capture stderr")?;

        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                info!("[BUILD] {}", line);
            }
        });

        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                info!("[BUILD-ERR] {}", line);
            }
        });

        let _ = tokio::join!(stdout_task, stderr_task);
        Ok(())
    }

    fn extract_git_metadata(repo: &Repository) -> Result<GitMetadata> {
        let head = repo.head().context("Failed to get HEAD")?;
        let branch = head.shorthand().unwrap_or("unknown").to_string();
        let commit = head.peel_to_commit().context("Failed to peel to commit")?;
        let hash = commit.id().to_string();
        let message = commit.message().unwrap_or("").trim().to_string();

        Ok(GitMetadata {
            hash,
            message,
            branch,
        })
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
        if raw == "null" || raw.is_empty() || raw == "{}" {
            return None;
        }

        // Expected format: {"80/tcp":{}, "8080/tcp":{}}
        raw.split('"')
            .find(|s| s.contains('/'))
            .and_then(|s| s.split('/').next())
            .and_then(|s| s.parse().ok())
    }

    fn apply_prebuild_fixes(repo_path: &Path) -> Result<()> {
        let package_json_path = repo_path.join("package.json");
        let pnpm_lock_path = repo_path.join("pnpm-lock.yaml");

        if package_json_path.exists() && pnpm_lock_path.exists() {
            let content = std::fs::read_to_string(&package_json_path)?;
            if let Ok(mut pkg) = serde_json::from_str::<serde_json::Value>(&content)
                && pkg.get("packageManager").is_none()
            {
                info!("Injecting packageManager into package.json for Railpack compatibility");
                pkg["packageManager"] = serde_json::json!("pnpm@9.15.0");
                let new_content = serde_json::to_string_pretty(&pkg)?;
                std::fs::write(&package_json_path, new_content)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_apply_prebuild_fixes_injects_pnpm() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        let package_json = r#"{"name": "test"}"#;
        fs::write(repo_path.join("package.json"), package_json).unwrap();
        fs::write(repo_path.join("pnpm-lock.yaml"), "").unwrap();

        AppBuilder::apply_prebuild_fixes(repo_path).unwrap();

        let new_content = fs::read_to_string(repo_path.join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&new_content).unwrap();
        assert_eq!(pkg["packageManager"], "pnpm@9.15.0");
    }

    #[test]
    fn test_apply_prebuild_fixes_does_not_override_existing() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        let package_json = r#"{"name": "test", "packageManager": "pnpm@8.0.0"}"#;
        fs::write(repo_path.join("package.json"), package_json).unwrap();
        fs::write(repo_path.join("pnpm-lock.yaml"), "").unwrap();

        AppBuilder::apply_prebuild_fixes(repo_path).unwrap();

        let new_content = fs::read_to_string(repo_path.join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&new_content).unwrap();
        assert_eq!(pkg["packageManager"], "pnpm@8.0.0");
    }
}
