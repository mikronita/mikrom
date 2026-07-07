use crate::cloud_hypervisor::api::{ch_request, wait_for_socket};
use crate::cloud_hypervisor::config as ch_config;
use crate::cloud_hypervisor::process::CloudHypervisorProcess;
use crate::config::AgentConfig;
use crate::hypervisor::KernelBootArgsBuilder;
use crate::hypervisor::vm_hypervisor::{HypervisorType, VmHypervisor};
use crate::hypervisor::{HypervisorError, VmConfig, VmDetailedInfo, VmInfo, VmStatus};
use async_trait::async_trait;
use dashmap::DashMap;
use mikrom_proto::id::{AppId, VmId};
use serde_json::json;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

pub struct CloudHypervisorManager {
    pub(crate) config: AgentConfig,
    active_vms: Arc<DashMap<VmId, Arc<RwLock<CloudHypervisorProcess>>>>,
    vms: Arc<DashMap<VmId, VmInfo>>,
    builder: Arc<crate::builder::ImageBuilder>,
    nats_client: Arc<RwLock<Option<async_nats::Client>>>,
}

const DATABASE_CONFIGURE_PORT: u32 = 3080;
const NEON_CONFIGURE_TOKEN_ENV: &str = "NEON_CONFIGURE_TOKEN";
const MIKROM_DATABASE_CONFIGURE_TOKEN_ENV: &str = "MIKROM_DATABASE_CONFIGURE_TOKEN";
const NEON_PAGESERVER_IPV6_ENV: &str = "NEON_PAGESERVER_IPV6";
const NEON_SAFEKEEPERS_GENERATION_ENV: &str = "NEON_SAFEKEEPERS_GENERATION";
const NEON_SAFEKEEPER_CONNSTRS_ENV: &str = "NEON_SAFEKEEPER_CONNSTRS";
const DEFAULT_NEON_PAGESERVER_IPV6: &str = "fd00::deed:1d1c";
const DATABASE_CONFIGURE_RETRY_WINDOW: Duration = Duration::from_secs(60);
const DATABASE_CONFIGURE_RETRY_WINDOW_ENV: &str = "MIKROM_DATABASE_CONFIGURE_WINDOW_SECS";

impl CloudHypervisorManager {
    pub async fn new(config: AgentConfig) -> Self {
        let builder = crate::builder::ImageBuilder::new().expect("ImageBuilder::new is infallible");

        Self {
            config,
            active_vms: Arc::new(DashMap::new()),
            vms: Arc::new(DashMap::new()),
            builder: Arc::new(builder),
            nats_client: Arc::new(RwLock::new(None)),
        }
    }

    fn ch_connect_timeout(&self) -> Duration {
        self.config.cloud_hypervisor_api_connect_timeout()
    }

    fn ch_status_timeout(&self) -> Duration {
        self.config.cloud_hypervisor_api_status_timeout()
    }

    fn ch_header_timeout(&self) -> Duration {
        self.config.cloud_hypervisor_api_header_timeout()
    }

    fn ch_body_timeout(&self) -> Duration {
        self.config.cloud_hypervisor_api_body_timeout()
    }

    fn build_boot_args(&self, config: &VmConfig) -> String {
        KernelBootArgsBuilder::cloud_hypervisor(config).build()
    }

    fn workload_artifacts(&self, workload_type: i32) -> (&Path, &Path) {
        if workload_type == mikrom_proto::scheduler::WorkloadType::Database as i32 {
            (
                self.config.cloud_hypervisor_database_kernel.as_path(),
                self.config.cloud_hypervisor_database_base_rootfs.as_path(),
            )
        } else {
            (
                self.config.cloud_hypervisor_kernel.as_path(),
                self.config.cloud_hypervisor_base_rootfs.as_path(),
            )
        }
    }

    fn build_database_configure_url(ipv6: &str) -> String {
        format!("http://[{ipv6}]:{DATABASE_CONFIGURE_PORT}/configure")
    }

    fn database_pageserver_ipv6<'a>(&self, config: &'a VmConfig) -> &'a str {
        config
            .env
            .get(NEON_PAGESERVER_IPV6_ENV)
            .map(String::as_str)
            .unwrap_or(DEFAULT_NEON_PAGESERVER_IPV6)
    }

    fn build_database_configure_spec(
        &self,
        vm_id: &VmId,
        config: &VmConfig,
    ) -> Result<serde_json::Value, HypervisorError> {
        let tenant_id = config.env.get("NEON_TENANT_ID").cloned().ok_or_else(|| {
            HypervisorError::ProcessError(
                "NEON_TENANT_ID is required for database workloads".to_string(),
            )
        })?;
        let timeline_id = config.env.get("NEON_TIMELINE_ID").cloned().ok_or_else(|| {
            HypervisorError::ProcessError(
                "NEON_TIMELINE_ID is required for database workloads".to_string(),
            )
        })?;
        let pageserver_ipv6 = self.database_pageserver_ipv6(config);
        let pageserver_host = Self::neon_host_alias("neon-pageserver", pageserver_ipv6);
        let safekeeper_connstrings = Self::normalize_neon_safekeeper_connstrings(
            config.env.get(NEON_SAFEKEEPER_CONNSTRS_ENV),
            "neon-safekeeper",
            &pageserver_host,
        );
        let safekeepers_generation = config
            .env
            .get(NEON_SAFEKEEPERS_GENERATION_ENV)
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(1);
        let cluster_id = config
            .env
            .get("MIKROM_DATABASE_ID")
            .cloned()
            .unwrap_or_else(|| vm_id.to_string());

        let spec = json!({
            "format_version": 1.0,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "operation_uuid": Uuid::new_v4().to_string(),
            "cluster": {
                "cluster_id": cluster_id,
                "name": null,
                "state": null,
                "roles": [],
                "databases": [],
                "postgresql_conf": "shared_preload_libraries='neon'",
                "settings": null,
            },
            "tenant_id": tenant_id,
            "timeline_id": timeline_id,
            "pageserver_connstring": format!("host={pageserver_host} port=6400"),
            "safekeeper_connstrings": safekeeper_connstrings,
            "safekeepers_generation": safekeepers_generation,
            "mode": "Primary",
            "skip_pg_catalog_updates": true,
            "reconfigure_concurrency": 1,
            "suspend_timeout_seconds": -1,
        });

        Ok(json!({
            "spec": spec,
            "compute_ctl_config": {
                "jwks": config
                    .env
                    .get("NEON_JWKS_JSON")
                    .map(|value| serde_json::from_str::<serde_json::Value>(value).unwrap_or_else(|_| json!({"keys": []})))
                    .unwrap_or_else(|| json!({"keys": []})),
            },
        }))
    }

    fn neon_host_alias(prefix: &str, value: &str) -> String {
        let sanitized: String = value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        format!("{prefix}-{sanitized}")
    }

    fn normalize_neon_safekeeper_connstrings(
        raw: Option<&String>,
        alias_prefix: &str,
        default_alias: &str,
    ) -> Vec<String> {
        let mut entries = Vec::new();

        if let Some(raw) = raw {
            for entry in raw
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
            {
                if let Some(normalized) =
                    Self::normalize_neon_safekeeper_connstr(entry, alias_prefix)
                {
                    entries.push(normalized);
                }
            }
        }

        if entries.is_empty() {
            entries.push(format!("{default_alias}:5454"));
        }

        entries
    }

    fn normalize_neon_safekeeper_connstr(value: &str, alias_prefix: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }

        if trimmed.contains('=') {
            return Some(trimmed.to_string());
        }

        let (host, port) = if let Some(host) = trimmed.strip_prefix('[') {
            let (host, rest) = host.split_once(']')?;
            let port = rest.strip_prefix(':')?.trim();
            (host, port)
        } else {
            if trimmed.chars().filter(|&c| c == ':').count() > 1 {
                return None;
            }
            let (host, port) = trimmed.rsplit_once(':')?;
            (host, port)
        };

        if host.is_empty() || port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }

        if host.contains(':') {
            let alias = Self::neon_host_alias(alias_prefix, host);
            return Some(format!("{alias}:{port}"));
        }

        Some(format!("{host}:{port}"))
    }

    fn database_configure_token<'a>(&self, config: &'a VmConfig) -> Option<&'a str> {
        config
            .env
            .get(NEON_CONFIGURE_TOKEN_ENV)
            .or_else(|| config.env.get(MIKROM_DATABASE_CONFIGURE_TOKEN_ENV))
            .map(String::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
    }

    #[allow(clippy::collapsible_if)]
    async fn configure_database_vm(
        &self,
        vm_id: &VmId,
        config: &VmConfig,
    ) -> Result<(), HypervisorError> {
        if config.workload_type != mikrom_proto::scheduler::WorkloadType::Database as i32 {
            return Ok(());
        }

        let ipv6 = config.ipv6_address.as_deref().ok_or_else(|| {
            HypervisorError::ProcessError("Database VM is missing IPv6 address".to_string())
        })?;
        let url = Self::build_database_configure_url(ipv6);
        let spec = self.build_database_configure_spec(vm_id, config)?;
        let client = reqwest::Client::builder()
            .timeout(self.config.cloud_hypervisor_configure_client_timeout())
            .build()
            .map_err(|e| {
                HypervisorError::ProcessError(format!("Failed to build configure client: {e}"))
            })?;

        let mut delay = Duration::from_millis(250);
        let deadline = tokio::time::Instant::now() + Self::database_configure_retry_window();
        let bearer_token = self.database_configure_token(config);

        let mut attempt = 1;
        while tokio::time::Instant::now() < deadline {
            let mut request = client.post(&url).json(&spec);
            if let Some(token) = bearer_token {
                request = request.bearer_auth(token);
            }

            match tokio::time::timeout(
                self.config.cloud_hypervisor_configure_request_timeout(),
                request.send(),
            )
            .await
            {
                Ok(Ok(resp)) if resp.status().is_success() => {
                    tracing::info!(
                        vm_id = %vm_id,
                        url = %url,
                        attempt,
                        "Configured Cloud Hypervisor database workload"
                    );
                    return Ok(());
                },
                Ok(Ok(resp)) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    tracing::warn!(
                        vm_id = %vm_id,
                        url = %url,
                        attempt,
                        status = %status,
                        body = %body,
                        "Database configure endpoint returned non-success status"
                    );
                    if Self::is_fatal_database_configure_status(status) {
                        return Err(HypervisorError::ProcessError(format!(
                            "Database configure failed with client error {status}: {body}"
                        )));
                    }
                },
                Ok(Err(e)) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        url = %url,
                        attempt,
                        error = %e,
                        "Database configure request failed"
                    );
                },
                Err(_) => {
                    tracing::warn!(
                        vm_id = %vm_id,
                        url = %url,
                        attempt,
                        "Database configure request timed out"
                    );
                },
            }

            if tokio::time::Instant::now() >= deadline {
                break;
            }

            if tokio::time::Instant::now() + delay > deadline {
                tokio::time::sleep_until(deadline).await;
            } else {
                tokio::time::sleep(delay).await;
            }

            delay = std::cmp::min(
                delay.saturating_mul(2),
                self.config.cloud_hypervisor_configure_backoff_max(),
            );
            attempt += 1;
        }

        Err(HypervisorError::ProcessError(format!(
            "Failed to configure database VM after {:?}: {url}",
            Self::database_configure_retry_window()
        )))
    }

    fn is_fatal_database_configure_status(status: reqwest::StatusCode) -> bool {
        if status == reqwest::StatusCode::REQUEST_TIMEOUT
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::PRECONDITION_FAILED
        {
            return false;
        }

        status.is_client_error()
    }

    fn database_configure_retry_window() -> Duration {
        let value = std::env::var(DATABASE_CONFIGURE_RETRY_WINDOW_ENV).ok();
        Self::parse_database_configure_retry_window(value.as_deref())
    }

    fn parse_database_configure_retry_window(value: Option<&str>) -> Duration {
        value
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs)
            .unwrap_or(DATABASE_CONFIGURE_RETRY_WINDOW)
    }

    fn get_vm_paths(&self, vm_id: &VmId) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let base = self.config.data_path.join("vms").join(vm_id.to_string());
        if let Err(e) = std::fs::create_dir_all(&base) {
            tracing::warn!("Failed to create VM directory {}: {e}", base.display());
        }
        (
            base.join("ch.sock"),
            base.join("ch.log"),
            base.join("ch.pid"),
            base.join("ch-serial.log"),
        )
    }

    async fn start_vm_background(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        let (socket_path, log_path, _pid_path, serial_log) = self.get_vm_paths(&vm_id);
        let socket_str = socket_path.to_string_lossy().to_string();
        let serial_log_str = serial_log.to_string_lossy().to_string();

        // 1. Networking: Setup TAP
        let (tap_name, tap_ifindex) = self.setup_tap(&vm_id).await?;
        self.setup_routing(config.ipv6_address.as_deref()).await?;

        // 2. Storage: Ensure volumes
        let mut disks = Vec::new();
        let mut fs_devices = Vec::new();
        let mut sidecars = Vec::new();

        let rootfs_path = self
            .config
            .data_path
            .join("vms")
            .join(vm_id.to_string())
            .join("rootfs.ext4");

        let (kernel_path, base_rootfs_path) = self.workload_artifacts(config.workload_type);
        tracing::info!(
            vm_id = %vm_id,
            image = %image,
            workload_type = config.workload_type,
            base_rootfs = %base_rootfs_path.display(),
            "Preparing rootfs for Cloud Hypervisor"
        );

        if config.workload_type == mikrom_proto::scheduler::WorkloadType::Database as i32 {
            self.builder
                .database_to_ext4(crate::builder::DatabaseRootfsParams {
                    output_path: &rootfs_path,
                    base_rootfs_path,
                    port: config.port,
                    ipv6_addr: config.ipv6_address.clone(),
                    ipv6_gw: config.ipv6_gateway.clone(),
                    env: &config.env,
                    volumes: &config.volumes,
                    workload_type: config.workload_type,
                })
                .await
        } else {
            self.builder
                .docker_to_ext4(crate::builder::DockerToExt4Params {
                    image: &image,
                    output_path: &rootfs_path,
                    base_rootfs_path: &base_rootfs_path.to_string_lossy(),
                    port: config.port,
                    ipv6_addr: config.ipv6_address.clone(),
                    ipv6_gw: config.ipv6_gateway.clone(),
                    volumes: &config.volumes,
                    workload_type: config.workload_type,
                })
                .await
        }
        .map_err(|e| HypervisorError::ProcessError(format!("Image builder failed: {e}")))?;

        disks.push(ch_config::DiskConfig {
            path: rootfs_path.to_string_lossy().to_string(),
            readonly: Some(false),
            image_type: Some("Raw".to_string()),
        });

        for vol in &config.volumes {
            let host_path = self.ensure_volume(vol).await?;
            use mikrom_proto::agent::AccessMode;
            if vol.access_mode == AccessMode::ReadWriteMany as i32 {
                let fs_socket = self
                    .config
                    .data_path
                    .join("vms")
                    .join(vm_id.to_string())
                    .join(format!("fs-{}.sock", vol.volume_id));
                let sidecar = self
                    .start_virtiofsd(&vm_id, &vol.volume_id, &host_path, &fs_socket)
                    .await?;
                sidecars.push(sidecar);

                fs_devices.push(ch_config::FsConfig {
                    tag: vol.volume_id.clone(),
                    socket: fs_socket.to_string_lossy().to_string(),
                    num_queues: 1,
                    queue_size: 1024,
                });
            } else {
                disks.push(ch_config::DiskConfig {
                    path: host_path,
                    readonly: Some(vol.read_only),
                    image_type: Some("Raw".to_string()),
                });
            }
        }

        // 3. Spawn CH process
        let mut proc = CloudHypervisorProcess::spawn(
            &self.config.cloud_hypervisor_binary,
            vm_id.to_string(),
            socket_path.clone(),
            log_path,
            Some(tap_ifindex),
        )
        .await?;
        proc.sidecars = sidecars;

        // 4. Wait for API socket
        wait_for_socket(
            &socket_str,
            self.config.cloud_hypervisor_socket_wait_timeout(),
        )
        .await?;

        // 5. Assemble CH VmConfig
        let mut ch_vm_config = ch_config::VmConfig {
            cpus: ch_config::CpusConfig {
                boot_vcpus: config.vcpus,
                max_vcpus: config.vcpus,
            },
            memory: ch_config::MemoryConfig {
                size: config.memory_mib * 1024 * 1024,
            },
            payload: ch_config::PayloadConfig {
                kernel: kernel_path.to_string_lossy().to_string(),
                cmdline: Some(self.build_boot_args(&config)),
                ..Default::default()
            },
            console: Some(ch_config::ConsoleConfig {
                mode: "Off".to_string(),
                ..Default::default()
            }),
            serial: Some(ch_config::SerialConfig {
                mode: "File".to_string(),
                file: Some(serial_log_str),
                ..Default::default()
            }),
            disks: Some(disks),
            net: Some(vec![ch_config::NetConfig {
                id: Some("net0".to_string()),
                tap: tap_name.clone(),
                num_queues: Some(2),
                offload_tso: Some(false),
                offload_ufo: Some(false),
                offload_csum: Some(false),
                ..Default::default()
            }]),
            rng: Some(ch_config::RngConfig {
                src: "/dev/urandom".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        if !fs_devices.is_empty() {
            ch_vm_config.fs = Some(fs_devices);
        }

        let body = serde_json::to_string(&ch_vm_config).map_err(|e| HypervisorError::ApiError {
            path: "/api/v1/vm.create".to_string(),
            msg: format!("Failed to serialize VmConfig: {e}"),
        })?;

        // 6. Send vm.create
        match ch_request(
            "PUT",
            &socket_str,
            "/api/v1/vm.create",
            Some(&body),
            self.ch_connect_timeout(),
            self.ch_status_timeout(),
            self.ch_header_timeout(),
            self.ch_body_timeout(),
        )
        .await
        {
            Ok(_) => tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM created successfully"),
            Err(e) => {
                // If it timed out, verify if it was actually created
                if e.to_string().contains("timeout") {
                    tracing::warn!(vm_id = %vm_id, "vm.create timed out, verifying current state...");
                    let info = ch_request(
                        "GET",
                        &socket_str,
                        "/api/v1/vm.info",
                        None,
                        self.ch_connect_timeout(),
                        self.ch_status_timeout(),
                        self.ch_header_timeout(),
                        self.ch_body_timeout(),
                    )
                    .await?;
                    if info.contains("\"Created\"") || info.contains("\"Running\"") {
                        tracing::info!(vm_id = %vm_id, "Verified VM exists despite timeout");
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            },
        }

        // 7. Send vm.boot
        match ch_request(
            "PUT",
            &socket_str,
            "/api/v1/vm.boot",
            None,
            self.ch_connect_timeout(),
            self.ch_status_timeout(),
            self.ch_header_timeout(),
            self.ch_body_timeout(),
        )
        .await
        {
            Ok(_) => tracing::info!(vm_id = %vm_id, "Cloud Hypervisor VM booted successfully"),
            Err(e) => {
                if e.to_string().contains("timeout") {
                    tracing::warn!(vm_id = %vm_id, "vm.boot timed out, verifying current state...");
                    let info = ch_request(
                        "GET",
                        &socket_str,
                        "/api/v1/vm.info",
                        None,
                        self.ch_connect_timeout(),
                        self.ch_status_timeout(),
                        self.ch_header_timeout(),
                        self.ch_body_timeout(),
                    )
                    .await?;
                    if info.contains("\"Running\"") {
                        tracing::info!(vm_id = %vm_id, "Verified VM is Running despite boot timeout");
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            },
        }

        if config.workload_type == mikrom_proto::scheduler::WorkloadType::Database as i32 {
            match self.configure_database_vm(&vm_id, &config).await {
                Ok(()) => {},
                Err(e) => {
                    tracing::error!(
                        vm_id = %vm_id,
                        error = %e,
                        "Failed to configure Cloud Hypervisor database workload"
                    );
                    let _ = proc.kill().await;
                    self.cleanup_tap(&tap_name).await;
                    return Err(e);
                },
            }
        }

        let proc_arc = Arc::new(RwLock::new(proc));
        self.active_vms.insert(vm_id, proc_arc.clone());
        self.vms.insert(
            vm_id,
            VmInfo {
                vm_id,
                app_id,
                image,
                config,
                status: VmStatus::Running,
                started_at: Some(chrono::Utc::now().timestamp_millis()),
                error_message: None,
            },
        );

        // 8. Persist state for reconciliation
        let proc_lock = proc_arc.read().await;
        if let Err(e) = self.persist_vm_state(&vm_id, &proc_lock).await {
            tracing::warn!(vm_id = %vm_id, "Failed to persist VM state: {e}");
        }

        Ok(())
    }

    fn clone_stub(&self) -> Self {
        Self {
            config: self.config.clone(),
            active_vms: self.active_vms.clone(),
            vms: self.vms.clone(),
            builder: self.builder.clone(),
            nats_client: self.nats_client.clone(),
        }
    }

    async fn run_gc(&self) {
        let mut exited = Vec::new();
        for entry in self.active_vms.iter_mut() {
            let vm_id = *entry.key();
            let proc_lock = entry.value();
            let mut proc = proc_lock.write().await;

            match proc.child.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!(vm_id = %vm_id, status = ?status, "Detected Cloud Hypervisor process exit via GC");
                    exited.push(vm_id);
                },
                Ok(None) => {},
                Err(e) => {
                    tracing::error!(vm_id = %vm_id, error = %e, "Error checking Cloud Hypervisor process status");
                },
            }
        }

        for vm_id in exited {
            if let Some((_, proc_lock)) = self.active_vms.remove(&vm_id) {
                let mut proc = proc_lock.write().await;
                let _ = proc.kill().await;
                let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
                self.cleanup_tap(&tap_name).await;
            }
            self.vms.remove(&vm_id);
            let path = self.config.data_path.join("vms").join(vm_id.to_string());
            let _ = tokio::fs::remove_dir_all(path).await;
        }
    }

    async fn persist_vm_state(
        &self,
        vm_id: &VmId,
        proc: &CloudHypervisorProcess,
    ) -> Result<(), HypervisorError> {
        let info = self.vms.get(vm_id).ok_or_else(|| {
            HypervisorError::ProcessError(format!("VM info not found for {vm_id}"))
        })?;

        let state = PersistedChVm {
            vm_id: *vm_id,
            app_id: info.app_id,
            pid: proc.pid,
            ipv6: info.config.ipv6_address.clone(),
            gw6: info.config.ipv6_gateway.clone(),
            mac: info.config.mac_address.clone(),
            tap_name: format!("ch-tap-{}", &vm_id.to_string()[..8]),
            tap_ifindex: proc.tap_ifindex,
            started_at: info.started_at.unwrap_or(0),
        };

        let path = self
            .config
            .data_path
            .join("vms")
            .join(vm_id.to_string())
            .join("state.json");
        let payload = serde_json::to_vec_pretty(&state).map_err(|e| {
            HypervisorError::ProcessError(format!("Failed to serialize VM state: {e}"))
        })?;

        tokio::fs::write(path, payload)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to write VM state: {e}")))?;

        Ok(())
    }

    fn is_pid_alive(&self, pid: u32) -> bool {
        let mut system = sysinfo::System::new();
        system.refresh_processes(
            sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from(pid as usize)]),
            true,
        );
        if let Some(process) = system.process(sysinfo::Pid::from(pid as usize)) {
            process.name().to_string_lossy().contains("cloud-hypervis")
        } else {
            false
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedChVm {
    vm_id: VmId,
    app_id: AppId,
    pid: u32,
    ipv6: Option<String>,
    gw6: Option<String>,
    mac: Option<String>,
    tap_name: String,
    tap_ifindex: Option<u32>,
    started_at: i64,
}

impl fmt::Debug for CloudHypervisorManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudHypervisorManager").finish()
    }
}

#[async_trait]
impl VmHypervisor for CloudHypervisorManager {
    fn hypervisor_type(&self) -> HypervisorType {
        HypervisorType::CloudHypervisor
    }

    fn agent_id(&self) -> &str {
        &self.config.host_id
    }

    #[allow(clippy::collapsible_if)]
    async fn start_vm(
        &self,
        vm_id: VmId,
        app_id: AppId,
        image: String,
        config: VmConfig,
    ) -> Result<(), HypervisorError> {
        let self_clone = self.clone_stub();
        let image_clone = image.clone();
        let config_clone = config.clone();
        tokio::spawn(async move {
            if let Err(e) = self_clone
                .start_vm_background(vm_id, app_id, image, config)
                .await
            {
                tracing::error!(vm_id = %vm_id, error = %e, "Failed to start Cloud Hypervisor VM in background");
                self_clone.vms.insert(
                    vm_id,
                    VmInfo {
                        vm_id,
                        app_id,
                        image: image_clone,
                        config: config_clone,
                        status: VmStatus::Failed,
                        started_at: None,
                        error_message: Some(e.to_string()),
                    },
                );
            }
        });
        Ok(())
    }

    async fn stop_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        let (socket_path, _, _, _) = self.get_vm_paths(vm_id);
        let socket_str = socket_path.to_string_lossy().to_string();

        if let Some((_, proc_lock)) = self.active_vms.remove(vm_id) {
            let mut proc = proc_lock.write().await;

            // 1. Try graceful shutdown via ACPI power button
            tracing::info!(vm_id = %vm_id, "Attempting graceful shutdown via power button...");
            let _ = ch_request(
                "PUT",
                &socket_str,
                "/api/v1/vm.power-button",
                None,
                self.ch_connect_timeout(),
                self.ch_status_timeout(),
                self.ch_header_timeout(),
                self.ch_body_timeout(),
            )
            .await;

            // 2. Wait for exit
            let mut exited = false;
            for _ in 0..50 {
                match proc.child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    },
                    _ => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                }
            }

            // 3. Fallback to hard kill if still running
            if !exited {
                tracing::warn!(vm_id = %vm_id, "Graceful shutdown timed out, performing hard kill");
                let _ = proc.kill().await;
            }

            let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
            self.cleanup_tap(&tap_name).await;
        }
        self.vms.remove(vm_id);

        // Remove state file
        let path = self.config.data_path.join("vms").join(vm_id.to_string());
        if let Err(e) = tokio::fs::remove_dir_all(&path).await {
            tracing::warn!("Failed to remove VM state directory: {e}");
        }

        Ok(())
    }

    async fn pause_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("pause".to_string()))
    }

    async fn resume_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("resume".to_string()))
    }

    #[allow(clippy::single_match, clippy::collapsible_if)]
    async fn delete_vm(&self, vm_id: &VmId) -> Result<(), HypervisorError> {
        // 1. Best-effort stop
        let _ = self.stop_vm(vm_id).await;

        let (socket_path, _, _, _) = self.get_vm_paths(vm_id);

        if socket_path.exists() {
            tracing::info!(vm_id = %vm_id, "Attempting to shut down orphaned Cloud Hypervisor via API");
            let _ = ch_request(
                "PUT",
                &socket_path.to_string_lossy(),
                "/api/v1/vm.power-button",
                None,
                self.ch_connect_timeout(),
                self.ch_status_timeout(),
                self.ch_header_timeout(),
                self.ch_body_timeout(),
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let _ = ch_request(
                "PUT",
                &socket_path.to_string_lossy(),
                "/api/v1/vm.shutdown",
                None,
                self.ch_connect_timeout(),
                self.ch_status_timeout(),
                self.ch_header_timeout(),
                self.ch_body_timeout(),
            )
            .await;
        }

        let path = self.config.data_path.join("vms").join(vm_id.to_string());
        let state_path = path.join("state.json");

        if state_path.exists() {
            match tokio::fs::read_to_string(&state_path).await {
                Ok(state_str) => match serde_json::from_str::<PersistedChVm>(&state_str) {
                    Ok(state) => {
                        let pid = state.pid;
                        if self.is_pid_alive(pid) {
                            tracing::info!(vm_id = %vm_id, pid = pid, "Killing orphaned Cloud Hypervisor process via PID");
                            let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                        }
                    },
                    Err(_) => {},
                },
                Err(_) => {},
            }
        }

        let tap_name = format!("ch-tap-{}", &vm_id.to_string()[..8]);
        self.cleanup_tap(&tap_name).await;

        // 2. Remove state directory
        let _ = tokio::fs::remove_dir_all(path).await;

        // 3. Finally remove from memory
        self.vms.remove(vm_id);

        Ok(())
    }

    async fn restart_vm(&self, _vm_id: &VmId) -> Result<(), HypervisorError> {
        Err(HypervisorError::UnsupportedOperation("restart".to_string()))
    }

    #[allow(clippy::single_match, clippy::collapsible_if)]
    async fn get_vm_info(&self, vm_id: &VmId) -> Option<VmInfo> {
        let mut info = self.vms.get(vm_id)?.clone();

        let socket_path = self.get_vm_paths(vm_id).0;
        let socket_str = socket_path.to_string_lossy().to_string();

        if let Ok(resp_body) = ch_request(
            "GET",
            &socket_str,
            "/api/v1/vm.info",
            None,
            self.ch_connect_timeout(),
            self.ch_status_timeout(),
            self.ch_header_timeout(),
            self.ch_body_timeout(),
        )
        .await
        {
            match serde_json::from_str::<ch_config::VmInfoResponse>(&resp_body) {
                Ok(ch_info) => {
                    info.status = match ch_info.state.as_str() {
                        "Created" => VmStatus::Starting,
                        "Running" => VmStatus::Running,
                        "Paused" => VmStatus::Paused,
                        "Shutdown" => VmStatus::Stopped,
                        _ => VmStatus::Failed,
                    };
                },
                Err(_) => {},
            }
        }

        Some(info)
    }

    async fn get_all_vms(&self) -> Vec<VmDetailedInfo> {
        let mut results = Vec::new();
        for entry in self.vms.iter() {
            let vm_id = entry.key();

            if let Some(info) = self.get_vm_info(vm_id).await {
                let (pid, tap_ifindex) = if let Some(proc_lock) = self.active_vms.get(vm_id) {
                    let proc = proc_lock.read().await;
                    (Some(proc.pid), proc.tap_ifindex)
                } else {
                    (None, None)
                };

                // Fetch CH counters
                let socket_path = self.get_vm_paths(vm_id).0;
                let socket_str = socket_path.to_string_lossy().to_string();
                let raw_metrics = if info.status == VmStatus::Running {
                    match ch_request(
                        "GET",
                        &socket_str,
                        "/api/v1/vm.counters",
                        None,
                        self.ch_connect_timeout(),
                        self.ch_status_timeout(),
                        self.ch_header_timeout(),
                        self.ch_body_timeout(),
                    )
                    .await
                    {
                        Ok(resp) => serde_json::from_str(&resp).ok(),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                results.push(VmDetailedInfo {
                    vm_id: *vm_id,
                    app_id: info.app_id,
                    status: info.status,
                    error_message: info.error_message.clone(),
                    pid,
                    metrics_path: None,
                    socket_path: Some(socket_str),
                    tap_name: Some(format!("ch-tap-{}", &vm_id.to_string()[..8])),
                    tap_ifindex,
                    raw_metrics,
                });
            }
        }
        results
    }

    async fn get_vm_started_at_ms(&self, vm_id: &VmId) -> Option<u64> {
        self.vms
            .get(vm_id)
            .and_then(|v| v.started_at.map(|t| t as u64))
    }

    async fn is_app_started(&self, _vm_id: &VmId) -> bool {
        true // Cloud Hypervisor doesn't have a built-in health check for apps yet
    }

    fn get_logs(&self, _vm_id: &VmId) -> Vec<String> {
        Vec::new()
    }

    async fn update_vm_firewall(
        &self,
        _vm_id: &VmId,
        _rules: Vec<mikrom_agent_ebpf_common::FirewallRule>,
    ) -> Result<(), HypervisorError> {
        Ok(())
    }

    async fn init_network(&self) -> Result<(), HypervisorError> {
        crate::network::ensure_host_networking().await
    }

    async fn load_runtime_state(&self) -> Result<(), HypervisorError> {
        let vms_dir = self.config.data_path.join("vms");
        if !vms_dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&vms_dir)
            .await
            .map_err(|e| HypervisorError::ProcessError(format!("Failed to read vms dir: {e}")))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let state_path = path.join("state.json");
            if !state_path.exists() {
                continue;
            }

            let Ok(raw) = tokio::fs::read_to_string(&state_path).await else {
                continue;
            };

            let Ok(state) = serde_json::from_str::<PersistedChVm>(&raw) else {
                continue;
            };

            // Check if PID is still alive and is cloud-hypervisor
            if self.is_pid_alive(state.pid) {
                tracing::info!(vm_id = %state.vm_id, pid = %state.pid, "Reconciling Cloud Hypervisor VM");

                // Reconstruct the process handle
                // We use a command that exits immediately just to satisfy the tokio::process::Child requirement
                let mut cmd = tokio::process::Command::new("true");
                let child = cmd.spawn().unwrap();

                let proc = CloudHypervisorProcess {
                    vm_id: state.vm_id.to_string(),
                    socket_path: self.get_vm_paths(&state.vm_id).0,
                    pid: state.pid,
                    child,
                    sidecars: Vec::new(),
                    tap_ifindex: state.tap_ifindex,
                };

                self.active_vms
                    .insert(state.vm_id, Arc::new(RwLock::new(proc)));
                self.vms.insert(
                    state.vm_id,
                    VmInfo {
                        vm_id: state.vm_id,
                        app_id: state.app_id,
                        image: "".to_string(), // Placeholder, might want to persist image too
                        config: VmConfig {
                            vcpus: 1, // Default or persist it
                            memory_mib: 512,
                            ipv6_address: state.ipv6.clone(),
                            ipv6_gateway: state.gw6.clone(),
                            mac_address: state.mac.clone(),
                            ..Default::default()
                        },
                        status: VmStatus::Running,
                        error_message: None,
                        started_at: Some(state.started_at),
                    },
                );

                // Ensure routing is still there
                if let Err(e) = self.setup_routing(state.ipv6.as_deref()).await {
                    tracing::warn!(
                        "Failed to restore routing for recovered VM {}: {e}",
                        state.vm_id
                    );
                }
            }
        }

        Ok(())
    }

    async fn persist_runtime_state(&self) -> Result<(), HypervisorError> {
        Ok(())
    }

    async fn cleanup_all_stale_resources(&self) {
        let vms_dir = self.config.data_path.join("vms");
        let Ok(mut entries) = tokio::fs::read_dir(&vms_dir).await else {
            return;
        };

        let active_ids: std::collections::HashSet<String> = self
            .active_vms
            .iter()
            .map(|e| e.key().to_string())
            .collect();

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(vm_id_str) = path.file_name().and_then(|n| n.to_str())
                && !active_ids.contains(vm_id_str)
                && uuid::Uuid::parse_str(vm_id_str).is_ok()
            {
                tracing::debug!(vm_id = %vm_id_str, "Removing stale Cloud Hypervisor VM directory");
                if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                    tracing::warn!("Failed to remove stale VM directory: {e}");
                }
            }
        }
    }

    async fn set_nats_client(&self, client: async_nats::Client) {
        let mut nats = self.nats_client.write().await;
        *nats = Some(client);
    }

    fn start_background_tasks(&self) {
        let self_clone = Arc::new(RwLock::new(self.clone_stub()));
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                let mgr = self_clone.read().await;
                mgr.run_gc().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn workload_artifacts_select_neon_for_database() {
        let mgr = CloudHypervisorManager {
            config: AgentConfig {
                nats_url: "nats://localhost:4222".to_string(),
                host_id: "host-1".to_string(),
                use_tls: false,
                bridge_ip: "fd00::1/64".to_string(),
                certs_dir: "/certs/agent".to_string(),
                data_path: PathBuf::from("/tmp"),
                agent_hostname: None,
                agent_advertise_address: None,
                wireguard_port: None,
                wireguard_pubkey: None,
                cloud_hypervisor_enabled: true,
                cloud_hypervisor_binary: PathBuf::from("/usr/bin/cloud-hypervisor"),
                cloud_hypervisor_kernel: PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin"),
                cloud_hypervisor_base_rootfs: PathBuf::from(
                    "/opt/cloud-hypervisor/base-rootfs.ext4",
                ),
                cloud_hypervisor_database_kernel: PathBuf::from("/opt/neon/vmlinux.bin"),
                cloud_hypervisor_database_base_rootfs: PathBuf::from("/opt/neon/base-rootfs.ext4"),
                http_port: 5002,
                max_vms_per_host: 0,
                nats_flapping_session_secs: 30,
            },
            active_vms: Arc::new(DashMap::new()),
            vms: Arc::new(DashMap::new()),
            builder: Arc::new(crate::builder::ImageBuilder),
            nats_client: Arc::new(RwLock::new(None)),
        };

        let (kernel, rootfs) =
            mgr.workload_artifacts(mikrom_proto::scheduler::WorkloadType::Database as i32);
        assert_eq!(kernel, Path::new("/opt/neon/vmlinux.bin"));
        assert_eq!(rootfs, Path::new("/opt/neon/base-rootfs.ext4"));
    }

    #[test]
    fn boot_args_include_ipv6_network_parameters_when_present() {
        let mgr = CloudHypervisorManager {
            config: AgentConfig {
                nats_url: "nats://localhost:4222".to_string(),
                host_id: "host-1".to_string(),
                use_tls: false,
                bridge_ip: "fd00::1/64".to_string(),
                certs_dir: "/certs/agent".to_string(),
                data_path: PathBuf::from("/tmp"),
                agent_hostname: None,
                agent_advertise_address: None,
                wireguard_port: None,
                wireguard_pubkey: None,
                cloud_hypervisor_enabled: true,
                cloud_hypervisor_binary: PathBuf::from("/usr/bin/cloud-hypervisor"),
                cloud_hypervisor_kernel: PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin"),
                cloud_hypervisor_base_rootfs: PathBuf::from(
                    "/opt/cloud-hypervisor/base-rootfs.ext4",
                ),
                cloud_hypervisor_database_kernel: PathBuf::from("/opt/neon/vmlinux.bin"),
                cloud_hypervisor_database_base_rootfs: PathBuf::from("/opt/neon/base-rootfs.ext4"),
                http_port: 5002,
                max_vms_per_host: 0,
                nats_flapping_session_secs: 30,
            },
            active_vms: Arc::new(DashMap::new()),
            vms: Arc::new(DashMap::new()),
            builder: Arc::new(crate::builder::ImageBuilder),
            nats_client: Arc::new(RwLock::new(None)),
        };

        let config = VmConfig {
            ipv6_address: Some("fd40:b90d:fc5f:1ae0::2".to_string()),
            ipv6_gateway: Some("fd40:b90d:fc5f:1ae0::1".to_string()),
            ..Default::default()
        };

        let boot_args = mgr.build_boot_args(&config);
        assert!(boot_args.contains("init=/mikrom-init"));
        assert!(
            boot_args
                .contains("ip=[fd40:b90d:fc5f:1ae0::2]::[fd40:b90d:fc5f:1ae0::1]:64::eth0:off")
        );
    }

    #[test]
    fn test_persisted_ch_vm_serialization() {
        let vm_id = VmId::new();
        let app_id = AppId::new();
        let state = PersistedChVm {
            vm_id,
            app_id,
            pid: 1234,
            ipv6: Some("fd40::1".to_string()),
            gw6: Some("fe80::1".to_string()),
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            tap_name: "ch-tap-test".to_string(),
            tap_ifindex: Some(1),
            started_at: 1716900000,
        };

        let json = serde_json::to_string(&state).unwrap();
        let decoded: PersistedChVm = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.vm_id, vm_id);
        assert_eq!(decoded.app_id, app_id);
        assert_eq!(decoded.pid, 1234);
        assert_eq!(decoded.ipv6, Some("fd40::1".to_string()));
        assert_eq!(decoded.gw6, Some("fe80::1".to_string()));
    }

    #[test]
    fn database_configure_helpers_use_persisted_ids_and_ipv6_port() {
        let mgr = CloudHypervisorManager {
            config: AgentConfig {
                nats_url: "nats://localhost:4222".to_string(),
                host_id: "host-1".to_string(),
                use_tls: false,
                bridge_ip: "fd00::1/64".to_string(),
                certs_dir: "/certs/agent".to_string(),
                data_path: PathBuf::from("/tmp"),
                agent_hostname: None,
                agent_advertise_address: None,
                wireguard_port: None,
                wireguard_pubkey: None,
                cloud_hypervisor_enabled: true,
                cloud_hypervisor_binary: PathBuf::from("/usr/bin/cloud-hypervisor"),
                cloud_hypervisor_kernel: PathBuf::from("/opt/cloud-hypervisor/vmlinux.bin"),
                cloud_hypervisor_base_rootfs: PathBuf::from(
                    "/opt/cloud-hypervisor/base-rootfs.ext4",
                ),
                cloud_hypervisor_database_kernel: PathBuf::from("/opt/neon/vmlinux.bin"),
                cloud_hypervisor_database_base_rootfs: PathBuf::from("/opt/neon/base-rootfs.ext4"),
                http_port: 5002,
                max_vms_per_host: 0,
                nats_flapping_session_secs: 30,
            },
            active_vms: Arc::new(DashMap::new()),
            vms: Arc::new(DashMap::new()),
            builder: Arc::new(crate::builder::ImageBuilder),
            nats_client: Arc::new(RwLock::new(None)),
        };

        let url = CloudHypervisorManager::build_database_configure_url("fd40:b90d:fc5f:1ae0::1");
        assert_eq!(url, "http://[fd40:b90d:fc5f:1ae0::1]:3080/configure");

        let mut env = std::collections::HashMap::new();
        env.insert("NEON_TENANT_ID".to_string(), "tenant-123".to_string());
        env.insert("NEON_TIMELINE_ID".to_string(), "timeline-456".to_string());
        env.insert("MIKROM_DATABASE_ID".to_string(), "db-789".to_string());

        let config = VmConfig {
            env,
            ipv6_address: Some("fd40:b90d:fc5f:1ae0::1".to_string()),
            workload_type: mikrom_proto::scheduler::WorkloadType::Database as i32,
            ..Default::default()
        };

        let spec = mgr
            .build_database_configure_spec(&VmId::new(), &config)
            .unwrap();
        let expected_pageserver_host =
            CloudHypervisorManager::neon_host_alias("neon-pageserver", "fd00::deed:1d1c");

        assert_eq!(spec["spec"]["format_version"], 1.0);
        assert_eq!(spec["spec"]["cluster"]["cluster_id"], "db-789");
        assert_eq!(spec["spec"]["tenant_id"], "tenant-123");
        assert_eq!(spec["spec"]["timeline_id"], "timeline-456");
        assert_eq!(spec["spec"]["mode"], "Primary");
        assert_eq!(
            spec["spec"]["pageserver_connstring"],
            format!("host={expected_pageserver_host} port=6400")
        );
        assert_eq!(
            spec["spec"]["safekeeper_connstrings"],
            serde_json::json!([format!("{expected_pageserver_host}:5454")])
        );
        assert_eq!(spec["spec"]["safekeepers_generation"], serde_json::json!(1));
        assert!(spec["spec"]["timestamp"].as_str().unwrap().len() > 10);
        assert!(spec["spec"]["operation_uuid"].as_str().unwrap().len() > 10);
        assert_eq!(
            spec["compute_ctl_config"]["jwks"],
            serde_json::json!({"keys": []})
        );

        assert_eq!(mgr.database_configure_token(&config), None);

        let mut config_with_token = config.clone();
        config_with_token.env.insert(
            NEON_CONFIGURE_TOKEN_ENV.to_string(),
            "  token-123  ".to_string(),
        );
        assert_eq!(
            mgr.database_configure_token(&config_with_token),
            Some("token-123")
        );
    }

    #[test]
    fn database_configure_retry_window_defaults_and_parses_env_value() {
        assert_eq!(
            CloudHypervisorManager::parse_database_configure_retry_window(None),
            Duration::from_secs(60)
        );
        assert_eq!(
            CloudHypervisorManager::parse_database_configure_retry_window(Some("120")),
            Duration::from_secs(120)
        );
        assert_eq!(
            CloudHypervisorManager::parse_database_configure_retry_window(Some("0")),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn database_configure_status_412_is_retryable() {
        assert!(!CloudHypervisorManager::is_fatal_database_configure_status(
            reqwest::StatusCode::PRECONDITION_FAILED
        ));
        assert!(!CloudHypervisorManager::is_fatal_database_configure_status(
            reqwest::StatusCode::REQUEST_TIMEOUT
        ));
        assert!(!CloudHypervisorManager::is_fatal_database_configure_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(CloudHypervisorManager::is_fatal_database_configure_status(
            reqwest::StatusCode::BAD_REQUEST
        ));
    }

    #[test]
    fn normalize_neon_safekeeper_connstr_rejects_unbracketed_ipv6_without_port() {
        assert_eq!(
            CloudHypervisorManager::normalize_neon_safekeeper_connstr(
                "fd40:b90d:fc5f:1ae0::1",
                "neon-safekeeper",
            ),
            None
        );
    }
}
