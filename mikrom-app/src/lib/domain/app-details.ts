import type { DeploymentInfo, LiveDeploymentInfo, VmMetricsResponse } from "$lib/api";

export type MetricsSnapshot = {
  time: string;
  cpu: number;
  ram: number;
  rx: number;
  tx: number;
  total_rx: number;
  total_tx: number;
};

export type DetailBadgeProps = {
  variant: "outline" | "destructive";
  className: string;
};

export function normalizeCpuUsage(cpuUsage?: number) {
  const value = cpuUsage ?? 0;
  return value <= 1 ? value * 100 : value;
}

export function formatNetworkRate(kibPerSecond: number) {
  if (!kibPerSecond || kibPerSecond <= 0) return "0 KiB/s";
  if (kibPerSecond < 0.1) return `${(kibPerSecond * 1024).toFixed(0)} B/s`;
  if (kibPerSecond >= 1024) return `${(kibPerSecond / 1024).toFixed(1)} MiB/s`;
  return `${kibPerSecond.toFixed(1)} KiB/s`;
}

export function formatBytes(bytes: number) {
  if (!bytes || bytes <= 0) return "0 B";
  const unit = 1024;
  const sizes = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = Math.floor(Math.log(bytes) / Math.log(unit));
  if (index < 0) return "0 B";
  return `${(bytes / Math.pow(unit, index)).toFixed(1)} ${sizes[index]}`;
}

export function sortDeployments(list: DeploymentInfo[]) {
  return [...list].sort(
    (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
  );
}

export function formatDeploymentDate(dateStr: string) {
  const value = new Date(dateStr);
  if (Number.isNaN(value.getTime())) return "--";
  return value.toLocaleString();
}

export function normalizeDeployment(
  deployment: DeploymentInfo | LiveDeploymentInfo,
  previous?: DeploymentInfo,
) {
  const full = deployment as Partial<DeploymentInfo>;
  const live = deployment as LiveDeploymentInfo;
  const fallbackTime = previous?.created_at ?? previous?.updated_at ?? new Date().toISOString();
  const canonicalId =
    ("deployment_id" in deployment ? live.deployment_id : null) ??
    ("id" in deployment ? full.id : null) ??
    deployment.job_id ??
    previous?.id ??
    "";

  return {
    id: canonicalId,
    app_id: full.app_id ?? live.app_id,
    build_id: full.build_id ?? previous?.build_id ?? null,
    image_tag: full.image_tag ?? previous?.image_tag ?? null,
    job_id: deployment.job_id ?? previous?.job_id ?? null,
    ipv6_address: deployment.ipv6_address ?? previous?.ipv6_address ?? null,
    status: deployment.status ?? previous?.status ?? "UNKNOWN",
    vcpus: full.vcpus ?? previous?.vcpus ?? 0,
    memory_mib: full.memory_mib ?? previous?.memory_mib ?? 0,
    disk_mib: full.disk_mib ?? previous?.disk_mib ?? 0,
    port: full.port ?? previous?.port ?? 0,
    env_vars: full.env_vars ?? previous?.env_vars ?? {},
    git_commit_hash: full.git_commit_hash ?? previous?.git_commit_hash ?? null,
    git_commit_message: full.git_commit_message ?? previous?.git_commit_message ?? null,
    git_branch: full.git_branch ?? previous?.git_branch ?? null,
    trigger_source: full.trigger_source ?? previous?.trigger_source ?? "manual",
    scale_state: full.scale_state ?? live.scale_state ?? previous?.scale_state,
    created_at: previous?.created_at ?? full.created_at ?? fallbackTime,
    updated_at: previous?.updated_at ?? full.updated_at ?? fallbackTime,
  };
}

export function getDeploymentBadgeProps(status: string): DetailBadgeProps {
  const s = status.toLowerCase();
  if (s === "running") {
    return {
      variant: "outline",
      className: "border-transparent bg-status-info/10 text-status-info",
    };
  }

  if (
    s === "draining" ||
    s === "building" ||
    s === "scheduled" ||
    s === "pending" ||
    s === "paused"
  ) {
    return {
      variant: "outline",
      className: "border-transparent bg-status-warning/10 text-status-warning",
    };
  }

  if (s === "failed" || s === "cancelled") {
    return {
      variant: "destructive",
      className: "",
    };
  }

  return {
    variant: "outline",
    className: "",
  };
}

export function getDeploymentButtonText(
  dep: DeploymentInfo,
  isCurrentlyInProd: boolean,
) {
  if (isCurrentlyInProd) return "Currently in Prod";
  if (dep.status === "DRAINING") return "Draining...";
  if (dep.status === "BUILDING") return "Building...";
  if (dep.status === "STARTING" || dep.status === "SCHEDULED") return "Starting...";
  return "Promote to Prod";
}

export function buildMetricSnapshot(
  sample: VmMetricsResponse,
  now: number,
  previous?: { tx: number; rx: number; time: number; txRate: number; rxRate: number },
) {
  const txBytes = sample.tx_bytes || 0;
  const rxBytes = sample.rx_bytes || 0;

  let txRate = 0;
  let rxRate = 0;

  if (previous) {
    const deltaTime = (now - previous.time) / 1000;
    if (deltaTime > 0.8) {
      txRate = Math.max(0, txBytes - previous.tx) / deltaTime / 1024;
      rxRate = Math.max(0, rxBytes - previous.rx) / deltaTime / 1024;
    } else {
      txRate = previous.txRate || 0;
      rxRate = previous.rxRate || 0;
    }
  }

  return {
    cache: {
      tx: txBytes,
      rx: rxBytes,
      time: now,
      txRate,
      rxRate,
    },
    sample: {
      cpu: normalizeCpuUsage(sample.cpu_usage),
      ram: (sample.ram_used_bytes || 0) / (1024 * 1024),
      rx: rxRate,
      tx: txRate,
      total_rx: rxBytes,
      total_tx: txBytes,
      lastUpdate: now,
    },
  };
}

export function aggregateReplicaMetrics(
  activeReplicas: Array<{
    cpu: number;
    ram: number;
    rx: number;
    tx: number;
    total_rx: number;
    total_tx: number;
  }>,
): MetricsSnapshot {
  let cpuTotal = 0;
  let ramTotal = 0;
  let rxTotal = 0;
  let txTotal = 0;
  let totalRx = 0;
  let totalTx = 0;

  for (const replica of activeReplicas) {
    cpuTotal += replica.cpu;
    ramTotal += replica.ram;
    rxTotal += replica.rx;
    txTotal += replica.tx;
    totalRx += replica.total_rx;
    totalTx += replica.total_tx;
  }

  const count = activeReplicas.length;
  return {
    time: new Date().toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    }),
    cpu: count > 0 ? cpuTotal / count : 0,
    ram: ramTotal,
    rx: rxTotal,
    tx: txTotal,
    total_rx: totalRx,
    total_tx: totalTx,
  };
}
