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
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::info;

pub struct ImageBuilder;

impl ImageBuilder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub async fn docker_to_ext4(
        &self,
        image: &str,
        output_path: &Path,
        base_rootfs_path: &str,
        port: u32,
        ipv6_addr: Option<String>,
        ipv6_gw: Option<String>,
    ) -> anyhow::Result<()> {
        info!(
            "Converting Docker image {} to ext4 at {:?} (port={}, base={})",
            image, output_path, port, base_rootfs_path
        );

        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to the local Docker daemon")?;
        let parent_dir = output_path.parent().unwrap_or(Path::new("/tmp"));
        let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
        let mount_dir = parent_dir.join(format!("mnt-{container_name}"));
        let export_tar_path = parent_dir.join(format!("{container_name}.tar"));

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
            info!("Copying base rootfs from {} to {:?}...", base_rootfs_path, output_path);
            tokio::fs::copy(base_rootfs_path, &output_path)
                .await
                .with_context(|| format!("Failed to copy base rootfs from {}", base_rootfs_path))?;

            // 5. Mount and copy files
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

            // Export the container filesystem and unpack it with the tar crate.
            let mut export_stream = docker.export_container(&container_name);
            let mut export_tar = tokio::fs::File::create(&export_tar_path)
                .await
                .context("Failed to create temporary export archive")?;
            while let Some(chunk) = export_stream.next().await {
                let chunk = chunk.context("Failed to stream container export")?;
                export_tar
                    .write_all(&chunk)
                    .await
                    .context("Failed to write container export archive")?;
            }
            export_tar
                .flush()
                .await
                .context("Failed to flush export archive")?;

            let mount_dir_str = mount_dir.to_string_lossy();
            let export_tar_str = export_tar_path.to_string_lossy();
            
            info!("Extracting container archive to {} (surgical system protection)...", mount_dir_str);
            let status = Command::new("tar")
                .arg("-xf")
                .arg(&*export_tar_str)
                .arg("-C")
                .arg(&*mount_dir_str)
                .arg("--overwrite")
                // Protect critical OS libraries (GLIBC) to prevent breaking sshd/system tools
                .arg("--exclude=lib/x86_64-linux-gnu")
                .arg("--exclude=lib64")
                .arg("--exclude=usr/lib/x86_64-linux-gnu")
                // Protect core identity and security config
                .arg("--exclude=etc/passwd")
                .arg("--exclude=etc/shadow")
                .arg("--exclude=etc/group")
                .arg("--exclude=etc/gshadow")
                .arg("--exclude=etc/ssh")
                .arg("--exclude=etc/hostname")
                .arg("--exclude=etc/hosts")
                .arg("--exclude=etc/resolv.conf")
                // Standard system mount points and ephemeral data
                .arg("--exclude=boot")
                .arg("--exclude=dev")
                .arg("--exclude=proc")
                .arg("--exclude=sys")
                .arg("--exclude=run")
                .arg("--exclude=var/lib/dpkg")
                .arg("--exclude=var/lib/apt")
                .status()
                .await
                .context("Failed to execute tar command")?;

            if !status.success() {
                anyhow::bail!("Tar command failed with status {}", status);
            }

            // 6. Ensure critical system directories and permissions (Safeguard)
            info!("Applying system permission safeguards...");
            let critical_paths = [
                ("/root", 0o700),
                ("/root/.ssh", 0o700),
                ("/home/mikrom", 0o755),
                ("/home/mikrom/.ssh", 0o700),
            ];

            for (path, mode) in critical_paths {
                let full_path = mount_dir.join(path.trim_start_matches('/'));
                if tokio::fs::metadata(&full_path).await.is_ok() {
                    tokio::fs::set_permissions(&full_path, fs::Permissions::from_mode(mode))
                        .await
                        .context(format!("Failed to set permissions for {}", path))?;
                }
            }

            // Fix authorized_keys permissions if they exist
            let auth_keys_paths = ["root/.ssh/authorized_keys", "home/mikrom/.ssh/authorized_keys"];
            for path in auth_keys_paths {
                let full_path = mount_dir.join(path);
                if tokio::fs::metadata(&full_path).await.is_ok() {
                    tokio::fs::set_permissions(&full_path, fs::Permissions::from_mode(0o600))
                        .await
                        .context(format!("Failed to set permissions for {}", path))?;
                }
            }

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
            env_map.insert("PORT".to_string(), port.to_string());
            if let Some(addr) = ipv6_addr {
                env_map.insert("IPV6_ADDR".to_string(), addr);
            }
            if let Some(gw) = ipv6_gw {
                env_map.insert("IPV6_GW".to_string(), gw);
            }

            let init_config = serde_json::json!({
                "env": env_map,
                "workdir": workdir,
                "entrypoint": entrypoint_list,
                "cmd": cmd_list
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
        let _ = tokio::fs::remove_file(&export_tar_path).await;

        result
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

    #[test]
    fn test_builder_new() {
        let _ = ImageBuilder::new();
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
            )
            .await;
        assert!(result.is_err());
    }
}
