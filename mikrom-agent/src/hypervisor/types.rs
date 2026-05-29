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
