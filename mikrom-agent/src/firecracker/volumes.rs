use crate::ceph::{CephFs, CephRbd, StorageProvider};
use crate::hypervisor::{HypervisorError, Volume};
use mikrom_proto::id::VmId;

impl crate::firecracker::FirecrackerManager {
    pub(crate) async fn ensure_volume(&self, vol: &Volume) -> Result<String, HypervisorError> {
        if !vol.pool_name.is_empty() {
            use mikrom_proto::agent::AccessMode;
            if vol.access_mode == AccessMode::ReadWriteMany as i32 {
                let safe_id = sanitize_filename(&vol.volume_id);
                let mount_point = format!("{}/cephfs/{safe_id}", self.fc_config.data_dir);
                CephFs::mount_volume(&vol.volume_id, &mount_point)
                    .await
                    .map_err(|e| {
                        HypervisorError::ProcessError(format!("Failed to mount CephFS volume: {e}"))
                    })?;

                Ok(mount_point)
            } else {
                self.ensure_rbd_volume(vol).await
            }
        } else {
            self.ensure_local_volume(vol).await
        }
    }

    pub(crate) async fn ensure_rbd_volume(&self, vol: &Volume) -> Result<String, HypervisorError> {
        let storage = CephRbd;
        if !storage.exists(&vol.pool_name, &vol.volume_id).await {
            storage
                .create_volume(&vol.pool_name, &vol.volume_id, vol.size_mib.min(i32::MAX as u64) as i32)
                .await
                .map_err(|e| {
                    HypervisorError::ProcessError(format!("Failed to create RBD volume: {e}"))
                })?;
        }

        let dev_path = storage
            .map_volume(&vol.pool_name, &vol.volume_id)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to map RBD volume: {e}")))?;

        let is_formatted = {
            use std::io::{Read, Seek, SeekFrom};
            let mut file = std::fs::File::open(&dev_path).map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to open device {dev_path}: {e}"))
            })?;

            let mut buffer = [0u8; 2];
            file.seek(SeekFrom::Start(1080)).map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to seek in device {dev_path}: {e}"))
            })?;

            match file.read_exact(&mut buffer) {
                Ok(_) => buffer[0] == 0x53 && buffer[1] == 0xEF,
                Err(_) => false,
            }
        };

        if !is_formatted && !vol.read_only {
            tracing::info!(volume_id = %vol.volume_id, device = %dev_path, "Formatting volume with ext4...");
            let output = tokio::process::Command::new("mkfs.ext4")
                .arg("-F")
                .arg(&dev_path)
                .output()
                .await
                .map_err(|e| {
                    HypervisorError::ProcessError(format!("Failed to execute mkfs.ext4: {e}"))
                })?;

            if !output.status.success() {
                return Err(HypervisorError::ProcessError(format!(
                    "mkfs.ext4 failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            tracing::info!(volume_id = %vol.volume_id, "Volume formatted successfully");
        }

        Ok(dev_path)
    }

    pub(crate) async fn ensure_local_volume(
        &self,
        vol: &Volume,
    ) -> Result<String, HypervisorError> {
        let vol_dir = format!("{}/volumes", self.fc_config.data_dir);
        tokio::fs::create_dir_all(&vol_dir).await.map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to create volumes dir: {e}"))
        })?;

        let vol_path = format!("{vol_dir}/{}.ext4", sanitize_filename(&vol.volume_id));
        if tokio::fs::metadata(&vol_path).await.is_err() {
            let file = tokio::fs::File::create(&vol_path).await.map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to create volume file: {e}"))
            })?;
            file.set_len(vol.size_mib * 1024 * 1024)
                .await
                .map_err(|e| {
                    HypervisorError::ProcessError(format!("Failed to set volume size: {e}"))
                })?;
        }
        Ok(vol_path)
    }

    pub(crate) async fn volume_api_path(
        &self,
        vol: &Volume,
        vol_host_path: &str,
        chroot_dir: &Option<String>,
    ) -> Result<String, HypervisorError> {
        if let Some(chroot) = chroot_dir {
            let filename = format!("{}.ext4", sanitize_filename(&vol.volume_id));
            let c_path = format!("{chroot}/root/{filename}");

            if !vol.pool_name.is_empty() {
                self.mknod_at(vol_host_path, &c_path).await?;
            } else {
                self.ensure_file_at(vol_host_path, &c_path).await?;
            }

            self.recursive_chown(
                &c_path,
                self.fc_config.jailer_uid,
                self.fc_config.jailer_gid,
            )
            .await?;
            Ok(format!("/{filename}"))
        } else {
            Ok(vol_host_path.to_string())
        }
    }

    pub(crate) async fn start_virtiofsd(
        &self,
        vm_id: &VmId,
        vol_id: &str,
        shared_dir: &str,
        socket_path: &std::path::Path,
    ) -> Result<tokio::process::Child, HypervisorError> {
        let binary = &self.fc_config.virtiofsd_path;

        if tokio::fs::metadata(binary).await.is_err() {
            return Err(HypervisorError::ProcessError(format!(
                "virtiofsd binary not found at {binary}"
            )));
        }

        if let Some(parent) = socket_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::remove_file(socket_path).await;

        let mut cmd = tokio::process::Command::new(binary);
        cmd.arg("--socket-path").arg(socket_path);
        cmd.arg("--shared-dir").arg(shared_dir);
        cmd.arg("--sandbox").arg("none");

        tracing::info!(
            vm_id = %vm_id,
            vol_id = %vol_id,
            shared_dir = %shared_dir,
            "Spawning virtiofsd"
        );

        let child = cmd.spawn().map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to spawn virtiofsd: {e}"))
        })?;

        Ok(child)
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
