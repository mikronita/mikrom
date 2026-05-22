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
use tokio::process::Command;
use tracing::info;

pub struct ImageBuilder;

impl ImageBuilder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn docker_to_ext4(
        &self,
        image: &str,
        output_path: &Path,
        base_rootfs_path: &str,
        port: u32,
        ipv6_addr: Option<String>,
        ipv6_gw: Option<String>,
        volumes: &[crate::firecracker::config::Volume],
    ) -> anyhow::Result<()> {
        info!(
            "Converting Docker image {} to ext4 at {:?} (port={}, base={}, volumes={})",
            image,
            output_path,
            port,
            base_rootfs_path,
            volumes.len()
        );

        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;
        let parent_dir = output_path.parent().unwrap_or(Path::new("/tmp"));
        let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
        let mount_dir = parent_dir.join(format!("mnt-{container_name}"));

        let result = async {
            // 0. Pull the source image
            let mut pull_stream = docker.create_image(
                Some(
                    CreateImageOptionsBuilder::default()
                        .from_image(image)
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
                    anyhow::bail!("Failed to pull docker image {image}: {}", error);
                }
            }

            // 1. Inspect metadata (Extract Entrypoint and Cmd as structured data)
            let image_info = docker
                .inspect_image(image)
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
                image: Some(image.to_string()),
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
                base_rootfs_path, output_path
            );
            tokio::fs::copy(base_rootfs_path, &output_path)
                .await
                .with_context(|| format!("Failed to copy base rootfs from {}", base_rootfs_path))?;

            // 5. Mount and copy only the application payload
            tokio::fs::create_dir_all(&mount_dir).await?;

            let mount_dir_str = mount_dir.to_string_lossy();
            info!("Mounting image to {}...", mount_dir_str);
            let status = Command::new("mount")
                .arg("-o")
                .arg("loop")
                .arg(output_path)
                .arg(&mount_dir)
                .status()
                .await?;

            if !status.success() {
                anyhow::bail!("Failed to mount ext4 image");
            }

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
                port,
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
            env_map.insert("PORT".to_string(), port.to_string());
            if let Some(addr) = ipv6_addr {
                env_map.insert("IPV6_ADDR".to_string(), addr);
            }
            if let Some(gw) = ipv6_gw {
                env_map.insert("IPV6_GW".to_string(), gw);
            }

            let mut volumes_json = Vec::new();
            for (idx, vol) in volumes.iter().enumerate() {
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
                "volumes": volumes_json
            });

            tokio::fs::write(
                etc_dir.join("init.json"),
                serde_json::to_string_pretty(&init_config)?,
            )
            .await
            .context("Failed to write init.json")?;

            info!("Successfully created ext4 rootfs for {}", image);
            Ok::<(), anyhow::Error>(())
        }
        .await;

        info!("Flushing and cleaning up...");
        // Flush filesystem buffers before teardown.
        // SAFETY: libc::sync has no arguments and only requests a global sync.
        unsafe {
            libc::sync();
        }

        if let Err(e) = unmount_path(&mount_dir, 0) {
            tracing::error!(
                "Failed to unmount {}; filesystem may be corrupted: {}",
                mount_dir.to_string_lossy(),
                e
            );
            let _ = unmount_path(&mount_dir, libc::MNT_DETACH);
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
                image,
                Some(RemoveImageOptionsBuilder::default().force(true).build()),
                None,
            )
            .await;

        result
    }

    async fn copy_container_directory(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
        destination_name: &str,
    ) -> anyhow::Result<()> {
        let destination_root = mount_dir.join(destination_name);

        if destination_root.exists() {
            if destination_root.is_dir() {
                fs::remove_dir_all(&destination_root)
                    .with_context(|| format!("Failed to clear {}", destination_root.display()))?;
            } else {
                fs::remove_file(&destination_root)
                    .with_context(|| format!("Failed to clear {}", destination_root.display()))?;
            }
        }
        fs::create_dir_all(&destination_root)
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

        fs::set_permissions(&destination_root, fs::Permissions::from_mode(0o755)).with_context(
            || {
                format!(
                    "Failed to set permissions on {}",
                    destination_root.display()
                )
            },
        )?;

        Ok(())
    }

    async fn copy_container_directory_optional(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
        destination_name: &str,
    ) -> anyhow::Result<()> {
        match Self::copy_container_directory(
            container_name,
            mount_dir,
            source_path,
            destination_name,
        )
        .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                if Self::is_missing_container_payload_error(&err.to_string()) {
                    info!(
                        source = %source_path.display(),
                        destination = %destination_name,
                        "Skipping optional runtime payload that is not present in the image"
                    );
                    Ok(())
                } else {
                    Err(err)
                }
            },
        }
    }

    async fn copy_container_file(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
    ) -> anyhow::Result<()> {
        let destination = mount_dir.join(source_path.strip_prefix("/").unwrap_or(source_path));

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        if destination.exists() {
            if destination.is_dir() {
                fs::remove_dir_all(&destination)
                    .with_context(|| format!("Failed to clear {}", destination.display()))?;
            } else {
                fs::remove_file(&destination)
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

        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", destination.display()))?;

        Ok(())
    }

    async fn copy_container_file_optional(
        container_name: &str,
        mount_dir: &Path,
        source_path: &Path,
    ) -> anyhow::Result<()> {
        match Self::copy_container_file(container_name, mount_dir, source_path).await {
            Ok(()) => Ok(()),
            Err(err) => {
                if Self::is_missing_container_payload_error(&err.to_string()) {
                    info!(
                        source = %source_path.display(),
                        "Skipping optional runtime binary that is not present in the image"
                    );
                    Ok(())
                } else {
                    Err(err)
                }
            },
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

    fn is_missing_container_payload_error(error_text: &str) -> bool {
        let lowered = error_text.to_lowercase();
        lowered.contains("could not find the file")
            || lowered.contains("no such file")
            || lowered.contains("not found")
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
    fn test_missing_container_payload_error_detection_is_case_insensitive() {
        assert!(ImageBuilder::is_missing_container_payload_error(
            "docker cp: Could Not Find The File"
        ));
        assert!(ImageBuilder::is_missing_container_payload_error(
            "docker cp: no such file or directory"
        ));
        assert!(ImageBuilder::is_missing_container_payload_error(
            "source not found in container"
        ));
        assert!(!ImageBuilder::is_missing_container_payload_error(
            "permission denied"
        ));
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
            .docker_to_ext4(
                "nonexistent-image-12345",
                &temp_path,
                "/tmp/nonexistent-base.ext4",
                8080,
                None,
                None,
                &[],
            )
            .await;
        assert!(result.is_err());
    }
}
