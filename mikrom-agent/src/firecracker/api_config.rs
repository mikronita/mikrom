use crate::firecracker::api::fc_put;
use crate::firecracker::guard::VmStartupGuard;
use crate::hypervisor::{HypervisorError, VmConfig};
use std::time::Duration;

impl crate::firecracker::FirecrackerManager {
    /// Firecracker JSON PUT helper — serialises `payload` and forwards to
    /// the Firecracker Unix-socket API.
    async fn fc_put_json(
        &self,
        socket: &str,
        path: &str,
        payload: serde_json::Value,
    ) -> Result<(), HypervisorError> {
        fc_put(socket, path, &payload.to_string()).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn configure_vm_api(
        &self,
        config: &VmConfig,
        kernel_path: &str,
        rootfs_path: &std::path::Path,
        chroot_dir: &Option<String>,
        active_socket_path: &str,
        tap_name: Option<&str>,
        guard: &mut VmStartupGuard,
    ) -> Result<(), HypervisorError> {
        let socket = active_socket_path;

        self.apply_machine_config(socket, config).await?;
        self.apply_boot_source(socket, config, kernel_path, chroot_dir)
            .await?;
        self.apply_root_drive(socket, rootfs_path, chroot_dir)
            .await?;
        self.apply_network_interface(socket, config, tap_name)
            .await?;
        self.apply_additional_volumes(socket, config, chroot_dir, guard)
            .await?;

        self.start_instance(socket).await?;
        self.add_ipv6_host_route(config).await;
        Ok(())
    }

    pub(crate) async fn apply_machine_config(
        &self,
        socket: &str,
        config: &VmConfig,
    ) -> Result<(), HypervisorError> {
        self.fc_put_json(
            socket,
            "/machine-config",
            serde_json::json!({
                "vcpu_count": config.vcpus,
                "mem_size_mib": config.memory_mib,
                "smt": false,
                "track_dirty_pages": false
            }),
        )
        .await
    }

    pub(crate) fn build_boot_args(&self, config: &VmConfig) -> String {
        let mut boot_args =
            "console=ttyS0 reboot=k panic=1 pci=off nomodules rw root=/dev/vda init=/mikrom-init i8042.nokbd i8042.noaux quiet"
                .to_string();
        if let (Some(ip_str), Some(gw_str)) = (&config.ip_address, &config.gateway) {
            if let (Ok(_ip), Ok(_gw)) = (
                ip_str.parse::<std::net::Ipv4Addr>(),
                gw_str.parse::<std::net::Ipv4Addr>(),
            ) {
                let mask = config.netmask.as_deref().unwrap_or("255.255.255.0");
                if mask.parse::<std::net::Ipv4Addr>().is_ok() {
                    boot_args.push_str(&format!(" ip={ip_str}::{gw_str}:{mask}::eth0:off"));
                }
            }
        }

        if let (Some(ipv6_str), Some(gw6_str)) = (&config.ipv6_address, &config.ipv6_gateway) {
            if let (Ok(_ipv6), Ok(_gw6)) = (
                ipv6_str.parse::<std::net::Ipv6Addr>(),
                gw6_str.parse::<std::net::Ipv6Addr>(),
            ) {
                boot_args.push_str(&format!(" ip=[{ipv6_str}]::[{gw6_str}]:64::eth0:off"));
            }
        }

        boot_args
    }

    pub(crate) async fn apply_boot_source(
        &self,
        socket: &str,
        config: &VmConfig,
        kernel_path: &str,
        chroot_dir: &Option<String>,
    ) -> Result<(), HypervisorError> {
        let kernel_api_path = if chroot_dir.is_some() {
            "/vmlinux.bin".to_string()
        } else {
            kernel_path.to_string()
        };
        self.fc_put_json(
            socket,
            "/boot-source",
            serde_json::json!({
                "kernel_image_path": kernel_api_path,
                "boot_args": self.build_boot_args(config)
            }),
        )
        .await
    }

    pub(crate) async fn apply_root_drive(
        &self,
        socket: &str,
        rootfs_path: &std::path::Path,
        chroot_dir: &Option<String>,
    ) -> Result<(), HypervisorError> {
        let rootfs_api_path = if chroot_dir.is_some() {
            "/rootfs.ext4".to_string()
        } else {
            rootfs_path.to_string_lossy().to_string()
        };
        self.fc_put_json(
            socket,
            "/drives/rootfs",
            serde_json::json!({
                "drive_id": "rootfs",
                "path_on_host": rootfs_api_path,
                "is_root_device": true,
                "is_read_only": false
            }),
        )
        .await
    }

    pub(crate) async fn apply_network_interface(
        &self,
        socket: &str,
        config: &VmConfig,
        tap_name: Option<&str>,
    ) -> Result<(), HypervisorError> {
        if let Some(tap) = tap_name {
            self.fc_put_json(
                socket,
                "/network-interfaces/eth0",
                serde_json::json!({
                    "iface_id": "eth0",
                    "guest_mac": config.mac_address.as_deref().unwrap_or("AA:BB:CC:DD:EE:01"),
                    "host_dev_name": tap
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn apply_additional_volumes(
        &self,
        socket: &str,
        config: &VmConfig,
        chroot_dir: &Option<String>,
        guard: &mut VmStartupGuard,
    ) -> Result<(), HypervisorError> {
        for vol in &config.volumes {
            let vol_host_path = self.ensure_volume(vol).await?;

            use mikrom_proto::agent::AccessMode;
            if vol.access_mode == AccessMode::ReadWriteMany as i32 {
                let vfs_tag = vol.volume_id.replace('-', "_");
                let paths = crate::firecracker::paths::VmPaths::new(
                    &self.fc_config.data_dir,
                    &self.agent_id,
                    guard.vm_id,
                );
                let socket_path = paths.vfs_socket_path(&vol.volume_id);

                let vfs_child = self
                    .start_virtiofsd(&guard.vm_id, &vol.volume_id, &vol_host_path, &socket_path)
                    .await?;
                if let Some(pid) = vfs_child.id() {
                    guard.vfs_pids.push(pid);
                }
                guard.vfs_processes.push(vfs_child);

                let vfs_socket_api_path = if let Some(chroot) = chroot_dir {
                    let filename = format!("vfs_{}.socket", vol.volume_id);
                    let c_path = format!("{chroot}/root/{filename}");
                    self.mknod_at(&socket_path.to_string_lossy(), &c_path)
                        .await?;
                    format!("/{filename}")
                } else {
                    socket_path.to_string_lossy().to_string()
                };

                self.fc_put_json(
                    socket,
                    &format!("/vfs/{}", vfs_tag),
                    serde_json::json!({
                        "vfs_id": vfs_tag,
                        "socket_path": vfs_socket_api_path
                    }),
                )
                .await?;
            } else {
                let vol_api_path = self
                    .volume_api_path(vol, &vol_host_path, chroot_dir)
                    .await?;
                let drive_id = vol.volume_id.replace('-', "_");
                self.fc_put_json(
                    socket,
                    &format!("/drives/{}", drive_id),
                    serde_json::json!({
                        "drive_id": drive_id,
                        "path_on_host": vol_api_path,
                        "is_root_device": false,
                        "is_read_only": vol.read_only
                    }),
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn start_instance(&self, socket: &str) -> Result<(), HypervisorError> {
        tokio::time::sleep(Duration::from_millis(15)).await;
        self.fc_put_json(
            socket,
            "/actions",
            serde_json::json!({ "action_type": "InstanceStart" }),
        )
        .await
    }
}
