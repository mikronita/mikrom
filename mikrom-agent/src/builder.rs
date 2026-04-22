use std::path::Path;
use std::process::{Command, Stdio};
use tracing::info;

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

            // 0. Optional: Docker login
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
                    let _ = stdin.write_all(pass.as_bytes());
                    let _ = stdin.flush();
                }
                let _ = child.wait();
            }

            // 1. Pull image
            let status = Command::new("docker").args(["pull", &image]).status()?;
            if !status.success() {
                anyhow::bail!("Failed to pull docker image {image}");
            }

            // 2. Inspect metadata (Extract Entrypoint and Cmd as raw JSON to preserve quoting)
            let output = Command::new("docker")
                .args([
                    "inspect",
                    "--format",
                    "{{range .Config.Env}}{{.}}||{{end}}###{{json .Config.Entrypoint}}###{{json .Config.Cmd}}###{{.Config.WorkingDir}}",
                    &image,
                ])
                .output()?;

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
                .unwrap_or("/app".to_string());

            let entrypoint_list: Vec<String> =
                serde_json::from_str(entrypoint_json).unwrap_or_default();
            let cmd_list: Vec<String> = serde_json::from_str(cmd_json).unwrap_or_default();

            let mut full_command_parts = Vec::new();
            for part in entrypoint_list.iter().chain(cmd_list.iter()) {
                full_command_parts.push(format!("'{}'", part.replace("'", "'\\''")));
            }
            let final_cmd = full_command_parts.join(" ");

            // 3. Create temporary container to export filesystem
            let container_name = format!("mikrom-build-{}", uuid::Uuid::new_v4());
            let status = Command::new("docker")
                .args(["create", "--name", &container_name, &image])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to create temporary docker container");
            }

            // 4. Prepare empty ext4 file (1GB)
            let size_bytes = 1024 * 1024 * 1024;
            let file = std::fs::File::create(&output_path)?;
            file.set_len(size_bytes)?;

            // Format as ext4
            info!("Formatting ext4 image...");
            let status = Command::new("mkfs.ext4")
                .arg("-F")
                .arg(&output_path)
                .status()?;

            if !status.success() {
                anyhow::bail!("Failed to format ext4 image");
            }

            // 5. Mount and copy files
            let mount_dir = format!("/tmp/mnt-{container_name}");
            std::fs::create_dir_all(&mount_dir)?;

            info!("Mounting image to {}...", mount_dir);
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
            let mut export_child = Command::new("docker")
                .args(["export", &container_name])
                .stdout(Stdio::piped())
                .spawn()?;

            let export_stdout = export_child
                .stdout
                .take()
                .expect("Failed to capture Docker export stdout");

            let tar_status = Command::new("tar")
                .args(["-C", &mount_dir, "-xf", "-"])
                .stdin(export_stdout)
                .status()?;

            if !tar_status.success() {
                let _ = Command::new("umount").arg(&mount_dir).status();
                anyhow::bail!("Tar extraction failed");
            }

            // 6. Create mikrom-init.sh
            info!("Creating /mikrom-init.sh...");
            let init_script_path = format!("{}/mikrom-init.sh", mount_dir);
            let mut init_content = String::from("#!/bin/sh\n\n");

            init_content.push_str("mount -t proc proc /proc\n");
            init_content.push_str("mount -t sysfs sys /sys\n");
            init_content.push_str("mount -t devtmpfs devtmpfs /dev || true\n");
            init_content.push_str("mkdir -p /run /tmp /dev/pts /dev/shm\n");
            init_content.push_str("mount -t tmpfs tmpfs /run\n");
            init_content.push_str("mount -t tmpfs tmpfs /tmp\n");
            init_content.push_str("mount -t tmpfs tmpfs /dev/shm\n");
            init_content.push_str("mount -t devpts devpts /dev/pts 2>/dev/null || true\n");

            init_content.push_str("export TERM=linux\n");
            init_content.push_str("export COLUMNS=100\n");
            init_content.push_str("export LINES=24\n");

            init_content.push_str("hostname localhost\n");
            init_content
                .push_str("ip link set lo up 2>/dev/null || ifconfig lo 127.0.0.1 up 2>/dev/null || true\n");

            for env in env_vars {
                if let Some((key, val)) = env.split_once('=') {
                    init_content.push_str(&format!(
                        "export {}=\"{}\"\n",
                        key,
                        val.replace("\"", "\\\"")
                    ));
                }
            }
            init_content.push_str("export PORT=8080\n");

            init_content.push_str("echo '[mikrom] Starting application...'\n");
            init_content.push_str("exec /bin/sh /app-run.sh\n");

            std::fs::write(&init_script_path, init_content)?;
            let _ = Command::new("chmod").args(["+x", &init_script_path]).status();

            let runner_path = format!("{}/app-run.sh", mount_dir);
            let runner_content =
                format!("#!/bin/sh\ncd \"{}\" || cd /\n{}\n", workdir, final_cmd);
            std::fs::write(&runner_path, runner_content)?;
            let _ = Command::new("chmod").args(["+x", &runner_path]).status();

            // 7. Cleanup
            info!("Flushing and cleaning up...");
            let _ = Command::new("sync").status();
            let _ = Command::new("umount").arg(&mount_dir).status();
            let _ = std::fs::remove_dir(&mount_dir);
            let _ = Command::new("docker").args(["rm", "-f", &container_name]).status();

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
