use anyhow::Context;
use bollard::Docker;
use bollard::auth::DockerCredentials;
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, RemoveContainerOptionsBuilder,
    RemoveImageOptionsBuilder,
};
use futures::stream::StreamExt;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::process::Command;
use tracing::info;

#[derive(Debug)]
pub struct ImageBuilder;

pub struct DockerToExt4Params<'a> {
    pub image: &'a str,
    pub output_path: &'a Path,
    pub base_rootfs_path: &'a str,
    pub port: u32,
    pub ipv6_addr: Option<String>,
    pub ipv6_gw: Option<String>,
    pub volumes: &'a [crate::hypervisor::Volume],
    pub workload_type: i32,
}

pub struct DatabaseRootfsParams<'a> {
    pub output_path: &'a Path,
    pub base_rootfs_path: &'a Path,
    pub port: u32,
    pub ipv6_addr: Option<String>,
    pub ipv6_gw: Option<String>,
    pub env: &'a std::collections::HashMap<String, String>,
    pub volumes: &'a [crate::hypervisor::Volume],
    pub workload_type: i32,
}

impl ImageBuilder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub async fn docker_to_ext4(&self, params: DockerToExt4Params<'_>) -> anyhow::Result<()> {
        info!(
            "Converting Docker image {} to ext4 at {:?} (port={}, base={}, volumes={})",
            params.image,
            params.output_path,
            params.port,
            params.base_rootfs_path,
            params.volumes.len()
        );

        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;
        let parent_dir = params.output_path.parent().unwrap_or(Path::new("/tmp"));
        let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
        let mount_dir = parent_dir.join(format!("mnt-{container_name}"));
        let mounted = Arc::new(AtomicBool::new(false));
        let mounted_clone = Arc::clone(&mounted);

        let result = async {
            // 0. Pull the source image
            let mut pull_stream = docker.create_image(
                Some(
                    CreateImageOptionsBuilder::default()
                        .from_image(params.image)
                        .build(),
                ),
                None,
                self.registry_credentials(),
            );
            while let Some(message) = pull_stream.next().await {
                let message = message?;
                if let Some(status) = message.status.as_deref() {
                    info!("[DOCKER-PULL] {}", status.trim_end());
                }
                if let Some(error) = message.error_detail.and_then(|detail| detail.message) {
                    anyhow::bail!("Failed to pull docker image {}: {}", params.image, error);
                }
            }

            // 1. Inspect metadata (Extract Entrypoint and Cmd as structured data)
            let image_info = docker
                .inspect_image(params.image)
                .await
                .context("Failed to inspect docker image")?;
            let image_config = image_info.config.unwrap_or_default();
            let env_vars = image_config.env.unwrap_or_default();
            let entrypoint_list = image_config.entrypoint.unwrap_or_default();
            let cmd_list = image_config.cmd.unwrap_or_default();
            let workdir = image_config
                .working_dir
                .unwrap_or_else(|| "/app".to_string());
            let app_workdir = Self::normalize_workdir(&workdir)?;
            let app_entrypoint = Self::rewrite_program_path(&entrypoint_list, &workdir);
            let app_cmd = Self::rewrite_program_path(&cmd_list, &workdir);

            // 2. Create temporary container to export filesystem
            let container_config = bollard::models::ContainerCreateBody {
                image: Some(params.image.to_string()),
                env: Some(env_vars.clone()),
                entrypoint: Some(entrypoint_list.clone()),
                cmd: Some(cmd_list.clone()),
                working_dir: Some(workdir.clone()),
                ..Default::default()
            };
            docker
                .create_container(
                    Some(
                        CreateContainerOptionsBuilder::default()
                            .name(&container_name)
                            .build(),
                    ),
                    container_config,
                )
                .await
                .context("Failed to create temporary docker container")?;

            // 4. Prepare rootfs using Agent Overlay (Copy base-rootfs.ext4)
            info!(
                "Copying base rootfs from {} to {:?}...",
                params.base_rootfs_path, params.output_path
            );
            tokio::fs::copy(params.base_rootfs_path, params.output_path)
                .await
                .with_context(|| {
                    format!(
                        "Failed to copy base rootfs from {}",
                        params.base_rootfs_path
                    )
                })?;

            // 5. Mount and copy only the application payload
            tokio::fs::create_dir_all(&mount_dir).await?;

            let mount_dir_str = mount_dir.to_string_lossy();
            info!("Mounting image to {}...", mount_dir_str);
            let status = Command::new("mount")
                .arg("-o")
                .arg("loop")
                .arg(params.output_path)
                .arg(&mount_dir)
                .status()
                .await?;

            if !status.success() {
                anyhow::bail!("Failed to mount ext4 image");
            }
            mounted_clone.store(true, Ordering::SeqCst);

            // Copy the user app payload and the Railpack runtime shim/runtime binary.
            Self::copy_container_directory(&container_name, &mount_dir, &app_workdir, "app")
                .await?;
            Self::copy_container_directory_optional(
                &container_name,
                &mount_dir,
                Path::new("/mise"),
                "mise",
            )
            .await?;
            Self::copy_container_directory_optional(
                &container_name,
                &mount_dir,
                Path::new("/etc/mise"),
                "etc/mise",
            )
            .await?;
            Self::copy_container_directory_optional(
                &container_name,
                &mount_dir,
                Path::new("/opt/corepack"),
                "opt/corepack",
            )
            .await?;
            Self::copy_container_file_optional(
                &container_name,
                &mount_dir,
                Path::new("/usr/local/bin/mise"),
            )
            .await?;
            Self::copy_entrypoint_binary(
                &container_name,
                &mount_dir,
                app_entrypoint.first().map(String::as_str),
            )
            .await?;
            Self::maybe_grant_net_bind_service(
                &mount_dir,
                app_entrypoint.first().map(String::as_str),
                params.port,
            )
            .await?;

            // 7. Setup Mikrom Init (Binario estático)
            info!("Setting up mikrom-init...");

            // Inject the binary into the rootfs
            // We look for it in multiple locations to support both local dev and production
            let host_init_paths = [
                "/usr/bin/mikrom-init",
                "target/release/mikrom-init",
                "../target/release/mikrom-init",
                "target/x86_64-unknown-linux-musl/release/mikrom-init",
                "../target/x86_64-unknown-linux-musl/release/mikrom-init",
            ];

            let mut found_init = false;
            let dest_init_path = mount_dir.join("mikrom-init");

            for path in &host_init_paths {
                if tokio::fs::metadata(path).await.is_ok() {
                    info!(path = %path, "Found mikrom-init binary, inyecting...");
                    tokio::fs::copy(path, &dest_init_path)
                        .await
                        .context("Failed to copy mikrom-init binary")?;
                    found_init = true;
                    break;
                }
            }

            if !found_init {
                anyhow::bail!(
                    "CRITICAL: mikrom-init binary not found in any of the expected paths: {:?}. \
                     Run 'make build-init' to generate it.",
                    host_init_paths
                );
            }

            tokio::fs::set_permissions(&dest_init_path, fs::Permissions::from_mode(0o755))
                .await
                .context("Failed to mark mikrom-init executable")?;

            // Create /etc/mikrom/init.json
            let etc_dir = mount_dir.join("etc/mikrom");
            tokio::fs::create_dir_all(&etc_dir)
                .await
                .context("Failed to create /etc/mikrom in guest")?;

            let mut env_map = std::collections::HashMap::new();
            for env in env_vars {
                if let Some((key, val)) = env.split_once('=') {
                    env_map.insert(key.to_string(), val.to_string());
                }
            }
            let existing_path = env_map.get("PATH").cloned().unwrap_or_default();
            let mut path_parts = vec![
                "/app/node_modules/.bin".to_string(),
                "/mise/shims".to_string(),
                "/usr/local/bin".to_string(),
            ];
            for part in existing_path.split(':') {
                if !part.is_empty() && !path_parts.iter().any(|candidate| candidate == part) {
                    path_parts.push(part.to_string());
                }
            }
            env_map.insert("PATH".to_string(), path_parts.join(":"));
            env_map.insert("PORT".to_string(), params.port.to_string());
            if let Some(addr) = &params.ipv6_addr {
                env_map.insert("IPV6_ADDR".to_string(), addr.clone());
            }
            if let Some(gw) = &params.ipv6_gw {
                env_map.insert("IPV6_GW".to_string(), gw.clone());
            }

            let mut volumes_json = Vec::new();
            for (idx, vol) in params.volumes.iter().enumerate() {
                volumes_json.push(serde_json::json!({
                    "drive_id": vol.volume_id.replace('-', "_"),
                    "mount_point": vol.mount_point,
                    "index": idx + 1 // vda is index 0 (rootfs), so vdb is 1, etc.
                }));
            }

            let init_config = serde_json::json!({
                "env": env_map,
                "workdir": "/app",
                "entrypoint": app_entrypoint,
                "cmd": app_cmd,
                "volumes": volumes_json,
                "workload_type": workload_type_label(params.workload_type)
            });

            tokio::fs::write(
                etc_dir.join("init.json"),
                serde_json::to_string_pretty(&init_config)?,
            )
            .await
            .context("Failed to write init.json")?;

            info!("Successfully created ext4 rootfs for {}", params.image);
            Ok::<(), anyhow::Error>(())
        }
        .await;

        info!("Flushing and cleaning up...");
        // Flush filesystem buffers before teardown.
        // sync(2) is a synchronous syscall that can block for seconds,
        // so run it in a blocking task.
        let _ = tokio::task::spawn_blocking(|| unsafe { libc::sync() }).await;

        if mounted.load(Ordering::SeqCst) {
            match unmount_path(&mount_dir, 0) {
                Ok(()) => {},
                Err(e) => {
                    tracing::error!(
                        "Failed to unmount {}; filesystem may be corrupted: {}",
                        mount_dir.to_string_lossy(),
                        e
                    );
                    let _ = unmount_path(&mount_dir, libc::MNT_DETACH);
                },
            }
        }

        if let Err(e) = tokio::fs::remove_dir(&mount_dir).await {
            tracing::warn!("Failed to remove mount directory {:?}: {}", mount_dir, e);
        }
        let _ = docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
        let _ = docker
            .remove_image(
                params.image,
                Some(RemoveImageOptionsBuilder::default().force(true).build()),
                None,
            )
            .await;

        result
    }

    pub async fn database_to_ext4(&self, params: DatabaseRootfsParams<'_>) -> anyhow::Result<()> {
        info!(
            "Preparing database rootfs from {:?} to {:?} (port={}, volumes={})",
            params.base_rootfs_path,
            params.output_path,
            params.port,
            params.volumes.len()
        );

        let parent_dir = params.output_path.parent().unwrap_or(Path::new("/tmp"));
        let mount_dir = parent_dir.join(format!("mnt-mikrom-build-{}", uuid::Uuid::new_v4()));
        let mounted = Arc::new(AtomicBool::new(false));
        let mounted_clone = Arc::clone(&mounted);

        let result = async {
            info!(
                "Copying database base rootfs from {} to {:?}...",
                params.base_rootfs_path.display(),
                params.output_path
            );
            tokio::fs::copy(params.base_rootfs_path, params.output_path)
                .await
                .with_context(|| {
                    format!(
                        "Failed to copy database base rootfs from {}",
                        params.base_rootfs_path.display()
                    )
                })?;

            tokio::fs::create_dir_all(&mount_dir).await?;

            let mount_dir_str = mount_dir.to_string_lossy();
            info!("Mounting database image to {}...", mount_dir_str);
            let status = Command::new("mount")
                .arg("-o")
                .arg("loop")
                .arg(params.output_path)
                .arg(&mount_dir)
                .status()
                .await?;

            if !status.success() {
                anyhow::bail!("Failed to mount database ext4 image");
            }
            mounted_clone.store(true, Ordering::SeqCst);

            info!("Setting up mikrom-init for database workload...");
            let host_init_paths = [
                "/usr/bin/mikrom-init",
                "target/release/mikrom-init",
                "../target/release/mikrom-init",
                "target/x86_64-unknown-linux-musl/release/mikrom-init",
                "../target/x86_64-unknown-linux-musl/release/mikrom-init",
            ];

            let dest_init_path = mount_dir.join("mikrom-init");
            let mut found_init = false;
            for path in &host_init_paths {
                if tokio::fs::metadata(path).await.is_ok() {
                    info!(path = %path, "Found mikrom-init binary, injecting...");
                    tokio::fs::copy(path, &dest_init_path)
                        .await
                        .context("Failed to copy mikrom-init binary")?;
                    found_init = true;
                    break;
                }
            }

            if !found_init {
                anyhow::bail!(
                    "CRITICAL: mikrom-init binary not found in any of the expected paths: {:?}. \
                     Run 'make build-init' to generate it.",
                    host_init_paths
                );
            }

            tokio::fs::set_permissions(&dest_init_path, fs::Permissions::from_mode(0o755))
                .await
                .context("Failed to mark mikrom-init executable")?;

            let etc_dir = mount_dir.join("etc/mikrom");
            tokio::fs::create_dir_all(&etc_dir)
                .await
                .context("Failed to create /etc/mikrom in guest")?;

            let mut env_map = params.env.clone();
            env_map.insert("PORT".to_string(), params.port.to_string());
            if let Some(addr) = &params.ipv6_addr {
                env_map.insert("IPV6_ADDR".to_string(), addr.clone());
            }
            if let Some(gw) = &params.ipv6_gw {
                env_map.insert("IPV6_GW".to_string(), gw.clone());
            }
            let neon_tenant_id = env_map
                .get("NEON_TENANT_ID")
                .cloned()
                .context("NEON_TENANT_ID is required for database workloads")?;
            let neon_timeline_id = env_map
                .get("NEON_TIMELINE_ID")
                .cloned()
                .context("NEON_TIMELINE_ID is required for database workloads")?;
            env_map.insert("NEON_TENANT_ID".to_string(), neon_tenant_id);
            env_map.insert("NEON_TIMELINE_ID".to_string(), neon_timeline_id);

            let mut volumes_json = Vec::new();
            for (idx, vol) in params.volumes.iter().enumerate() {
                volumes_json.push(serde_json::json!({
                    "drive_id": vol.volume_id.replace('-', "_"),
                    "mount_point": vol.mount_point,
                    "index": idx + 1,
                }));
            }

            let init_config = serde_json::json!({
                "env": env_map,
                "entrypoint": Vec::<String>::new(),
                "cmd": Vec::<String>::new(),
                "volumes": volumes_json,
                "workload_type": workload_type_label(params.workload_type)
            });

            tokio::fs::write(
                etc_dir.join("init.json"),
                serde_json::to_string_pretty(&init_config)?,
            )
            .await
            .context("Failed to write init.json")?;

            info!("Successfully created database ext4 rootfs");
            Ok::<(), anyhow::Error>(())
        }
        .await;

        info!("Flushing and cleaning up database rootfs...");
        let _ = tokio::task::spawn_blocking(|| unsafe { libc::sync() }).await;

        if mounted.load(Ordering::SeqCst) {
            match unmount_path(&mount_dir, 0) {
                Ok(()) => {},
                Err(e) => {
                    tracing::error!(
                        "Failed to unmount {}; filesystem may be corrupted: {}",
                        mount_dir.to_string_lossy(),
                        e
                    );
                    let _ = unmount_path(&mount_dir, libc::MNT_DETACH);
                },
            }
        }

        if let Err(e) = tokio::fs::remove_dir(&mount_dir).await {
            tracing::warn!("Failed to remove mount directory {:?}: {}", mount_dir, e);
        }

        result
    }

    async fn copy_container_directory(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
        destination_name: &str,
    ) -> anyhow::Result<()> {
        let destination_root = mount_dir.join(destination_name);

        if tokio::fs::metadata(&destination_root).await.is_ok() {
            let meta = tokio::fs::metadata(&destination_root).await?;
            if meta.is_dir() {
                tokio::fs::remove_dir_all(&destination_root)
                    .await
                    .with_context(|| format!("Failed to clear {}", destination_root.display()))?;
            } else {
                tokio::fs::remove_file(&destination_root)
                    .await
                    .with_context(|| format!("Failed to clear {}", destination_root.display()))?;
            }
        }
        tokio::fs::create_dir_all(&destination_root)
            .await
            .with_context(|| format!("Failed to create {}", destination_root.display()))?;

        let source = format!(
            "{}:{}",
            container_name,
            Self::container_source_spec(source_path)
        );
        let output = Command::new("docker")
            .arg("cp")
            .arg(&source)
            .arg(&destination_root)
            .output()
            .await
            .with_context(|| {
                format!(
                    "Failed to invoke docker cp for {} payload",
                    destination_name
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to copy {} from container {} to {}: {}",
                source_path.display(),
                container_name,
                destination_root.display(),
                stderr.trim()
            );
        }

        tokio::fs::set_permissions(&destination_root, std::fs::Permissions::from_mode(0o755))
            .await
            .with_context(|| {
                format!(
                    "Failed to set permissions on {}",
                    destination_root.display()
                )
            })?;

        Ok(())
    }

    async fn copy_container_directory_optional(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
        destination_name: &str,
    ) -> anyhow::Result<()> {
        let destination_root = mount_dir.join(destination_name);
        match Self::copy_container_directory(
            container_name,
            mount_dir,
            source_path,
            destination_name,
        )
        .await
        {
            Ok(()) => Ok(()),
            Err(err) if Self::is_missing_container_path_error(&err) => {
                if let Err(cleanup_err) = tokio::fs::remove_dir_all(&destination_root).await
                    && cleanup_err.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(
                        path = %destination_root.display(),
                        error = %cleanup_err,
                        "Failed to clean optional destination after skipping missing payload"
                    );
                }
                info!(
                    source = %source_path.display(),
                    destination = %destination_name,
                    "Skipping optional runtime payload that is not present in the image"
                );
                Ok(())
            },
            Err(err) => Err(err),
        }
    }

    async fn copy_container_file(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
    ) -> anyhow::Result<()> {
        let destination = mount_dir.join(source_path.strip_prefix("/").unwrap_or(source_path));

        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        if tokio::fs::metadata(&destination).await.is_ok() {
            let meta = tokio::fs::metadata(&destination).await?;
            if meta.is_dir() {
                tokio::fs::remove_dir_all(&destination)
                    .await
                    .with_context(|| format!("Failed to clear {}", destination.display()))?;
            } else {
                tokio::fs::remove_file(&destination)
                    .await
                    .with_context(|| format!("Failed to clear {}", destination.display()))?;
            }
        }

        let source = format!(
            "{}:{}",
            container_name,
            source_path.to_string_lossy().trim_end_matches('/')
        );
        let output = Command::new("docker")
            .arg("cp")
            .arg(&source)
            .arg(&destination)
            .output()
            .await
            .context("Failed to invoke docker cp for runtime binary")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to copy {} from container {} to {}: {}",
                source_path.display(),
                container_name,
                destination.display(),
                stderr.trim()
            );
        }

        tokio::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
            .await
            .with_context(|| format!("Failed to set permissions on {}", destination.display()))?;

        Ok(())
    }

    async fn copy_container_file_optional(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
    ) -> anyhow::Result<()> {
        let destination = mount_dir.join(source_path.strip_prefix("/").unwrap_or(source_path));
        match Self::copy_container_file(container_name, mount_dir, source_path).await {
            Ok(()) => Ok(()),
            Err(err) if Self::is_missing_container_path_error(&err) => {
                if let Some(parent) = destination.parent()
                    && let Err(cleanup_err) = tokio::fs::remove_dir_all(parent).await
                    && cleanup_err.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(
                        path = %parent.display(),
                        error = %cleanup_err,
                        "Failed to clean optional file destination after skipping missing payload"
                    );
                }
                info!(
                    source = %source_path.display(),
                    "Skipping optional runtime binary that is not present in the image"
                );
                Ok(())
            },
            Err(err) => Err(err),
        }
    }

    async fn copy_entrypoint_binary(
        container_name: &str,
        mount_dir: &Path,
        entrypoint: Option<&str>,
    ) -> anyhow::Result<()> {
        let Some(entrypoint) = entrypoint else {
            return Ok(());
        };

        let source_path = Path::new(entrypoint);
        if !source_path.is_absolute() {
            return Ok(());
        }

        let ignored_prefixes = [
            "/bin/",
            "/sbin/",
            "/usr/bin/",
            "/usr/sbin/",
            "/lib/",
            "/lib64/",
            "/usr/lib/",
            "/usr/lib64/",
            "/app/",
            "/mise/",
            "/etc/mise/",
            "/opt/corepack/",
        ];

        if ignored_prefixes
            .iter()
            .any(|prefix| entrypoint.starts_with(prefix))
        {
            return Ok(());
        }

        Self::copy_container_file(container_name, mount_dir, source_path).await
    }

    async fn maybe_grant_net_bind_service(
        mount_dir: &Path,
        entrypoint: Option<&str>,
        port: u32,
    ) -> anyhow::Result<()> {
        let Some(entrypoint) = entrypoint else {
            return Ok(());
        };

        if !Self::needs_net_bind_service(entrypoint, port) {
            return Ok(());
        }

        let destination = mount_dir.join(entrypoint.trim_start_matches('/'));
        let metadata = tokio::fs::metadata(&destination).await.with_context(|| {
            format!(
                "Failed to inspect entrypoint binary for CAP_NET_BIND_SERVICE: {}",
                destination.display()
            )
        })?;

        if !metadata.is_file() {
            anyhow::bail!(
                "Entrypoint target is not a regular file: {}",
                destination.display()
            );
        }

        let output = Command::new("setcap")
            .arg("cap_net_bind_service=+ep")
            .arg(&destination)
            .output()
            .await
            .with_context(|| {
                format!(
                    "Failed to invoke setcap for entrypoint binary {}",
                    destination.display()
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to grant CAP_NET_BIND_SERVICE to {}: {}",
                destination.display(),
                stderr.trim()
            );
        }

        info!(
            entrypoint = %entrypoint,
            destination = %destination.display(),
            "Granted CAP_NET_BIND_SERVICE to entrypoint binary"
        );

        Ok(())
    }

    fn needs_net_bind_service(entrypoint: &str, port: u32) -> bool {
        if port >= 1024 {
            return false;
        }

        let ignored_prefixes = [
            "/bin/",
            "/sbin/",
            "/usr/bin/",
            "/usr/sbin/",
            "/lib/",
            "/lib64/",
            "/usr/lib/",
            "/usr/lib64/",
        ];

        entrypoint.starts_with('/')
            && !ignored_prefixes
                .iter()
                .any(|prefix| entrypoint.starts_with(prefix))
    }

    fn normalize_workdir(workdir: &str) -> anyhow::Result<PathBuf> {
        let trimmed = workdir.trim();
        let without_leading = trimmed.trim_start_matches('/');
        let normalized = without_leading.trim_end_matches('/');

        if normalized.is_empty() {
            anyhow::bail!("WORKDIR must point to a non-root directory");
        }

        Ok(PathBuf::from(normalized))
    }

    fn rewrite_program_path(args: &[String], original_workdir: &str) -> Vec<String> {
        let Some((program, rest)) = args.split_first() else {
            return Vec::new();
        };

        let source_workdir = format!(
            "/{}",
            original_workdir
                .trim_start_matches('/')
                .trim_end_matches('/')
        );
        let rewritten_program = if program.starts_with(&source_workdir) {
            let suffix = &program[source_workdir.len()..];
            if suffix.is_empty() {
                "/app".to_string()
            } else {
                format!("/app{suffix}")
            }
        } else {
            program.clone()
        };

        let mut rewritten = Vec::with_capacity(args.len());
        rewritten.push(rewritten_program);
        rewritten.extend(rest.iter().cloned());
        rewritten
    }

    fn container_source_spec(source_path: &Path) -> String {
        let source_path = source_path.to_string_lossy();
        let normalized = source_path.trim_start_matches('/').trim_end_matches('/');

        format!("/{normalized}/.")
    }

    fn is_missing_container_path_error(err: &anyhow::Error) -> bool {
        let err = err.to_string().to_lowercase();
        err.contains("could not find the file")
            || err.contains("no such file or directory")
            || err.contains("not found in container")
            || err.contains("path does not exist")
    }

    fn registry_credentials(&self) -> Option<DockerCredentials> {
        let user = std::env::var("REGISTRY_USER").ok()?;
        let pass = std::env::var("REGISTRY_PASS").ok()?;

        Some(DockerCredentials {
            username: Some(user),
            password: Some(pass),
            ..Default::default()
        })
    }
}

fn workload_type_label(workload_type: i32) -> &'static str {
    if workload_type == 1 {
        "DATABASE"
    } else {
        "APP"
    }
}

fn unmount_path(path: &Path, flags: libc::c_int) -> anyhow::Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .context("Failed to convert mount path to C string")?;

    // SAFETY: libc::umount2 is a direct syscall wrapper. The path comes from a
    // valid Path and the flags are provided by the caller.
    let rc = unsafe { libc::umount2(c_path.as_ptr(), flags) };
    if rc != 0 {
        Err(std::io::Error::last_os_error()).context("umount2 failed")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_builder_new() {
        let _ = ImageBuilder::new();
    }

    #[test]
    fn test_rewrite_program_path_maps_workdir_executable_to_app() {
        let args = vec!["/srv/app/bin/server".to_string(), "--flag".to_string()];
        let rewritten = ImageBuilder::rewrite_program_path(&args, "/srv/app");
        assert_eq!(rewritten[0], "/app/bin/server");
        assert_eq!(rewritten[1], "--flag");
    }

    #[test]
    fn test_rewrite_program_path_leaves_external_binary_untouched() {
        let args = vec!["/usr/bin/node".to_string(), "server.js".to_string()];
        let rewritten = ImageBuilder::rewrite_program_path(&args, "/srv/app");
        assert_eq!(rewritten, args);
    }

    #[test]
    fn test_rewrite_program_path_handles_empty_args() {
        let rewritten = ImageBuilder::rewrite_program_path(&[], "/srv/app");
        assert!(rewritten.is_empty());
    }

    #[test]
    fn test_normalize_workdir_rejects_root() {
        assert!(ImageBuilder::normalize_workdir("/").is_err());
    }

    #[test]
    fn test_normalize_workdir_trims_slashes() {
        let normalized = ImageBuilder::normalize_workdir("/srv/app/").unwrap();
        assert_eq!(normalized, PathBuf::from("srv/app"));
    }

    #[test]
    fn test_container_source_spec_normalizes_paths() {
        assert_eq!(
            ImageBuilder::container_source_spec(Path::new("/app")),
            "/app/."
        );
        assert_eq!(
            ImageBuilder::container_source_spec(Path::new("/mise")),
            "/mise/."
        );
    }

    #[test]
    fn test_copy_container_file_builds_destination_path() {
        let dir = tempdir().unwrap();
        let mount_dir = dir.path().join("mount");
        fs::create_dir_all(&mount_dir).unwrap();

        let destination = mount_dir.join("usr/local/bin/mise");
        assert_eq!(destination, mount_dir.join("usr/local/bin/mise"));
    }

    #[test]
    fn test_copy_container_directory_builds_destination_path() {
        let dir = tempdir().unwrap();
        let mount_dir = dir.path().join("mount");
        fs::create_dir_all(&mount_dir).unwrap();

        let destination = mount_dir.join("mise");
        assert_eq!(destination, mount_dir.join("mise"));
    }

    #[test]
    fn test_is_missing_container_path_error_matches_docker_missing_path_messages() {
        let err = anyhow::anyhow!(
            "docker cp failed: Error response from daemon: Could not find the file /mise in container"
        );
        assert!(ImageBuilder::is_missing_container_path_error(&err));
    }

    #[test]
    fn test_needs_net_bind_service() {
        assert!(ImageBuilder::needs_net_bind_service("/whoami", 80));
        assert!(ImageBuilder::needs_net_bind_service("/app/bin/server", 80));
        assert!(!ImageBuilder::needs_net_bind_service("/bin/bash", 80));
        assert!(!ImageBuilder::needs_net_bind_service("/usr/bin/node", 80));
        assert!(!ImageBuilder::needs_net_bind_service("/whoami", 8080));
    }

    #[tokio::test]
    async fn test_copy_entrypoint_binary_ignores_system_and_runtime_paths() {
        let dir = tempdir().unwrap();
        let mount_dir = dir.path().join("mount");
        fs::create_dir_all(&mount_dir).unwrap();

        assert!(
            ImageBuilder::copy_entrypoint_binary("container", &mount_dir, Some("/bin/bash"))
                .await
                .is_ok()
        );
        assert!(
            ImageBuilder::copy_entrypoint_binary("container", &mount_dir, Some("/usr/bin/node"))
                .await
                .is_ok()
        );
        assert!(
            ImageBuilder::copy_entrypoint_binary("container", &mount_dir, Some("/mise/shims/pnpm"))
                .await
                .is_ok()
        );
    }

    #[test]
    fn test_rewrite_program_path_exact_workdir_maps_to_app() {
        let args = vec!["/srv/app".to_string(), "--flag".to_string()];
        let rewritten = ImageBuilder::rewrite_program_path(&args, "/srv/app");
        assert_eq!(rewritten[0], "/app");
        assert_eq!(rewritten[1], "--flag");
    }

    #[tokio::test]
    async fn test_docker_to_ext4_invalid_image() {
        let builder = ImageBuilder::new().unwrap();
        let temp_path = PathBuf::from("/tmp/test-invalid-image.ext4");
        let result = builder
            .docker_to_ext4(crate::builder::DockerToExt4Params {
                image: "nonexistent-image-12345",
                output_path: &temp_path,
                base_rootfs_path: "/tmp/nonexistent-base.ext4",
                port: 8080,
                ipv6_addr: None,
                ipv6_gw: None,
                volumes: &[],
                workload_type: 0,
            })
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_workload_type_label_maps_database() {
        assert_eq!(workload_type_label(1), "DATABASE");
        assert_eq!(workload_type_label(0), "APP");
    }
}
