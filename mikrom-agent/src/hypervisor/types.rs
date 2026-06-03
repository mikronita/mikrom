use mikrom_proto::id::{AppId, VmId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VmStatus {
    Starting = 1,
    Running = 2,
    Stopping = 3,
    #[default]
    Stopped = 4,
    Failed = 5,
    Paused = 6,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmInfo {
    pub vm_id: VmId,
    pub app_id: AppId,
    pub image: String,
    pub config: VmConfig,
    pub status: VmStatus,
    pub started_at: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Volume {
    pub volume_id: String,
    pub size_mib: u64,
    pub read_only: bool,
    pub pool_name: String,
    pub mount_point: String,
    pub access_mode: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VmConfig {
    pub vcpus: u32,
    pub memory_mib: u64,
    pub disk_mib: u64,
    pub port: u32,
    pub env: std::collections::HashMap<String, String>,
    pub ip_address: Option<String>,
    pub gateway: Option<String>,
    pub ipv6_address: Option<String>,
    pub ipv6_gateway: Option<String>,
    pub mac_address: Option<String>,
    pub netmask: Option<String>,
    pub volumes: Vec<Volume>,
    pub health_check_path: String,
    pub workload_type: i32,
}

pub(crate) const FIRECRACKER_BASE_KERNEL_BOOT_ARGS: &str = "console=ttyS0 reboot=k panic=1 pci=off nomodules rw root=/dev/vda init=/mikrom-init i8042.nokbd i8042.noaux quiet";
pub(crate) const CLOUD_HYPERVISOR_BASE_KERNEL_BOOT_ARGS: &str = "console=ttyS0 earlyprintk=ttyS0 reboot=k panic=1 net.ifnames=0 nomodules rw root=/dev/vda init=/mikrom-init";

/// Builder for kernel boot arguments with optional IPv6 network parameters.
pub struct KernelBootArgsBuilder<'a> {
    base_boot_args: &'a str,
    config: &'a VmConfig,
}

impl<'a> KernelBootArgsBuilder<'a> {
    fn with_base(base_boot_args: &'a str, config: &'a VmConfig) -> Self {
        Self {
            base_boot_args,
            config,
        }
    }

    pub fn firecracker(config: &'a VmConfig) -> Self {
        Self::with_base(FIRECRACKER_BASE_KERNEL_BOOT_ARGS, config)
    }

    pub fn cloud_hypervisor(config: &'a VmConfig) -> Self {
        Self::with_base(CLOUD_HYPERVISOR_BASE_KERNEL_BOOT_ARGS, config)
    }

    pub fn build(&self) -> String {
        let mut boot_args = self.base_boot_args.to_string();

        if let (Some(ipv6_str), Some(gw6_str)) =
            (&self.config.ipv6_address, &self.config.ipv6_gateway)
            && let (Ok(_ipv6), Ok(_gw6)) = (
                ipv6_str.parse::<std::net::Ipv6Addr>(),
                gw6_str.parse::<std::net::Ipv6Addr>(),
            )
        {
            boot_args.push_str(&format!(" ip=[{ipv6_str}]::[{gw6_str}]:64::eth0:off"));
        }

        boot_args
    }
}

#[derive(Clone, Debug)]
pub struct VmDetailedInfo {
    pub vm_id: VmId,
    pub app_id: AppId,
    pub status: VmStatus,
    pub error_message: Option<String>,
    pub pid: Option<u32>,
    pub metrics_path: Option<String>,
    pub socket_path: Option<String>,
    pub tap_name: Option<String>,
    pub tap_ifindex: Option<u32>,
    pub raw_metrics: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_boot_args_builder_skips_missing_values() {
        let config = VmConfig::default();
        let boot_args = KernelBootArgsBuilder::with_base("base-args", &config).build();
        assert_eq!(boot_args, "base-args");
    }

    #[test]
    fn kernel_boot_args_builder_appends_valid_ipv6_pair() {
        let config = VmConfig {
            ipv6_address: Some("fd40:b90d:fc5f:1ae0::2".to_string()),
            ipv6_gateway: Some("fd40:b90d:fc5f:1ae0::1".to_string()),
            ..Default::default()
        };

        let boot_args = KernelBootArgsBuilder::with_base("base-args", &config).build();
        assert_eq!(
            boot_args,
            "base-args ip=[fd40:b90d:fc5f:1ae0::2]::[fd40:b90d:fc5f:1ae0::1]:64::eth0:off"
        );
    }

    #[test]
    fn firecracker_and_cloud_hypervisor_builders_use_expected_bases() {
        let config = VmConfig::default();

        assert_eq!(
            KernelBootArgsBuilder::firecracker(&config).build(),
            FIRECRACKER_BASE_KERNEL_BOOT_ARGS
        );
        assert_eq!(
            KernelBootArgsBuilder::cloud_hypervisor(&config).build(),
            CLOUD_HYPERVISOR_BASE_KERNEL_BOOT_ARGS
        );
    }
}
