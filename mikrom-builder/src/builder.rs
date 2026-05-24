use anyhow::{Context, Result};
use bollard::Docker;
use bollard::auth::DockerCredentials;
use bollard::body_stream;
use bollard::query_parameters::{
    BuildImageOptionsBuilder, PushImageOptionsBuilder, RemoveImageOptionsBuilder,
};
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
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument, warn};
use walkdir::WalkDir;

pub struct AppBuilder {
    registry: String,
    registry_user: Option<String>,
    registry_pass: Option<String>,
}

#[derive(Debug)]
pub struct BuildResult {
    pub image_tag: String,
    pub exposed_port: u32,
}

#[derive(Clone, Debug)]
pub struct GitMetadata {
    pub hash: String,
    pub message: String,
    pub branch: String,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum BuildBackend {
    Dockerfile,
    Railpack,
}

impl AppBuilder {
    pub fn new(
        registry: String,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn build_image(
        &self,
        app_id: &str,
        git_url: &str,
        git_auth_token: Option<String>,
        image_name: &str,
        tag: &str,
        cancel: CancellationToken,
        metadata_tx: Option<tokio::sync::mpsc::Sender<GitMetadata>>,
    ) -> Result<BuildResult> {
        Self::validate_build_inputs(git_url, image_name, tag)?;
        let checkout_dir = TempDir::new().context("Failed to create checkout directory")?;
        Self::check_cancelled(&cancel)?;
        let repo = Self::clone_repository(git_url, git_auth_token, checkout_dir.path())?;
        let metadata = Self::extract_git_metadata(&repo)?;

        if let Some(tx) = metadata_tx {
            let _ = tx.send(metadata.clone()).await;
        }

        let workspace_dir = TempDir::new().context("Failed to create build workspace")?;
        Self::check_cancelled(&cancel)?;
        Self::copy_workspace(checkout_dir.path(), workspace_dir.path())?;
        Self::apply_prebuild_fixes(workspace_dir.path())?;

        let full_image_tag = self.format_image_tag(image_name, tag);
        let backend = Self::detect_build_backend(workspace_dir.path());

        match backend {
            BuildBackend::Dockerfile => {
                self.run_docker_build(workspace_dir.path(), &full_image_tag, &cancel)
                    .await?
            },
            BuildBackend::Railpack => {
                self.run_railpack_build(workspace_dir.path(), &full_image_tag, &cancel)
                    .await?
            },
        }

        let exposed_port = self
            .detect_exposed_port(&full_image_tag)
            .await
            .unwrap_or(8080);

        let push_result = self.push_to_registry(&full_image_tag).await;
        self.cleanup_local_image(&full_image_tag).await;
        push_result?;

        Ok(BuildResult {
            image_tag: full_image_tag,
            exposed_port,
        })
    }

    fn validate_build_inputs(git_url: &str, image_name: &str, tag: &str) -> Result<()> {
        if image_name.trim().is_empty() {
            anyhow::bail!("image_name cannot be empty");
        }
        if tag.trim().is_empty() {
            anyhow::bail!("tag cannot be empty");
        }
        if image_name.contains('/') || image_name.contains("..") || image_name.contains('\\') {
            anyhow::bail!("image_name contains invalid path separators");
        }
        if tag.contains('/') || tag.contains('\\') || tag.contains("..") {
            anyhow::bail!("tag contains invalid path separators");
        }
        if !Self::git_url_allowed(git_url) {
            anyhow::bail!(
                "git_url scheme is not allowed; only http(s), ssh and git-style URLs are supported"
            );
        }
        Ok(())
    }

    fn git_url_allowed(git_url: &str) -> bool {
        let lower = git_url.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return false;
        }

        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("ssh://")
            || lower.starts_with("git://")
            || lower.starts_with("git@")
        {
            return true;
        }

        !lower.contains("://")
    }

    fn check_cancelled(cancel: &CancellationToken) -> Result<()> {
        if cancel.is_cancelled() {
            anyhow::bail!("Build cancelled");
        }
        Ok(())
    }

    fn clone_repository(
        git_url: &str,
        git_auth_token: Option<String>,
        repo_path: &Path,
    ) -> Result<Repository> {
        info!(git_url = %git_url, "Cloning repository");

        if let Some(token) = git_auth_token {
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
                .context("Failed to clone private repository")
        } else {
            Repository::clone(git_url, repo_path).context("Failed to clone repository")
        }
    }

    fn copy_workspace(checkout_path: &Path, workspace_path: &Path) -> Result<()> {
        for entry in WalkDir::new(checkout_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let path = entry.path();
            if path == checkout_path {
                continue;
            }

            let rel = path
                .strip_prefix(checkout_path)
                .context("Failed to normalize workspace path")?;
            if rel.starts_with(".git") {
                continue;
            }

            let destination = workspace_path.join(rel);
            let metadata = fs::symlink_metadata(path)
                .with_context(|| format!("Failed to read metadata for {}", path.display()))?;

            if metadata.file_type().is_symlink() {
                let target = fs::read_link(path)
                    .with_context(|| format!("Failed to resolve symlink {}", path.display()))?;
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "Failed to create parent directory for {}",
                            destination.display()
                        )
                    })?;
                }

                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &destination).with_context(|| {
                    format!(
                        "Failed to create symlink {} -> {}",
                        destination.display(),
                        target.display()
                    )
                })?;

                #[cfg(not(unix))]
                {
                    let content = fs::read(path).with_context(|| {
                        format!("Failed to read symlink source {}", path.display())
                    })?;
                    fs::write(&destination, content).with_context(|| {
                        format!("Failed to materialize symlink {}", destination.display())
                    })?;
                }
            } else if metadata.is_dir() {
                fs::create_dir_all(&destination).with_context(|| {
                    format!("Failed to create directory {}", destination.display())
                })?;
            } else {
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "Failed to create parent directory for {}",
                            destination.display()
                        )
                    })?;
                }
                fs::copy(path, &destination).with_context(|| {
                    format!(
                        "Failed to copy {} to {}",
                        path.display(),
                        destination.display()
                    )
                })?;
            }
        }

        Ok(())
    }

    fn detect_build_backend(workspace_path: &Path) -> BuildBackend {
        if workspace_path.join("Dockerfile").exists() {
            BuildBackend::Dockerfile
        } else {
            BuildBackend::Railpack
        }
    }

    fn format_image_tag(&self, image_name: &str, tag: &str) -> String {
        format!("{}/{}:{}", self.normalized_registry(), image_name, tag)
    }

    fn normalized_registry(&self) -> &str {
        self.registry
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
    }

    async fn run_docker_build(
        &self,
        workspace_path: &Path,
        image_tag: &str,
        cancel: &CancellationToken,
    ) -> Result<()> {
        info!(image_tag = %image_tag, "Dockerfile detected, using docker build");
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;
        let context = Self::build_context(workspace_path).await?;
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
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    anyhow::bail!("Build cancelled");
                },
                message = stream.next() => {
                    let Some(message) = message else {
                        break;
                    };
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
            }
        }
        drop(stream);
        if cancel.is_cancelled() {
            anyhow::bail!("Build cancelled");
        }

        reader_task
            .await
            .context("Docker build context reader failed")??;
        Ok(())
    }

    async fn run_railpack_build(
        &self,
        workspace_path: &Path,
        image_tag: &str,
        cancel: &CancellationToken,
    ) -> Result<()> {
        info!(image_tag = %image_tag, "No Dockerfile found, using Railpack");
        let buildkit_host = std::env::var("BUILDKIT_HOST")
            .unwrap_or_else(|_| "docker-container://mikromrust-buildkit-1".to_string());

        let mut child = Command::new("railpack")
            .args(["build", ".", "--name", image_tag])
            .current_dir(workspace_path)
            .env("BUILDKIT_HOST", buildkit_host)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn railpack build")?;

        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                anyhow::bail!("Build cancelled");
            },
            result = Self::log_output(&mut child) => {
                result?;
            },
        }

        let status = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                anyhow::bail!("Build cancelled");
            },
            status = child.wait() => status?,
        };
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

    async fn cleanup_local_image(&self, image_tag: &str) {
        match Docker::connect_with_local_defaults() {
            Ok(docker) => {
                let _ = docker
                    .remove_image(
                        image_tag,
                        Some(RemoveImageOptionsBuilder::default().force(true).build()),
                        None,
                    )
                    .await;
                info!(image_tag = %image_tag, "Cleaned up local image after push");
            },
            Err(e) => {
                warn!(
                    image_tag = %image_tag,
                    error = %e,
                    "Failed to connect to Docker for image cleanup"
                );
            },
        }
    }

    /// Logs stdout and stderr from the build backend concurrently.
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
        let branch = head.shorthand().unwrap_or("detached").to_string();
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
            if pattern == "*" {
                // An unconditional global ignore must still allow the build metadata and Dockerfile.
                continue;
            }
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

        let dockerfile_path = repo_path.join("Dockerfile");
        if dockerfile_path.exists() {
            let content = std::fs::read_to_string(&dockerfile_path)?;
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let mut expose_found = false;

            for line in &lines {
                if line.trim().to_uppercase().starts_with("EXPOSE") {
                    expose_found = true;
                    break;
                }
            }

            if !expose_found {
                info!("No EXPOSE found, injecting PORT 8080 defaults across all stages");
                let mut final_lines = Vec::new();
                for line in lines {
                    final_lines.push(line.clone());
                    if line.trim().to_uppercase().starts_with("FROM") {
                        final_lines.push("ENV PORT=8080".to_string());
                        final_lines.push("EXPOSE 8080".to_string());
                    }
                }
                std::fs::write(&dockerfile_path, final_lines.join("\n"))?;
            } else {
                info!("EXPOSE detected in Dockerfile, respecting user port configuration");
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
    use tokio_util::sync::CancellationToken;

    #[test]
    fn test_normalized_registry_trims_scheme_and_slash() {
        let builder = AppBuilder::new("https://registry.example.com/".to_string(), None, None);

        assert_eq!(
            builder.format_image_tag("app", "v1"),
            "registry.example.com/app:v1"
        );
    }

    #[test]
    fn test_validate_build_inputs_rejects_invalid_git_url() {
        let err = AppBuilder::validate_build_inputs("file:///etc/passwd", "app", "latest")
            .expect_err("file:// URLs must be rejected");
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn test_validate_build_inputs_rejects_invalid_image_name() {
        let err =
            AppBuilder::validate_build_inputs("https://example.com/repo.git", "foo/bar", "latest")
                .expect_err("image names with separators must be rejected");
        assert!(err.to_string().contains("image_name"));
    }

    #[tokio::test]
    async fn test_build_image_can_be_cancelled_before_work_begins() {
        let builder = AppBuilder::new("localhost:5000".to_string(), None, None);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let err = builder
            .build_image(
                "app-1",
                "https://example.com/repo.git",
                None,
                "app",
                "latest",
                cancel,
                None,
            )
            .await
            .expect_err("cancelled build should fail");

        assert!(err.to_string().contains("cancelled"));
    }

    #[test]
    fn test_workspace_copy_keeps_checkout_pristine() {
        let checkout = tempdir().unwrap();
        let workspace = tempdir().unwrap();

        fs::write(checkout.path().join("package.json"), r#"{"name":"test"}"#).unwrap();
        fs::write(checkout.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::write(
            checkout.path().join("Dockerfile"),
            "FROM node:20\nCMD [\"node\"]",
        )
        .unwrap();

        AppBuilder::copy_workspace(checkout.path(), workspace.path()).unwrap();
        AppBuilder::apply_prebuild_fixes(workspace.path()).unwrap();

        let checkout_pkg = fs::read_to_string(checkout.path().join("package.json")).unwrap();
        let workspace_pkg = fs::read_to_string(workspace.path().join("package.json")).unwrap();

        assert!(!checkout_pkg.contains("packageManager"));
        assert!(workspace_pkg.contains("packageManager"));
    }

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

    #[test]
    fn test_apply_prebuild_fixes_respects_existing_expose() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        let dockerfile = "FROM node:20\nEXPOSE 80\nCMD [\"node\", \"index.js\"]";
        fs::write(repo_path.join("Dockerfile"), dockerfile).unwrap();

        AppBuilder::apply_prebuild_fixes(repo_path).unwrap();

        let new_content = fs::read_to_string(repo_path.join("Dockerfile")).unwrap();
        assert!(new_content.contains("EXPOSE 80"));
        assert!(!new_content.contains("8080"));
    }

    #[test]
    fn test_apply_prebuild_fixes_enforces_port_if_missing() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        let dockerfile = "FROM node:20\nCMD [\"node\", \"index.js\"]";
        fs::write(repo_path.join("Dockerfile"), dockerfile).unwrap();

        AppBuilder::apply_prebuild_fixes(repo_path).unwrap();

        let new_content = fs::read_to_string(repo_path.join("Dockerfile")).unwrap();
        assert!(new_content.contains("EXPOSE 8080"));
        assert!(new_content.contains("ENV PORT=8080"));
    }

    #[test]
    fn test_apply_prebuild_fixes_multi_stage_injection() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path();

        let dockerfile =
            "FROM node:20 AS builder\nRUN build.sh\n\nFROM alpine\nCOPY --from=builder /app /app";
        fs::write(repo_path.join("Dockerfile"), dockerfile).unwrap();

        AppBuilder::apply_prebuild_fixes(repo_path).unwrap();

        let new_content = fs::read_to_string(repo_path.join("Dockerfile")).unwrap();
        let env_count = new_content.matches("ENV PORT=8080").count();
        let expose_count = new_content.matches("EXPOSE 8080").count();
        assert_eq!(env_count, 2);
        assert_eq!(expose_count, 2);
    }
}
