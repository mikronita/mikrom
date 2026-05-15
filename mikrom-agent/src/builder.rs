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
        port: u32,
        ipv6_addr: Option<String>,
        ipv6_gw: Option<String>,
    ) -> anyhow::Result<()> {
        info!(
            "Converting Docker image {} to ext4 at {:?} (port={})",
            image, output_path, port
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

            // 4. Prepare empty ext4 file (1GB)
            let size_bytes = 1024 * 1024 * 1024;
            let file = tokio::fs::File::create(&output_path).await?;
            file.set_len(size_bytes).await?;

            // Format as ext4
            info!("Formatting ext4 image...");
            let status = Command::new("mkfs.ext4")
                .arg("-F")
                .arg(output_path)
                .status()
                .await?;

            if !status.success() {
                anyhow::bail!("Failed to format ext4 image");
            }

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

            let mount_dir_for_unpack = mount_dir.clone();
            let export_tar_for_unpack = export_tar_path.clone();
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let file = std::fs::File::open(&export_tar_for_unpack)
                    .context("Failed to open exported container archive")?;
                let mut archive = tar::Archive::new(file);
                archive
                    .unpack(&mount_dir_for_unpack)
                    .context("Failed to unpack container archive")?;
                Ok(())
            })
            .await
            .context("Failed to unpack exported container archive")??;

            // 6. Setup Mikrom Init (Binario estático)
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
            .docker_to_ext4("nonexistent-image-12345", &temp_path, 8080, None, None)
            .await;
        assert!(result.is_err());
    }
}
