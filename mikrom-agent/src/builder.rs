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
        let final_cmd = full_command_parts.join(" ");

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
        let mount_dir = format!("/tmp/mnt-{container_name}");
        tokio::fs::create_dir_all(&mount_dir).await?;

        info!("Mounting image to {}...", mount_dir);
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

        let mut tar_child = Command::new("tar")
            .args(["-C", &mount_dir, "-xf", "-"])
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
        init_content.push_str(
            "ip link set lo up 2>/dev/null || ifconfig lo 127.0.0.1 up 2>/dev/null || true\n",
        );

        for env in env_vars {
            if let Some((key, val)) = env.split_once('=') {
                init_content.push_str(&format!(
                    "export {}={}\n",
                    key,
                    try_quote(val).unwrap_or_else(|_| val.into())
                ));
            }
        }
        init_content.push_str(&format!("export PORT={}\n", port));

        init_content.push_str("echo '[mikrom] Starting application...'\n");
        init_content.push_str("exec /bin/sh /app-run.sh\n");

        tokio::fs::write(&init_script_path, init_content).await?;
        let _ = Command::new("chmod")
            .args(["+x", &init_script_path])
            .status()
            .await;

        let runner_path = format!("{}/app-run.sh", mount_dir);
        let runner_content = format!(
            "#!/bin/sh\ncd {} || cd /\n{}\n",
            try_quote(&workdir).unwrap_or_else(|_| "/app".into()),
            final_cmd
        );
        tokio::fs::write(&runner_path, runner_content).await?;
        let _ = Command::new("chmod")
            .args(["+x", &runner_path])
            .status()
            .await;

        // 7. Cleanup
        info!("Flushing and cleaning up...");
        let _ = Command::new("sync").status().await;
        let _ = Command::new("umount").arg(&mount_dir).status().await;
        let _ = tokio::fs::remove_dir(&mount_dir).await;
        let _ = Command::new("docker")
            .args(["rm", "-f", &container_name])
            .status()
            .await;

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
            .docker_to_ext4("nonexistent-image-12345", &temp_path, 8080)
            .await;
        assert!(result.is_err());
    }
}
