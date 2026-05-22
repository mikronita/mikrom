<script lang="ts">
  import { onMount } from "svelte";
  import { browser } from "$app/environment";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import {
    ArrowDownToLine,
    ArrowUpFromLine,
    Boxes,
    Cpu,
    ExternalLink,
    GitBranch,
    Globe2,
    Loader2,
    MemoryStick,
    Rocket,
    User,
    Zap,
    Eye,
    EyeOff,
    Clipboard,
    Trash2,
    Cog,
    CheckCircle2,
    Info,
    Scale,
  } from "lucide-svelte";
  import { SvelteMap } from "svelte/reactivity";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Badge,
    Button,
    ButtonGroup,
    AlertDialog,
    EmptyState,
    Modal,
    Input,
    Skeleton,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import MetricChart from "$lib/components/MetricChart.svelte";
  import ScaleAppModal from "$lib/components/ScaleAppModal.svelte";
  import DeployAppModal from "$lib/components/DeployAppModal.svelte";
  import { getToken } from "$lib/auth";
  import {
    activateDeployment,
    deleteApp,
    getAppSecret,
    listDeployments,
    type AppInfo,
    type DeploymentInfo,
    type LiveDeploymentInfo,
    type VmMetricsResponse,
    watchAppMetrics,
    watchDeploymentsSSE,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { vmsStore, vmsLoading } from "$lib/stores/vms";

  type MetricsSnapshot = {
    time: string;
    cpu: number;
    ram: number;
    rx: number;
    tx: number;
    total_rx: number;
    total_tx: number;
  };

  const webhookBaseUrl = browser
    ? `${window.location.protocol}//${window.location.hostname}:5001/v1`
    : "http://localhost:5001/v1";

  let deployments: DeploymentInfo[] = [];
  let loading = true;
  let liveMetrics: VmMetricsResponse | null = null;
  let metricsHistory: MetricsSnapshot[] = [];
  let secret: string | null = null;
  let showSecret = false;
  let showWebhookModal = false;
  let showScaleModal = false;
  let showDeployModal = false;
  let showDeleteAppDialog = false;
  let deletingApp = false;
  let activatingDeploymentId: string | null = null;
  let app: AppInfo | null = null;
  let active: DeploymentInfo | null;
  let inFlight: DeploymentInfo | undefined;
  let latestMetrics: MetricsSnapshot;
  let totalTrafficBytes: number;
  let recentDeploymentsList: DeploymentInfo[];
  let metricCards: Array<{
    key: string;
    label: string;
    detail: string;
    value: string;
    icon: typeof Cpu;
    color: string;
  }>;
  let showLivePerformance: boolean;

  $: appName = decodeURIComponent($page.params.appName ?? "");
  $: app = $appsStore.find((item) => item.name === appName) ?? null;
  $: appScaleState = app?.scale_state ?? "scaled_to_zero";

  const lastNetwork = new SvelteMap<
    string,
    { tx: number; rx: number; time: number; txRate: number; rxRate: number }
  >();
  const replicaSamples = new SvelteMap<
    string,
    {
      cpu: number;
      ram: number;
      rx: number;
      tx: number;
      total_rx: number;
      total_tx: number;
      lastUpdate: number;
    }
  >();
  $: runningReplicaCount = app
    ? $vmsStore.filter(
        (vm) =>
          vm.status.toLowerCase() === "running" &&
          (vm.app_id === app.id || vm.app_name === app.name),
      ).length
    : 0;

  function normalizeCpuUsage(cpuUsage?: number) {
    const value = cpuUsage ?? 0;
    return value <= 1 ? value * 100 : value;
  }

  function formatNetworkRate(kibPerSecond: number) {
    if (!kibPerSecond || kibPerSecond <= 0) return "0 KiB/s";
    if (kibPerSecond < 0.1) return `${(kibPerSecond * 1024).toFixed(0)} B/s`;
    if (kibPerSecond >= 1024)
      return `${(kibPerSecond / 1024).toFixed(1)} MiB/s`;
    return `${kibPerSecond.toFixed(1)} KiB/s`;
  }

  function formatBytes(bytes: number) {
    if (!bytes || bytes <= 0) return "0 B";
    const unit = 1024;
    const sizes = ["B", "KiB", "MiB", "GiB", "TiB"];
    const index = Math.floor(Math.log(bytes) / Math.log(unit));
    if (index < 0) return "0 B";
    return `${(bytes / Math.pow(unit, index)).toFixed(1)} ${sizes[index]}`;
  }

  function sortDeployments(list: DeploymentInfo[]) {
    return [...list].sort(
      (a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
    );
  }

  function formatDeploymentDate(dateStr: string) {
    const value = new Date(dateStr);
    if (Number.isNaN(value.getTime())) return "--";
    return value.toLocaleString();
  }

  function formatReplicaSummary(appInfo: AppInfo) {
    if (appInfo.autoscaling_enabled) {
      if ($vmsLoading && runningReplicaCount === 0)
        return `--/${appInfo.max_replicas}`;
      return `${runningReplicaCount}/${appInfo.max_replicas}`;
    }

    if ($vmsLoading && runningReplicaCount === 0) return "--";
    return `${runningReplicaCount}`;
  }

  function nowIso() {
    return new Date().toISOString();
  }

  function normalizeDeployment(
    deployment: DeploymentInfo | LiveDeploymentInfo,
    previous?: DeploymentInfo,
  ): DeploymentInfo {
    const full = deployment as Partial<DeploymentInfo>;
    const live = deployment as LiveDeploymentInfo;
    const fallbackTime =
      previous?.created_at ?? previous?.updated_at ?? nowIso();
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
      git_commit_hash:
        full.git_commit_hash ?? previous?.git_commit_hash ?? null,
      git_commit_message:
        full.git_commit_message ?? previous?.git_commit_message ?? null,
      git_branch: full.git_branch ?? previous?.git_branch ?? null,
      trigger_source:
        full.trigger_source ?? previous?.trigger_source ?? "manual",
      scale_state:
        full.scale_state ?? live.scale_state ?? previous?.scale_state,
      created_at: previous?.created_at ?? full.created_at ?? fallbackTime,
      updated_at: previous?.updated_at ?? full.updated_at ?? fallbackTime,
    };
  }

  function getDeploymentBadgeProps(status: string) {
    const s = status.toLowerCase();
    if (s === "running") {
      return {
        variant: "outline" as const,
        className:
          "border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]",
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
        variant: "outline" as const,
        className:
          "border-transparent bg-[color-mix(in_srgb,var(--status-warning)_12%,transparent)] text-[var(--status-warning)]",
      };
    }
    if (s === "failed" || s === "cancelled") {
      return {
        variant: "destructive" as const,
        className: "",
      };
    }
    return {
      variant: "outline" as const,
      className: "",
    };
  }

  function getDeploymentButtonText(
    dep: DeploymentInfo,
    isCurrentlyInProd: boolean,
  ) {
    if (isCurrentlyInProd) return "Currently in Prod";
    if (dep.status === "DRAINING") return "Draining...";
    if (dep.status === "BUILDING") return "Building...";
    if (dep.status === "STARTING" || dep.status === "SCHEDULED")
      return "Starting...";
    return "Promote to Prod";
  }

  function copy(text: string) {
    if (!text) return;
    navigator.clipboard
      .writeText(text)
      .then(() => toast.success("Copied to clipboard!"))
      .catch(() => toast.error("Failed to copy to clipboard"));
  }

  function handleMetrics(sample: VmMetricsResponse) {
    if (!sample) return;

    const txBytes = sample.tx_bytes || 0;
    const rxBytes = sample.rx_bytes || 0;
    const now = Date.now();
    const key = sample.job_id || sample.vm_id || "default";
    const prev = lastNetwork.get(key);

    let txRate = 0;
    let rxRate = 0;

    if (prev) {
      const deltaTime = (now - prev.time) / 1000;
      if (deltaTime > 0.8) {
        txRate = Math.max(0, txBytes - prev.tx) / deltaTime / 1024;
        rxRate = Math.max(0, rxBytes - prev.rx) / deltaTime / 1024;
        lastNetwork.set(key, {
          tx: txBytes,
          rx: rxBytes,
          time: now,
          txRate,
          rxRate,
        });
      } else {
        txRate = prev.txRate || 0;
        rxRate = prev.rxRate || 0;
      }
    } else {
      lastNetwork.set(key, {
        tx: txBytes,
        rx: rxBytes,
        time: now,
        txRate: 0,
        rxRate: 0,
      });
    }

    replicaSamples.set(key, {
      cpu: normalizeCpuUsage(sample.cpu_usage),
      ram: (sample.ram_used_bytes || 0) / (1024 * 1024),
      rx: rxRate,
      tx: txRate,
      total_rx: rxBytes,
      total_tx: txBytes,
      lastUpdate: now,
    });

    for (const [replicaKey, data] of replicaSamples.entries()) {
      if (now - data.lastUpdate > 15000) {
        replicaSamples.delete(replicaKey);
      }
    }

    const activeReplicas = Array.from(replicaSamples.values());
    if (activeReplicas.length === 0) return;

    const count = activeReplicas.length;
    const aggregated: MetricsSnapshot = {
      time: new Date().toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      }),
      cpu:
        activeReplicas.reduce((sum, replica) => sum + replica.cpu, 0) / count,
      ram: activeReplicas.reduce((sum, replica) => sum + replica.ram, 0),
      rx: activeReplicas.reduce((sum, replica) => sum + replica.rx, 0),
      tx: activeReplicas.reduce((sum, replica) => sum + replica.tx, 0),
      total_rx: activeReplicas.reduce(
        (sum, replica) => sum + replica.total_rx,
        0,
      ),
      total_tx: activeReplicas.reduce(
        (sum, replica) => sum + replica.total_tx,
        0,
      ),
    };

    liveMetrics = sample;
    metricsHistory = [...metricsHistory.slice(-29), aggregated];
  }

  onMount(() => {
    const token = getToken();
    if (!token) return;

    let cleanupDeployments: (() => void) | null = null;
    let cleanupMetrics: (() => void) | null = null;

    const init = async (currentAppName: string) => {
      loading = true;
      liveMetrics = null;
      metricsHistory = [];
      replicaSamples.clear();
      lastNetwork.clear();

      if ($appsStore.length === 0) {
        await refreshApps();
      }

      const [deploymentsResult, secretResult] = await Promise.all([
        listDeployments(token, currentAppName),
        getAppSecret(token, currentAppName),
      ]);

      if (deploymentsResult.data) {
        deployments = sortDeployments(
          deploymentsResult.data.map((dep) => normalizeDeployment(dep)),
        );
      }
      if (secretResult.data) secret = secretResult.data.github_webhook_secret;
      if (deploymentsResult.error) {
        toast.error(deploymentsResult.error || "Failed to load deployments");
      }
      loading = false;

      if (cleanupDeployments) cleanupDeployments();
      if (cleanupMetrics) cleanupMetrics();

      cleanupDeployments = watchDeploymentsSSE(token, (deployment) => {
        if (deployment.app_name !== currentAppName) return;

        const depId =
          "deployment_id" in deployment ? deployment.deployment_id : null;
        const jobId = deployment.job_id;
        const index = deployments.findIndex(
          (dep) =>
            (depId && dep.id === depId) || (jobId && dep.job_id === jobId),
        );

        if (index === -1) {
          deployments = sortDeployments([
            normalizeDeployment(deployment),
            ...deployments,
          ]);
        } else {
          deployments = sortDeployments(
            deployments.map((dep, depIndex) =>
              depIndex === index ? normalizeDeployment(deployment, dep) : dep,
            ),
          );
        }
      });

      cleanupMetrics = watchAppMetrics(token, currentAppName, handleMetrics);
    };

    const unsub = page.subscribe(($page) => {
      const name = decodeURIComponent($page.params.appName ?? "");
      if (name) init(name);
    });

    return () => {
      unsub();
      if (cleanupDeployments) cleanupDeployments();
      if (cleanupMetrics) cleanupMetrics();
    };
  });

  async function handleDeleteApp() {
    const token = getToken();
    if (!token || !app) return;
    deletingApp = true;
    try {
      const result = await deleteApp(token, appName);
      if (result.error) {
        toast.error(result.error);
        return;
      }
      toast.success(`Application ${app.name} deleted`);
      goto("/apps");
    } finally {
      deletingApp = false;
    }
  }

  async function handleActivate(deploymentId: string) {
    const token = getToken();
    if (!token) return;
    activatingDeploymentId = deploymentId;
    try {
      const result = await activateDeployment(token, appName, deploymentId);
      if (result.error) {
        toast.error(result.error);
        return;
      }
      toast.success("Deployment activated successfully");
    } finally {
      activatingDeploymentId = null;
    }
  }

  $: active =
    deployments.length === 0
      ? null
      : appScaleState === "scaled_to_zero"
        ? null
        : (app?.active_deployment_id
            ? deployments.find((dep) => dep.id === app.active_deployment_id)
            : null) ||
          [...deployments]
            .sort(
              (a, b) =>
                new Date(b.created_at).getTime() -
                new Date(a.created_at).getTime(),
            )
            .find((dep) =>
              ["RUNNING", "DRAINING", "HEALTHY", "STARTING"].includes(
                (dep.status || "").toUpperCase(),
              ),
            ) ||
          deployments[0];
  $: inFlight = deployments.find((d) =>
    ["HEALTH_CHECKING", "STARTING", "BUILDING", "SCHEDULED"].includes(d.status),
  );
  $: latestMetrics =
    (metricsHistory.length > 0
      ? metricsHistory[metricsHistory.length - 1]
      : null) ||
    (liveMetrics
      ? {
          time: new Date().toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
          }),
          cpu: normalizeCpuUsage(liveMetrics.cpu_usage),
          ram: (liveMetrics.ram_used_bytes || 0) / (1024 * 1024),
          rx: 0,
          tx: 0,
          total_rx: liveMetrics.rx_bytes || 0,
          total_tx: liveMetrics.tx_bytes || 0,
        }
      : { time: "", cpu: 0, ram: 0, rx: 0, tx: 0, total_rx: 0, total_tx: 0 });
  $: recentDeploymentsList = sortDeployments(deployments).slice(0, 5);
  $: totalTrafficBytes =
    (latestMetrics.total_rx || 0) + (latestMetrics.total_tx || 0);
  $: metricCards = [
    {
      key: "cpu",
      label: "CPU",
      detail: "Usage",
      value: `${(latestMetrics.cpu || 0).toFixed(1)}%`,
      icon: Cpu,
      color: "bg-[var(--chart-1)]",
    },
    {
      key: "ram",
      label: "RAM",
      detail: "Allocated",
      value: `${(latestMetrics.ram || 0).toFixed(0)} MiB`,
      icon: MemoryStick,
      color: "bg-[var(--chart-2)]",
    },
    {
      key: "rx",
      label: "Network in",
      detail: "Receive",
      value: formatNetworkRate(latestMetrics.rx || 0),
      icon: ArrowDownToLine,
      color: "bg-[var(--chart-3)]",
    },
    {
      key: "tx",
      label: "Network out",
      detail: "Transmit",
      value: formatNetworkRate(latestMetrics.tx || 0),
      icon: ArrowUpFromLine,
      color: "bg-[var(--chart-4)]",
    },
  ];
  $: showLivePerformance =
    appScaleState !== "scaled_to_zero" && runningReplicaCount > 0;

  $: if (appScaleState === "scaled_to_zero" || runningReplicaCount === 0) {
    liveMetrics = null;
    metricsHistory = [];
    replicaSamples.clear();
  }
</script>

<DashboardLayout>
  <div class="flex flex-col justify-between gap-4 md:flex-row md:items-center">
    <div class="flex items-center gap-4">
      <div
        class="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground"
      >
        <Boxes />
      </div>
      <div>
        <div class="flex flex-wrap items-center gap-3">
          <h1 class="text-2xl font-semibold tracking-tight">
            {app?.name || appName}.apps.mikrom.spluca.org
          </h1>
          <Button
            variant="outline"
            size="sm"
            href={`https://${app?.name || appName}.apps.mikrom.spluca.org`}
            target="_blank"
            rel="noreferrer"
            class="shrink-0"
          >
            <Globe2 class="size-4" />
            <span class="hidden sm:inline">Visit site</span>
            <ExternalLink class="size-4" />
          </Button>
        </div>
        <p class="mt-1 text-sm text-muted-foreground">
          Manage {app?.name || "application"} deployments and monitor production
          instances.
        </p>
      </div>
    </div>
    <div class="flex items-center gap-2">
      <Button
        size="sm"
        onclick={() => (showDeployModal = true)}
        disabled={!app}
      >
        Deploy Now
      </Button>
      <Button
        size="sm"
        variant="outline"
        onclick={() => (showScaleModal = true)}
      >
        <Scale class="size-4" />
        Scaling
      </Button>
      <Button
        size="sm"
        variant="outline"
        onclick={() => (showWebhookModal = true)}
      >
        <Cog class="size-4" />
        Auto-deploy
      </Button>
      <Button
        size="sm"
        variant="destructive"
        onclick={() => (showDeleteAppDialog = true)}
        disabled={deletingApp}
      >
        {#if deletingApp}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <Trash2 class="size-4" />
        {/if}
        Delete App
      </Button>
    </div>
  </div>

  <section class="space-y-4">
    <div class="flex items-center justify-between">
      <h2 class="text-lg font-bold tracking-tight">Deployment History</h2>
    </div>
    <Card class="overflow-hidden">
      <div class="overflow-x-auto">
        <table class="min-w-[900px] w-full">
          <thead>
            <tr class="border-b border-border text-left text-sm">
              <th class="px-4 py-3">Version</th>
              <th class="px-4 py-3">Status</th>
              <th class="px-4 py-3">Replicas</th>
              <th class="px-4 py-3">Created</th>
              <th class="px-4 py-3">Environment</th>
              <th class="px-4 py-3 text-right">Actions</th>
            </tr>
          </thead>
          <tbody>
            {#if loading && deployments.length === 0}
              {#each Array.from({ length: 3 }) as _}
                <tr class="border-b border-border">
                  <td class="px-4 py-4" colspan="6"
                    ><Skeleton class="h-8 w-full" /></td
                  >
                </tr>
              {/each}
            {:else if deployments.length === 0}
              <tr>
                <td class="py-10" colspan="6">
                  <EmptyState>
                    <Rocket class="size-10 text-muted-foreground" />
                    <h3 class="text-xl font-semibold">No active deployment</h3>
                    <p class="text-sm text-muted-foreground">
                      Deploy your application to see its status here.
                    </p>
                  </EmptyState>
                </td>
              </tr>
            {:else}
              {#each recentDeploymentsList as dep}
                {@const currentApp = app}
                {@const isProduction = active?.id === dep.id}
                {@const isCurrentTarget = inFlight
                  ? inFlight.id === dep.id
                  : isProduction}
                {@const canActivate =
                  ["RUNNING", "PAUSED", "STOPPED", "FAILED"].includes(
                    dep.status,
                  ) && !isProduction}
                {@const deploymentBadge = getDeploymentBadgeProps(dep.status)}
                <tr
                  class="border-b border-border"
                  style={isProduction
                    ? "background-color: color-mix(in srgb, var(--accent) 12%, var(--background));"
                    : undefined}
                >
                  <td class="px-4 py-4">
                    <div class="flex flex-col gap-1">
                      <span
                        class="max-w-[300px] truncate text-sm font-semibold line-clamp-1"
                      >
                        {dep.git_commit_message ||
                          dep.image_tag ||
                          (dep.status === "BUILDING"
                            ? `Deploying ${dep.id.split("-")[0]}...`
                            : `Deployment ${dep.id.split("-")[0]}`)}
                      </span>
                      <div
                        class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
                      >
                        <span
                          class="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5"
                          ><GitBranch class="size-3" />{dep.git_branch ||
                            "main"}</span
                        >
                        <span class="font-mono"
                          >{dep.git_commit_hash?.substring(0, 7) ||
                            dep.id.split("-")[0]}</span
                        >
                        <span class="inline-flex items-center gap-1">
                          {#if dep.trigger_source === "github_webhook"}
                            <Zap
                              class="size-3 fill-status-warning text-status-warning"
                            />
                          {:else}
                            <User class="size-3" />
                          {/if}
                          {dep.trigger_source || "manual"}
                        </span>
                      </div>
                    </div>
                  </td>
                  <td class="px-4 py-4"
                    ><Badge
                      variant={deploymentBadge.variant}
                      class={`font-semibold capitalize ${deploymentBadge.className}`}
                      >{dep.status}</Badge
                    ></td
                  >
                  <td
                    class="px-4 py-4 text-xs font-medium text-muted-foreground"
                  >
                    {#if currentApp && isCurrentTarget}
                      {formatReplicaSummary(currentApp)}
                    {:else}
                      0
                    {/if}
                  </td>
                  <td
                    class="whitespace-nowrap px-4 py-4 text-xs text-muted-foreground"
                    >{formatDeploymentDate(dep.created_at)}</td
                  >
                  <td class="px-4 py-4">
                    {#if isProduction}
                      <div
                        class="flex items-center gap-1.5 text-sm font-semibold text-status-info"
                      >
                        <CheckCircle2 class="size-5" />
                        <span>Production</span>
                      </div>
                    {:else}
                      <span class="text-xs italic text-muted-foreground"
                        >Preview</span
                      >
                    {/if}
                  </td>
                  <td class="px-4 py-4 text-right">
                    <ButtonGroup class="ml-auto">
                      {#if isProduction}
                        <Button size="sm" variant="outline" disabled>
                          Currently in Prod
                        </Button>
                      {:else}
                        <Button
                          size="sm"
                          variant="outline"
                          class="border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)] hover:bg-[color-mix(in_srgb,var(--status-info)_18%,transparent)]"
                          disabled={!canActivate ||
                            activatingDeploymentId !== null}
                          onclick={() => handleActivate(dep.id)}
                        >
                          {getDeploymentButtonText(dep, isProduction)}
                          {#if !["BUILDING", "STARTING", "SCHEDULED", "DRAINING"].includes(dep.status)}
                            <Rocket class="ml-2 size-3" />
                          {/if}
                          {#if dep.status === "BUILDING" || dep.status === "DRAINING" || activatingDeploymentId === dep.id}
                            <Loader2 class="ml-2 size-3 animate-spin" />
                          {/if}
                        </Button>
                      {/if}
                    </ButtonGroup>
                  </td>
                </tr>
              {/each}
            {/if}
          </tbody>
        </table>
      </div>
    </Card>
  </section>

  {#if showLivePerformance}
    <div
      class="space-y-6 animate-in fade-in duration-500 border-t border-border pt-6"
    >
      <h2 class="text-lg font-bold tracking-tight">Live Performance</h2>

      {#if !liveMetrics}
        <Card class="p-12">
          <div class="flex flex-col gap-4">
            <Skeleton class="h-6 w-44" />
            <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
              {#each Array.from({ length: 4 }) as _}
                <div class="rounded-lg border bg-background/80 p-3 shadow-sm">
                  <Skeleton class="h-4 w-24" />
                  <div class="mt-3 flex flex-col gap-2">
                    <Skeleton class="h-6 w-20" />
                    <Skeleton class="h-3 w-28" />
                  </div>
                </div>
              {/each}
            </div>
            <Skeleton class="h-48 w-full" />
          </div>
        </Card>
      {:else}
        <Card class="overflow-hidden">
          <CardHeader class="border-b bg-muted/20">
            <div
              class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between"
            >
              <div class="flex flex-col gap-1.5">
                <CardTitle>System Performance</CardTitle>
                <CardDescription
                  >Live CPU, RAM and network throughput. Total traffic: {formatBytes(
                    totalTrafficBytes,
                  )}.</CardDescription
                >
              </div>
              <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                {#each metricCards as metric}
                  <div
                    class="min-w-36 rounded-lg border bg-background/80 p-3 shadow-sm"
                  >
                    <div
                      class="flex items-center gap-2 text-xs font-medium text-muted-foreground"
                    >
                      <span class={`size-2 rounded-full ${metric.color}`}
                      ></span>
                      <svelte:component this={metric.icon} class="size-4" />
                      {metric.label}
                    </div>
                    <div class="mt-2 flex flex-col gap-1">
                      <span class="text-xl font-semibold tabular-nums"
                        >{metric.value}</span
                      >
                      <span class="text-xs text-muted-foreground"
                        >{metric.detail}</span
                      >
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          </CardHeader>
          <CardContent class="p-0">
            <MetricChart points={metricsHistory} />
          </CardContent>
        </Card>
      {/if}
    </div>
  {/if}

  {#if showWebhookModal}
    <Modal
      open={showWebhookModal}
      title="GitHub Auto-deploy Configuration"
      width="max-w-[600px]"
      onclose={() => (showWebhookModal = false)}
    >
      <div class="space-y-6 pt-4">
        <div class="flex items-start gap-3">
          <Info class="mt-0.5 size-6 shrink-0 text-indigo-500" />
          <div>
            <p class="text-sm text-muted-foreground">
              Set up a webhook in your GitHub repository to enable automatic
              deployments on every push to the <code
                class="rounded bg-muted px-1 text-foreground">main</code
              >
              or
              <code class="rounded bg-muted px-1 text-foreground">master</code> branch.
            </p>
          </div>
        </div>

        <div class="space-y-4 pt-2">
          <div class="space-y-1.5">
            <p
              class="text-[10px] font-bold uppercase tracking-wider text-muted-foreground"
            >
              Payload URL
            </p>
            <div class="flex items-center gap-2">
              <Input
                class="flex-1 font-mono text-xs"
                readonly
                value={`${webhookBaseUrl}/webhooks/github/${appName}`}
              />
              <Button
                variant="outline"
                size="sm"
                class="h-9 px-3"
                onclick={() =>
                  copy(`${webhookBaseUrl}/webhooks/github/${appName}`)}
              >
                <Clipboard class="size-4" />
              </Button>
            </div>
          </div>

          <div class="space-y-1.5">
            <p
              class="text-[10px] font-bold uppercase tracking-wider text-muted-foreground"
            >
              Secret
            </p>
            <div class="flex items-center gap-2">
              <div class="relative flex-1">
                <Input
                  class="w-full pr-10 font-mono text-xs"
                  readonly
                  type={showSecret ? "text" : "password"}
                  value={secret || ""}
                />
                <button
                  class="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  on:click={() => (showSecret = !showSecret)}
                >
                  {#if showSecret}<EyeOff class="size-4" />{:else}<Eye
                      class="size-4"
                    />{/if}
                </button>
              </div>
              <Button
                variant="outline"
                size="sm"
                class="h-9 px-3"
                onclick={() => copy(secret || "")}
              >
                <Clipboard class="size-4" />
              </Button>
            </div>
          </div>
        </div>

        <div class="rounded-lg border border-border bg-muted/50 p-4">
          <h4 class="mb-2 text-xs font-bold">Instructions:</h4>
          <ol
            class="list-inside list-decimal space-y-2 text-xs text-muted-foreground"
          >
            <li>Go to your repository on GitHub.</li>
            <li>
              Click on <span class="font-medium text-foreground">Settings</span>
              &gt; <span class="font-medium text-foreground">Webhooks</span>.
            </li>
            <li>
              Click <span class="font-medium text-foreground">Add webhook</span
              >.
            </li>
            <li>
              Paste the <span class="font-medium text-foreground"
                >Payload URL</span
              >
              and <span class="font-medium text-foreground">Secret</span> above.
            </li>
            <li>
              Set <span class="font-medium text-foreground">Content type</span>
              to <span class="font-mono">application/json</span>.
            </li>
            <li>
              Click <span class="font-medium text-foreground">Add webhook</span>
              at the bottom.
            </li>
          </ol>
        </div>
      </div>
    </Modal>
  {/if}

  {#if app && showScaleModal}
    <ScaleAppModal bind:open={showScaleModal} app={app!} />
  {/if}

  {#if app && showDeployModal}
    <DeployAppModal bind:open={showDeployModal} app={app!} />
  {/if}

  <AlertDialog
    open={showDeleteAppDialog}
    title="Delete application?"
    description={`This will permanently delete ${app?.name || appName} and all of its deployments, volumes and security rules.`}
    confirmLabel="Delete App"
    onclose={() => (showDeleteAppDialog = false)}
    onconfirm={async () => {
      showDeleteAppDialog = false;
      await handleDeleteApp();
    }}
  />
</DashboardLayout>
