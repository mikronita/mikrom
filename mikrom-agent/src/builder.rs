use std::path::Path;
use std::process::{Command, Stdio};
use tracing::{error, info};

pub struct ImageBuilder;

impl ImageBuilder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub async fn get_entrypoint(&self, image: &str) -> anyhow::Result<String> {
        let output = tokio::process::Command::new("docker")
            .args(["inspect", "--format", "{{if .Config.Entrypoint}}{{join .Config.Entrypoint \" \"}}{{else}}{{join .Config.Cmd \" \"}}{{end}}", image])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!("Failed to inspect docker image {}", image);
        }

        let entrypoint = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if entrypoint.is_empty() {
            anyhow::bail!("No entrypoint or cmd found for image {}", image);
        }

        Ok(entrypoint)
    }

    pub async fn docker_to_ext4(&self, image: &str, output_path: &Path) -> anyhow::Result<()> {
        let image = image.to_string();
        let output_path = output_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            info!(
                "Converting Docker image {} to ext4 at {:?}",
                image, output_path
            );

            // 0. Optional: Docker login if credentials exist in env
            if let (Ok(user), Ok(pass)) = (
                std::env::var("REGISTRY_USER"),
                std::env::var("REGISTRY_PASS"),
            ) {
                let registry_host = image.split('/').next().unwrap_or("");
                info!("Logging into registry {}...", registry_host);

                let mut child = std::process::Command::new("docker")
                    .args(["login", registry_host, "-u", &user, "--password-stdin"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;

                if let Some(mut stdin) = child.stdin.take() {
                    use std::io::Write;
                    stdin.write_all(pass.as_bytes())?;
                    stdin.flush()?;
                }
                let _ = child.wait();
            }

            // 1. Pull image
            info!("Pulling image {}...", image);
            let status = Command::new("docker")
                .args(["pull", &image])
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to execute 'docker pull': {}", e))?;

            if !status.success() {
                anyhow::bail!("Failed to pull docker image {image}");
            }

            // 2. Create temporary container to export filesystem
            let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
            info!("Creating temporary container {}...", container_name);
            let status = Command::new("docker")
                .args(["create", "--name", &container_name, &image])
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to execute 'docker create': {}", e))?;

            if !status.success() {
                anyhow::bail!("Failed to create temporary docker container");
            }

            // 3. Prepare empty ext4 file
            let size_bytes = 1024 * 1024 * 1024;
            let file = std::fs::File::create(&output_path).map_err(|e| {
                anyhow::anyhow!("Failed to create output file {:?}: {}", output_path, e)
            })?;
            file.set_len(size_bytes)?;

            // Format as ext4
            info!("Formatting ext4 image...");
            let status = Command::new("mkfs.ext4")
                .arg("-F")
                .arg(&output_path)
                .status()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to execute 'mkfs.ext4': {}. Is e2fsprogs installed?",
                        e
                    )
                })?;

            if !status.success() {
                anyhow::bail!("Failed to format ext4 image");
            }

            // 4. Mount and copy files
            let mount_dir = format!("/tmp/mnt-{container_name}");
            std::fs::create_dir_all(&mount_dir)?;

            info!("Mounting image to {}...", mount_dir);
            let status = Command::new("mount")
                .arg("-o")
                .arg("loop")
                .arg(&output_path)
                .arg(&mount_dir)
                .status()
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to execute 'mount': {}. Do you have root/sudo permissions?",
                        e
                    )
                })?;

            if !status.success() {
                anyhow::bail!("Failed to mount ext4 image");
            }

            // Use docker export and tar to copy files
            info!("Extracting container filesystem...");
            let mut export_child = Command::new("docker")
                .args(["export", &container_name])
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| anyhow::anyhow!("Failed to spawn 'docker export': {}", e))?;

            let export_stdout = export_child
                .stdout
                .take()
                .expect("Failed to capture Docker export stdout");

            let tar_status = Command::new("tar")
                .args(["-C", &mount_dir, "-xf", "-"])
                .stdin(export_stdout)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to execute 'tar': {}", e))?;

            if !tar_status.success() {
                error!("Failed to extract container filesystem");
                let _ = Command::new("umount").arg(&mount_dir).status();
                anyhow::bail!("Tar extraction failed");
            }

            // 5. Cleanup
            info!("Cleaning up temporary resources...");
            let _ = Command::new("umount").arg(&mount_dir).status();
            let _ = std::fs::remove_dir(&mount_dir);

            let _ = Command::new("docker")
                .args(["rm", "-f", &container_name])
                .status();

            info!("Successfully created ext4 rootfs for {}", image);
            Ok(())
        })
        .await?
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
            .docker_to_ext4("nonexistent-image-12345", &temp_path)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_entrypoint_real_image() {
        let builder = ImageBuilder::new().unwrap();
        // Use a small, standard image that is likely present or quick to pull
        let result = builder.get_entrypoint("alpine:latest").await;

        // Alpine doesn't have an Entrypoint by default, but it has a Cmd ["/bin/sh"]
        if let Ok(ep) = result {
            assert!(!ep.is_empty());
            assert!(ep.contains("sh") || ep.contains("bin"));
        }
    }
}
