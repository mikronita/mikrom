use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CpusConfig {
    pub boot_vcpus: u32,
    pub max_vcpus: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    pub size: u64, // bytes
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PayloadConfig {
    pub kernel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmdline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initramfs: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DiskConfig {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_type: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct NetConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub tap: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_queues: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_tso: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_ufo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload_csum: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct FsConfig {
    pub tag: String,
    pub socket: String,
    pub num_queues: u32,
    pub queue_size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RngConfig {
    pub src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iommu: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VmConfig {
    pub cpus: CpusConfig,
    pub memory: MemoryConfig,
    pub payload: PayloadConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disks: Option<Vec<DiskConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net: Option<Vec<NetConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs: Option<Vec<FsConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rng: Option<RngConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<SerialConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<ConsoleConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SerialConfig {
    pub mode: String, // "Off", "Null", "File", "Tty", "Socket"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConsoleConfig {
    pub mode: String, // "Off", "Null", "File", "Tty", "Socket"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmInfoResponse {
    pub config: VmConfig,
    pub state: String, // "Created", "Running", "Paused", "Shutdown"
    pub memory_actual_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_config_serialization() {
        let config = VmConfig {
            cpus: CpusConfig {
                boot_vcpus: 2,
                max_vcpus: 4,
            },
            memory: MemoryConfig {
                size: 1024 * 1024 * 1024,
            },
            payload: PayloadConfig {
                kernel: "/path/to/kernel".to_string(),
                cmdline: Some("console=ttyS0".to_string()),
                ..Default::default()
            },
            disks: Some(vec![DiskConfig {
                path: "/path/to/rootfs".to_string(),
                readonly: Some(false),
                image_type: Some("Raw".to_string()),
            }]),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"boot_vcpus\":2"));
        assert!(json.contains("\"size\":1073741824"));
        assert!(json.contains("\"kernel\":\"/path/to/kernel\""));
        assert!(json.contains("\"disks\":["));
        // Check that None fields are skipped
        assert!(!json.contains("\"net\""));
        assert!(!json.contains("\"fs\""));
    }

    #[test]
    fn test_vm_info_response_deserialization() {
        let data = r#"{
            "config": {
                "cpus": {"boot_vcpus": 1, "max_vcpus": 1},
                "memory": {"size": 536870912},
                "payload": {"kernel": "/opt/ch/vmlinux.bin"}
            },
            "state": "Running",
            "memory_actual_size": 536870912
        }"#;

        let resp: VmInfoResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.state, "Running");
        assert_eq!(resp.config.cpus.boot_vcpus, 1);
        assert_eq!(resp.config.memory.size, 536870912);
    }
}
