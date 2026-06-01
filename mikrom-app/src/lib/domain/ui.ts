import type { AppInfo, HealthResponse, LiveDeploymentInfo } from "$lib/api";

export type AppFilterState = "active" | "paused" | "idle";

export type AppResources = {
  vcpus: number;
  memory_mib: number;
  count: number;
};

export type AppCard = AppInfo & {
  liveVm: LiveDeploymentInfo | null;
  resources: AppResources;
  filterState: AppFilterState;
  scaleLabel: string;
  scaleBadgeClass: string;
  searchText: string;
};

export type DashboardAppRow = AppInfo & {
  liveVm: LiveDeploymentInfo | null;
  status: string;
  statusVariant: "outline" | "secondary" | "destructive";
  statusClass: string;
};

export type DashboardSummary = {
  totalApps: number;
  runningCount: number;
  pendingCount: number;
  hasUndeployedApps: boolean;
  recentApps: DashboardAppRow[];
};

export type HealthDisplayStatus = "ONLINE" | "CHECKING" | "OFFLINE";

type VmsByKey = Map<string, LiveDeploymentInfo[]>;

function normalizeStatus(status: string | undefined | null) {
  return (status || "").toLowerCase();
}

function collectVmsByKey(vms: LiveDeploymentInfo[]) {
  const byAppId: VmsByKey = new Map();
  const byAppName: VmsByKey = new Map();
  let runningCount = 0;
  let pendingCount = 0;

  for (const vm of vms) {
    const status = normalizeStatus(vm.status);
    if (status === "running") runningCount += 1;
    if (["scheduled", "pending", "building"].includes(status)) pendingCount += 1;

    if (vm.app_id) {
      const bucket = byAppId.get(vm.app_id) || [];
      bucket.push(vm);
      byAppId.set(vm.app_id, bucket);
    }

    if (vm.app_name) {
      const bucket = byAppName.get(vm.app_name) || [];
      bucket.push(vm);
      byAppName.set(vm.app_name, bucket);
    }
  }

  return { byAppId, byAppName, runningCount, pendingCount };
}

function uniqueVms(app: AppInfo, byAppId: VmsByKey, byAppName: VmsByKey) {
  const candidates = [...(byAppId.get(app.id) || []), ...(byAppName.get(app.name) || [])];
  const seen = new Set<string>();

  return candidates.filter((vm) => {
    const key = vm.job_id || vm.deployment_id || vm.vm_id;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

export function sortByCreatedAtDesc<T extends { created_at: string }>(items: T[]) {
  return [...items].sort(
    (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
  );
}

export function getAppResources(app: AppInfo, byAppId: VmsByKey, byAppName: VmsByKey): AppResources {
  const runningVms = uniqueVms(app, byAppId, byAppName).filter((vm) => normalizeStatus(vm.status) === "running");

  return {
    vcpus: runningVms.reduce((total, vm) => total + (vm.vcpus || 1), 0),
    memory_mib: runningVms.reduce((total, vm) => total + (vm.memory_mib || 128), 0),
    count: runningVms.length,
  };
}

export function getAppFilterState(app: AppInfo, resources: AppResources): AppFilterState {
  if (resources.count > 0) return "active";
  if (app.scale_state === "scaled_to_zero") return "paused";
  return "idle";
}

export function getScaleStateLabel(scaleState: string) {
  return scaleState === "scaled_to_zero" ? "Paused" : "Running";
}

export function getScaleStateBadgeClass(scaleState: string) {
  return scaleState === "scaled_to_zero"
    ? "border-transparent bg-muted/70 text-muted-foreground"
    : "border-transparent bg-status-info/10 text-status-info";
}

export function buildAppCards(apps: AppInfo[], vms: LiveDeploymentInfo[]) {
  const { byAppId, byAppName } = collectVmsByKey(vms);

  return sortByCreatedAtDesc(apps).map((app) => {
    const liveVms = uniqueVms(app, byAppId, byAppName);
    const resources = getAppResources(app, byAppId, byAppName);

    return {
      ...app,
      liveVm: liveVms[0] || null,
      resources,
      filterState: getAppFilterState(app, resources),
      scaleLabel: getScaleStateLabel(app.scale_state),
      scaleBadgeClass: getScaleStateBadgeClass(app.scale_state),
      searchText: [app.name, app.hostname, app.git_url].filter(Boolean).join(" ").toLowerCase(),
    };
  });
}

export function filterAppCards(cards: AppCard[], query: string, statusFilter: "all" | AppFilterState) {
  const normalizedQuery = query.trim().toLowerCase();

  return cards.filter((card) => {
    const matchesQuery =
      normalizedQuery.length === 0 || card.searchText.includes(normalizedQuery);
    const matchesStatus =
      statusFilter === "all" || card.filterState === statusFilter;

    return matchesQuery && matchesStatus;
  });
}

export function buildDashboardSummary(apps: AppInfo[], vms: LiveDeploymentInfo[]): DashboardSummary {
  const { byAppId, byAppName, runningCount, pendingCount } = collectVmsByKey(vms);
  const sortedApps = sortByCreatedAtDesc(apps);

  const recentApps = sortedApps.slice(0, 5).map((app) => {
    const liveVms = uniqueVms(app, byAppId, byAppName);
    const liveVm = liveVms[0] || null;
    let status = liveVm?.status || (app.active_deployment_id ? "Paused" : "Stopped");

    if (app.scale_state === "scaled_to_zero" && status !== "Stopped") {
      status = "Paused";
    }

    return {
      ...app,
      liveVm,
      status,
      ...getRuntimeStatusPresentation(status),
    };
  });

  return {
    totalApps: apps.length,
    runningCount,
    pendingCount,
    hasUndeployedApps:
      apps.length > 0 &&
      apps.every((app) => uniqueVms(app, byAppId, byAppName).length === 0),
    recentApps,
  };
}

export function getRuntimeStatusPresentation(status: string) {
  const normalized = normalizeStatus(status);

  if (normalized === "running") {
    return {
      statusVariant: "outline" as const,
      statusClass: "border-transparent bg-status-info/10 text-status-info",
    };
  }

  if (normalized === "paused") {
    return {
      statusVariant: "outline" as const,
      statusClass: "border-transparent bg-muted/70 text-muted-foreground",
    };
  }

  if (normalized === "stopped") {
    return {
      statusVariant: "outline" as const,
      statusClass: "border-transparent bg-muted/40 text-muted-foreground/60",
    };
  }

  if (
    ["building", "pending", "scheduled", "starting", "draining"].includes(normalized)
  ) {
    return {
      statusVariant: "secondary" as const,
      statusClass: "",
    };
  }

  if (["failed", "cancelled", "offline", "error"].includes(normalized)) {
    return {
      statusVariant: "destructive" as const,
      statusClass: "",
    };
  }

  return {
    statusVariant: "outline" as const,
    statusClass: "",
  };
}

export function getHealthClass(status: HealthDisplayStatus) {
  if (status === "ONLINE") {
    return "!border-transparent !bg-status-online/10 !text-status-online";
  }

  if (status === "CHECKING") {
    return "!border-transparent !bg-muted/70 !text-muted-foreground";
  }

  return "!border-transparent !bg-status-offline/10 !text-status-offline";
}

export function getSystemHealthStatus(health: HealthResponse | null | undefined): HealthDisplayStatus {
  if (!health) return "OFFLINE";

  const services = Object.values(health.services || {});
  if (services.length === 0) return "CHECKING";
  if (services.every((status) => status === "ONLINE")) return "ONLINE";
  if (services.some((status) => status !== "ONLINE")) return "OFFLINE";

  return "CHECKING";
}
