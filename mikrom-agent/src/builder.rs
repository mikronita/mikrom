use std::path::Path;
use std::process::{Command, Stdio};
use tracing::{error, info};

pub struct ImageBuilder;

impl ImageBuilder {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub async fn docker_to_ext4(&self, image: &str, output_path: &Path) -> anyhow::Result<()> {
        let image = image.to_string();
        let output_path = output_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            info!(
                "Converting Docker image {} to ext4 at {:?}",
                image, output_path
            );

            // 1. Pull image
            let status = Command::new("docker").args(["pull", &image]).status()?;
            if !status.success() {
                anyhow::bail!("Failed to pull docker image {}", image);
            }

            // 2. Create temporary container to export filesystem
            let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
            let status = Command::new("docker")
                .args(["create", "--name", &container_name, &image])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to create temporary docker container");
            }

            // 3. Prepare empty ext4 file
            let size_bytes = 1024 * 1024 * 1024;
            let file = std::fs::File::create(&output_path)?;
            file.set_len(size_bytes)?;

            // Format as ext4
            let status = Command::new("mkfs.ext4")
                .arg("-F")
                .arg(&output_path)
                .status()?;

            if !status.success() {
                anyhow::bail!("Failed to format ext4 image");
            }

            // 4. Mount and copy files
            let mount_dir = format!("/tmp/mnt-{}", container_name);
            std::fs::create_dir_all(&mount_dir)?;

            let status = Command::new("mount")
                .arg("-o")
                .arg("loop")
                .arg(&output_path)
                .arg(&mount_dir)
                .status()?;

            if !status.success() {
                anyhow::bail!("Failed to mount ext4 image");
            }

            // Use docker export and tar to copy files
            // docker export <container> | tar -C <mount_dir> -xf -
            let mut export_child = Command::new("docker")
                .args(["export", &container_name])
                .stdout(Stdio::piped())
                .spawn()?;

            let export_stdout = export_child.stdout.take().unwrap();

            let tar_status = Command::new("tar")
                .args(["-C", &mount_dir, "-xf", "-"])
                .stdin(export_stdout)
                .status()?;

            if !tar_status.success() {
                error!("Failed to extract container filesystem");
                let _ = Command::new("umount").arg(&mount_dir).status();
                anyhow::bail!("Tar extraction failed");
            }

            // 5. Cleanup
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
}
