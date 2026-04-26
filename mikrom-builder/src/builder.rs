use anyhow::{Context, Result};
use git2::Repository;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{error, info, instrument, warn};

pub struct AppBuilder {
    registry: String,
}

pub struct BuildResult {
    pub image_tag: String,
    pub exposed_port: u32,
    pub git_commit_hash: String,
    pub git_commit_message: String,
    pub git_branch: String,
}

pub struct GitMetadata {
    pub hash: String,
    pub message: String,
    pub branch: String,
}

impl AppBuilder {
    pub fn new(registry: String, _buildpack_builder: String) -> Self {
        Self { registry }
    }

    #[instrument(skip(self, git_url, metadata_tx))]
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

        info!(git_url = %git_url, path = ?repo_path, "Cloning repository");

        // Use git2 for cloning
        let (git_commit_hash, git_commit_message, git_branch) = {
            let repo =
                Repository::clone(git_url, repo_path).context("Failed to clone repository")?;
            Self::extract_git_metadata(&repo)?
        };

        if let Some(tx) = metadata_tx {
            let _ = tx
                .send(GitMetadata {
                    hash: git_commit_hash.clone(),
                    message: git_commit_message.clone(),
                    branch: git_branch.clone(),
                })
                .await;
        }
        info!(
            commit = %git_commit_hash,
            branch = %git_branch,
            message = %git_commit_message,
            "Extracted git metadata"
        );

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

        Ok(BuildResult {
            image_tag: full_image_tag,
            exposed_port,
            git_commit_hash,
            git_commit_message,
            git_branch,
        })
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
        self.parse_exposed_ports(&raw)
    }

    fn parse_exposed_ports(&self, raw_json: &str) -> Option<u32> {
        if raw_json == "null" || raw_json.is_empty() {
            return None;
        }

        // ExposedPorts is a map like {"80/tcp": {}, "3000/tcp": {}}
        let ports: serde_json::Value = serde_json::from_str(raw_json).ok()?;
        let ports_map = ports.as_object()?;

        let mut available_ports: Vec<u32> = ports_map
            .keys()
            .filter_map(|key| key.split('/').next()?.parse::<u32>().ok())
            .collect();

        if available_ports.is_empty() {
            return None;
        }

        // Prioritize common web ports
        for &p in &[80, 8080, 3000] {
            if available_ports.contains(&p) {
                return Some(p);
            }
        }

        // Otherwise pick the smallest port to be deterministic
        available_ports.sort_unstable();
        available_ports.first().copied()
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

    #[test]
    fn test_extract_git_metadata() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path();

        // Initialize a real git repo for testing
        let repo = Repository::init(repo_path)?;

        // Create a dummy file and commit it
        std::fs::write(repo_path.join("README.md"), "test")?;
        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let sig = repo.signature()?;

        let commit_id = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Initial exhaustive test commit",
            &tree,
            &[],
        )?;

        // Test extraction
        let (hash, message, branch) = AppBuilder::extract_git_metadata(&repo)?;

        assert_eq!(hash, commit_id.to_string());
        assert_eq!(message, "Initial exhaustive test commit");
        // Git default branch can be master or main depending on config, so we just check it's not empty
        assert!(!branch.is_empty());

        Ok(())
    }
}
