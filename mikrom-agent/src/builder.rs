use anyhow::Context;
use shlex::try_quote;
use std::path::Path;
use std::process::Stdio;
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

        // 0. Optional: Docker login
        if let (Ok(user), Ok(pass)) = (
            std::env::var("REGISTRY_USER"),
            std::env::var("REGISTRY_PASS"),
        ) {
            let registry_host = image.split('/').next().unwrap_or("");
            info!("Logging into registry {}...", registry_host);

            let mut child = Command::new("docker")
                .args(["login", registry_host, "-u", &user, "--password-stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(pass.as_bytes()).await?;
                stdin.flush().await?;
            }
            let _ = child.wait().await;
        }

        // 1. Pull image
        let status = Command::new("docker")
            .args(["pull", image])
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("Failed to pull docker image {image}");
        }

        // 2. Inspect metadata (Extract Entrypoint and Cmd as raw JSON to preserve quoting)
        let output = Command::new("docker")
            .args([
                "inspect",
                "--format",
                "{{range .Config.Env}}{{.}}||{{end}}###{{json .Config.Entrypoint}}###{{json .Config.Cmd}}###{{.Config.WorkingDir}}",
                image,
            ])
            .output()
            .await?;

        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let parts: Vec<&str> = raw.split("###").collect();
        let env_vars: Vec<String> = parts
            .first()
            .unwrap_or(&"")
            .split("||")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let entrypoint_json = parts.get(1).map(|s| s.trim()).unwrap_or("[]");
        let cmd_json = parts.get(2).map(|s| s.trim()).unwrap_or("[]");
        let workdir = parts
            .get(3)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/app".to_string());

        let entrypoint_list: Vec<String> =
            serde_json::from_str(entrypoint_json).unwrap_or_default();
        let cmd_list: Vec<String> = serde_json::from_str(cmd_json).unwrap_or_default();

        let mut full_command_parts = Vec::new();
        for part in entrypoint_list.iter().chain(cmd_list.iter()) {
            full_command_parts.push(try_quote(part).unwrap_or_else(|_| part.into()).into_owned());
        }
        let _final_cmd = full_command_parts.join(" ");

        // 3. Create temporary container to export filesystem
        let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
        let status = Command::new("docker")
            .args(["create", "--name", &container_name, image])
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("Failed to create temporary docker container");
        }

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
        let parent_dir = output_path.parent().unwrap_or(Path::new("/tmp"));
        let mount_dir = parent_dir.join(format!("mnt-{container_name}"));
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

        // Use docker export and tar to copy files
        let mut export_child = Command::new("docker")
            .args(["export", &container_name])
            .stdout(Stdio::piped())
            .spawn()?;

        let mount_dir_str = mount_dir.to_string_lossy();
        let mut tar_child = Command::new("tar")
            .args(["-C", &mount_dir_str, "-xf", "-"])
            .stdin(Stdio::piped())
            .spawn()?;

        let mut stdout = export_child
            .stdout
            .take()
            .expect("Failed to capture Docker export stdout");
        let mut stdin = tar_child.stdin.take().expect("Failed to capture Tar stdin");

        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stdout, &mut stdin).await;
        });

        let (export_status, tar_status) = tokio::try_join!(export_child.wait(), tar_child.wait())?;

        if !export_status.success() || !tar_status.success() {
            let _ = Command::new("umount").arg(&mount_dir).status().await;
            anyhow::bail!("Tar extraction or Docker export failed");
        }

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

        let _ = Command::new("chmod")
            .args(["+x", &dest_init_path.to_string_lossy()])
            .status()
            .await;

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

        // 7. Cleanup
        info!("Flushing and cleaning up...");
        let _ = Command::new("sync").status().await;

        let status = Command::new("umount").arg(&mount_dir).status().await?;
        if !status.success() {
            tracing::error!(
                "Failed to unmount {}; filesystem may be corrupted",
                mount_dir_str
            );
            // Try lazy unmount as last resort
            let _ = Command::new("umount")
                .arg("-l")
                .arg(&mount_dir)
                .status()
                .await;
        }

        if let Err(e) = tokio::fs::remove_dir(&mount_dir).await {
            tracing::warn!("Failed to remove mount directory {:?}: {}", mount_dir, e);
        }
        let status = Command::new("docker")
            .args(["rm", "-f", &container_name])
            .status()
            .await;
        if let Ok(s) = status {
            if !s.success() {
                tracing::warn!(
                    "Failed to remove temporary docker container {}",
                    container_name
                );
            }
        } else if let Err(e) = status {
            tracing::warn!(
                "Error removing temporary docker container {}: {}",
                container_name,
                e
            );
        }

        info!("Successfully created ext4 rootfs for {}", image);
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
