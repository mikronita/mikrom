use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;
use std::process::Stdio;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, instrument};

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
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path();

        // ── Git Clone ────────────────────────────────────────────────────────
        info!(git_url = %git_url, path = ?repo_path, "Cloning repository");
        let repo = Repository::clone(git_url, repo_path).context("Failed to clone repository")?;

        let (git_commit_hash, git_commit_message, git_branch) = Self::extract_git_metadata(&repo)?;

        if let Some(tx) = metadata_tx {
            let _ = tx
                .send(GitMetadata {
                    hash: git_commit_hash.clone(),
                    message: git_commit_message.clone(),
                    branch: git_branch.clone(),
                })
                .await;
        }

        let registry_base = self
            .registry
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let full_image_tag = format!("{}/{}:{}", registry_base, image_name, tag);

        // ── Pre-build Fixes ──────────────────────────────────────────────────
        Self::apply_prebuild_fixes(repo_path)?;

        // ── Build Strategy ───────────────────────────────────────────────────
        let dockerfile_path = repo_path.join("Dockerfile");
        if dockerfile_path.exists() {
            info!(image_tag = %full_image_tag, "Dockerfile detected, using docker build");
            let mut child = Command::new("docker")
                .arg("build")
                .arg("-t")
                .arg(&full_image_tag)
                .arg(".")
                .current_dir(repo_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Failed to spawn docker build")?;

            Self::log_output(&mut child).await?;

            let status = child.wait().await?;
            if !status.success() {
                return Err(anyhow::anyhow!(
                    "Docker build failed with status {}",
                    status
                ));
            }
        } else {
            info!(image_tag = %full_image_tag, "No Dockerfile found, using Railpack");

            let mut child = Command::new("railpack")
                .arg("build")
                .arg(".")
                .arg("--name")
                .arg(&full_image_tag)
                .current_dir(repo_path)
                .env(
                    "BUILDKIT_HOST",
                    std::env::var("BUILDKIT_HOST")
                        .unwrap_or_else(|_| "docker-container://mikromrust-buildkit-1".to_string()),
                )
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Failed to spawn railpack build")?;

            Self::log_output(&mut child).await?;

            let status = child.wait().await?;
            if !status.success() {
                return Err(anyhow::anyhow!(
                    "Railpack build failed with status {}",
                    status
                ));
            }
        }

        // ── Detection & Push ────────────────────────────────────────────────
        let exposed_port = self.detect_exposed_port(&full_image_tag).await.unwrap_or(0);

        info!(image_tag = %full_image_tag, "Pushing to registry...");
        let mut push_child = Command::new("docker")
            .args(["push", &full_image_tag])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn docker push")?;

        Self::log_output(&mut push_child).await?;

        let push_status = push_child.wait().await?;
        if !push_status.success() {
            return Err(anyhow::anyhow!("Docker push failed for {}", full_image_tag));
        }

        Ok(BuildResult {
            image_tag: full_image_tag,
            exposed_port,
            git_commit_hash,
            git_commit_message,
            git_branch,
        })
    }

    async fn log_output(child: &mut tokio::process::Child) -> Result<()> {
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    match line {
                        Ok(Some(l)) => info!("[BUILD] {}", l),
                        Ok(None) => break,
                        Err(e) => error!("Error reading stdout: {}", e),
                    }
                }
                line = stderr_reader.next_line() => {
                    match line {
                        Ok(Some(l)) => info!("[BUILD-ERR] {}", l),
                        Ok(None) => {},
                        Err(e) => error!("Error reading stderr: {}", e),
                    }
                }
            }
        }
        Ok(())
    }

    fn extract_git_metadata(repo: &Repository) -> Result<(String, String, String)> {
        let head = repo.head().context("Failed to get HEAD")?;
        let branch = head.shorthand().unwrap_or("unknown").to_string();
        let commit = head.peel_to_commit().context("Failed to peel to commit")?;
        let hash = commit.id().to_string();
        let message = commit.message().unwrap_or("").trim().to_string();
        Ok((hash, message, branch))
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
        if raw == "null" || raw.is_empty() {
            return Some(8080); // Default if not detected
        }

        raw.split('"')
            .find(|s| s.contains('/'))
            .and_then(|s| s.split('/').next())
            .and_then(|s| s.parse().ok())
    }

    fn apply_prebuild_fixes(repo_path: &Path) -> Result<()> {
        // Fix for Railpack failing to resolve pnpm version 9 from lockfile
        let package_json_path = repo_path.join("package.json");
        let pnpm_lock_path = repo_path.join("pnpm-lock.yaml");

        if package_json_path.exists() && pnpm_lock_path.exists() {
            let content = std::fs::read_to_string(&package_json_path)?;
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content).map(|mut pkg| {
                if pkg.get("packageManager").is_none() {
                    info!(
                        "Injecting packageManager into package.json to fix Railpack pnpm resolution"
                    );
                    pkg["packageManager"] = serde_json::json!("pnpm@9.15.0");
                }
                pkg
            }) {
                let new_content = serde_json::to_string_pretty(&pkg).unwrap_or(content);
                let _ = std::fs::write(&package_json_path, new_content);
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
