use anyhow::{Context, Result};
use bollard::auth::DockerCredentials;
use bollard::query_parameters::{
    BuildImageOptionsBuilder, PushImageOptionsBuilder, RemoveImageOptionsBuilder,
};
use bollard::{Docker, body_stream};
use futures::stream::StreamExt;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use glob::Pattern;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, instrument};
use walkdir::WalkDir;

pub struct AppBuilder {
    registry: String,
    registry_user: Option<String>,
    registry_pass: Option<String>,
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
    pub fn new(
        registry: String,
        _buildpack_builder: String,
        registry_user: Option<String>,
        registry_pass: Option<String>,
    ) -> Self {
        Self {
            registry,
            registry_user,
            registry_pass,
        }
    }

    #[instrument(skip(self, metadata_tx))]
    pub async fn build_image(
        &self,
        app_id: &str,
        git_url: &str,
        git_auth_token: Option<String>,
        image_name: &str,
        tag: &str,
        metadata_tx: Option<tokio::sync::mpsc::Sender<GitMetadata>>,
    ) -> Result<BuildResult> {
        let temp_dir = TempDir::new().context("Failed to create temporary directory")?;
        let repo_path = temp_dir.path();

        // 1. Git Clone & Metadata
        info!(app_id = %app_id, git_url = %git_url, "Cloning repository");
        let repo = if let Some(token) = git_auth_token {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(move |_url, _username_from_url, _allowed_types| {
                git2::Cred::userpass_plaintext("x-access-token", &token)
            });

            let mut fetch_options = FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);

            let mut builder = git2::build::RepoBuilder::new();
            builder.fetch_options(fetch_options);
            builder
                .clone(git_url, repo_path)
                .context("Failed to clone private repository")?
        } else {
            Repository::clone(git_url, repo_path).context("Failed to clone repository")?
        };
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

        // 5. Cleanup local image to free disk space
        if let Ok(docker) = Docker::connect_with_local_defaults() {
            let _ = docker
                .remove_image(
                    &full_image_tag,
                    Some(RemoveImageOptionsBuilder::default().force(true).build()),
                    None,
                )
                .await;
            info!(image_tag = %full_image_tag, "Cleaned up local image after push");
        }

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
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;
        let context = Self::build_context(repo_path).await?;
        let options = BuildImageOptionsBuilder::default()
            .dockerfile("Dockerfile")
            .t(image_tag)
            .rm(true)
            .build();
        let (tx, rx) = mpsc::channel::<bytes::Bytes>(8);
        let reader_task = tokio::task::spawn_blocking({
            let context_path = context.path().to_path_buf();
            move || -> Result<()> {
                let mut file = fs::File::open(&context_path)
                    .with_context(|| format!("Failed to open {}", context_path.display()))?;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let read = file
                        .read(&mut buffer)
                        .with_context(|| format!("Failed to read {}", context_path.display()))?;
                    if read == 0 {
                        break;
                    }
                    if tx
                        .blocking_send(bytes::Bytes::copy_from_slice(&buffer[..read]))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(())
            }
        });
        let mut stream =
            docker.build_image(options, None, Some(body_stream(ReceiverStream::new(rx))));
        while let Some(message) = stream.next().await {
            let message = message?;
            if let Some(line) = message.stream.as_deref() {
                info!("[DOCKER-BUILD] {}", line.trim_end());
            }
            if let Some(status) = message.status.as_deref() {
                info!("[DOCKER-BUILD] {}", status.trim_end());
            }
            if let Some(error) = message.error_detail.and_then(|detail| detail.message) {
                anyhow::bail!("Docker build failed: {}", error);
            }
        }
        reader_task
            .await
            .context("Docker build context reader failed")??;
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
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;

        let auth_config = match (&self.registry_user, &self.registry_pass) {
            (Some(username), Some(password)) => {
                let serveraddress = image_tag.split('/').next().map(|s| s.to_string());
                info!(
                    username = %username,
                    serveraddress = ?serveraddress,
                    "Using registry credentials for push"
                );
                Some(DockerCredentials {
                    username: Some(username.clone()),
                    password: Some(password.clone()),
                    serveraddress,
                    ..Default::default()
                })
            },
            (None, None) => {
                info!("No registry credentials provided, pushing anonymously");
                None
            },
            _ => anyhow::bail!(
                "Both registry_user and registry_pass must be provided for authenticated push"
            ),
        };

        let mut stream = docker.push_image(
            image_tag,
            Some(PushImageOptionsBuilder::default().build()),
            auth_config,
        );
        while let Some(message) = stream.next().await {
            let message = message?;
            if let Some(status) = message.status.as_deref() {
                info!("[DOCKER-PUSH] {}", status.trim_end());
            }
            if let Some(error) = message.error_detail.and_then(|detail| detail.message) {
                anyhow::bail!("Docker push failed: {}", error);
            }
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

    /// Safely logs stdout and stderr concurrently without data loss.
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

    async fn build_context(repo_path: &Path) -> Result<tempfile::NamedTempFile> {
        let repo_path = repo_path.to_path_buf();
        let dockerignore = Self::load_dockerignore(&repo_path)?;
        let archive = tokio::task::spawn_blocking(move || -> Result<tempfile::NamedTempFile> {
            let file = tempfile::NamedTempFile::new()
                .context("Failed to create temporary Docker build context")?;
            {
                let mut builder = tar::Builder::new(file.as_file());
                for entry in WalkDir::new(&repo_path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|entry| entry.ok())
                {
                    let path = entry.path();
                    if path == repo_path {
                        continue;
                    }

                    let rel = path
                        .strip_prefix(&repo_path)
                        .context("Failed to normalize Docker build context path")?;
                    if !Self::dockerignore_allows(&dockerignore, rel, path.is_dir()) {
                        continue;
                    }

                    let metadata = fs::symlink_metadata(path).with_context(|| {
                        format!("Failed to read metadata for {}", path.display())
                    })?;
                    if metadata.file_type().is_symlink() {
                        let target = fs::read_link(path).with_context(|| {
                            format!("Failed to resolve symlink {}", path.display())
                        })?;
                        let mut header = tar::Header::new_gnu();
                        header.set_entry_type(tar::EntryType::Symlink);
                        header.set_size(0);
                        header.set_cksum();
                        builder
                            .append_link(&mut header, rel, target)
                            .context("Failed to append symlink to Docker build context")?;
                    } else if metadata.is_dir() {
                        builder
                            .append_dir(rel, path)
                            .context("Failed to append directory to Docker build context")?;
                    } else {
                        builder
                            .append_path_with_name(path, rel)
                            .context("Failed to append file to Docker build context")?;
                    }
                }
                builder.finish().context("Failed to finish archive")?;
            }
            Ok(file)
        })
        .await
        .context("Failed to build Docker context archive")??;

        Ok(archive)
    }

    fn load_dockerignore(repo_path: &Path) -> Result<Vec<(bool, Pattern)>> {
        let path = repo_path.join(".dockerignore");
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut patterns = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (negated, pattern) = if let Some(rest) = trimmed.strip_prefix('!') {
                (true, rest.trim())
            } else {
                (false, trimmed)
            };

            if pattern.is_empty() {
                continue;
            }

            let pattern = pattern.trim_start_matches('/').trim_end_matches('/');
            patterns.push((
                negated,
                Pattern::new(pattern)
                    .with_context(|| format!("Invalid .dockerignore pattern: {pattern}"))?,
            ));
        }

        Ok(patterns)
    }

    fn dockerignore_allows(patterns: &[(bool, Pattern)], path: &Path, is_dir: bool) -> bool {
        let mut path_str = path.to_string_lossy().replace('\\', "/");
        if is_dir && !path_str.ends_with('/') {
            path_str.push('/');
        }

        let mut allowed = true;
        for (negated, pattern) in patterns {
            if pattern.matches(&path_str) {
                allowed = *negated;
            }
        }
        allowed
    }

    async fn detect_exposed_port(&self, image_tag: &str) -> Option<u32> {
        let docker = Docker::connect_with_local_defaults().ok()?;
        let image = docker.inspect_image(image_tag).await.ok()?;
        let ports = image.config?.exposed_ports?;
        ports
            .iter()
            .find_map(|port| port.split('/').next())
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
