use crate::hypervisor::HypervisorError;
use crate::qemu::manager::QemuManager;
use mikrom_proto::id::VmId;

impl QemuManager {
    /// Query CPU information for a VM.
    pub async fn query_cpus(
        &self,
        vm_id: &VmId,
    ) -> Result<Vec<(usize, String, bool)>, HypervisorError> {
        with_qmp!(self, vm_id, "CPU query", |qmp| {
            let resp = qmp
                .execute("query-cpus")
                .await
                .map_err(|e| HypervisorError::ProcessError(format!("query-cpus failed: {e}")))?;
            let mut cpus = Vec::new();
            if let Some(serde_json::Value::Array(arr)) = resp.get("return") {
                for entry in arr {
                    let index = entry.get("CPU").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let arch = entry
                        .get("arch")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let pending = entry
                        .get("pending")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    cpus.push((index, arch, pending));
                }
            }
            Ok(cpus)
        })
    }

    /// Query block device statistics for a VM.
    pub async fn query_blockstats(
        &self,
        vm_id: &VmId,
    ) -> Result<Vec<(String, u64, u64)>, HypervisorError> {
        with_qmp!(self, vm_id, "blockstats query", |qmp| {
            let resp = qmp.execute("query-blockstats").await.map_err(|e| {
                HypervisorError::ProcessError(format!("query-blockstats failed: {e}"))
            })?;
            let mut stats = Vec::new();
            if let Some(serde_json::Value::Array(arr)) = resp.get("return") {
                for entry in arr {
                    let device = entry
                        .get("device")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let read_bytes = entry
                        .get("stats")
                        .and_then(|s| s.get("rd_bytes"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let write_bytes = entry
                        .get("stats")
                        .and_then(|s| s.get("wr_bytes"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    stats.push((device, read_bytes, write_bytes));
                }
            }
            Ok(stats)
        })
    }
}
