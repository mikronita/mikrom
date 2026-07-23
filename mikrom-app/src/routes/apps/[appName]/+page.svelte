<script lang="ts">
  import { onMount } from "svelte";
  import { browser } from "$app/environment";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
    import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowDownToLine from "@lucide/svelte/icons/arrow-down-to-line";
  import ArrowUpFromLine from "@lucide/svelte/icons/arrow-up-from-line";
  import Boxes from "@lucide/svelte/icons/boxes";
  import Cpu from "@lucide/svelte/icons/cpu";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import Loader2 from "@lucide/svelte/icons/loader-2";
  import MemoryStick from "@lucide/svelte/icons/memory-stick";
  import Rocket from "@lucide/svelte/icons/rocket";
  import User from "@lucide/svelte/icons/user";
  import Zap from "@lucide/svelte/icons/zap";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Clipboard from "@lucide/svelte/icons/clipboard";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Cog from "@lucide/svelte/icons/cog";
  import CheckCircle2 from "@lucide/svelte/icons/check-circle-2";
  import Info from "@lucide/svelte/icons/info";
  import Scale from "@lucide/svelte/icons/scale";
  import Terminal from "@lucide/svelte/icons/terminal";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { SvelteMap } from "svelte/reactivity";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Badge,
    Button,
    Field,
    AlertDialog,
    EmptyState,
    Modal,
    Input,
    Skeleton,
    SectionTabs,
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
    updateApp,
    type AppInfo,
    type DeploymentInfo,
    type VmMetricsResponse,
    type LogLine,
    watchAppMetrics,
    watchAppLogsSSE,
    watchDeploymentsSSE,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { vmsStore, vmsLoading } from "$lib/stores/vms";
  import { formatDate } from "$lib/utils";
  import { Separator } from "$lib/components";
  import {
    aggregateReplicaMetrics,
    buildMetricSnapshot,
    formatBytes,
    formatDeploymentDate,
    formatNetworkRate,
    getDeploymentBadgeProps,
    getDeploymentButtonText,
    normalizeCpuUsage,
    normalizeDeployment,
    sortDeployments,
    type MetricsSnapshot,
  } from "$lib/domain/app-details";

  const webhookBaseUrl = browser
    ? `${window.location.protocol}//${window.location.hostname}:5001/v1`
    : "http://localhost:5001/v1";

  let deployments = $state<DeploymentInfo[]>([]);
  let loading = $state(true);
  let liveMetrics = $state<VmMetricsResponse | null>(null);
  let metricsHistory = $state<MetricsSnapshot[]>([]);
  let liveLogs = $state<LogLine[]>([]);
  let _logsLoading = $state(true);
  let secret = $state<string | null>(null);
  let showSecret = $state(false);
  let showWebhookModal = $state(false);
  let showScaleModal = $state(false);
  let showDeployModal = $state(false);
  let showPortModal = $state(false);
  let showLogsModal = $state(false);

  async function refreshDeployments() {
    const token = getToken();
    if (!token) return;
    const name = decodeURIComponent($page.params.appName ?? "");
    if (!name) return;
    const result = await listDeployments(token, name);
    if (result.data) {
      deployments = sortDeployments(
        result.data.map((dep) => normalizeDeployment(dep)),
      );
    }
  }
  let showDeleteAppDialog = $state(false);
  let deletingApp = $state(false);
  let activatingDeploymentId = $state<string | null>(null);
  let selectedPort = $state("8080");
  let activeTab = $state<"overview" | "deployments" | "performance" | "settings">("overview");
  const appTabs = [
    { value: "overview", label: "Overview" },
    { value: "deployments", label: "Deployments" },
    { value: "performance", label: "Performance" },
    { value: "settings", label: "Settings" },
  ] as const;

  let appName = $derived(decodeURIComponent($page.params.appName ?? ""));
  let app = $derived($appsStore.find((item) => item.name === appName) ?? null);
  let appScaleState = $derived(app?.scale_state ?? "scaled_to_zero");
  let scaleStateBadge = $derived.by(() => {
    switch (appScaleState as string) {
      case "active":
        return { label: "Active", color: "border-emerald-500/30 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400" };
      case "scaled_to_zero":
        return { label: "Scaled to Zero (Auto-Idle)", color: "border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400" };
      default:
        return { label: String(appScaleState), color: "border-border bg-muted text-muted-foreground" };
    }
  });

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
  let runningReplicaCount = $derived(
    app
    ? $vmsStore.filter(
        (vm) =>
          vm.status.toLowerCase() === "running" &&
          (vm.app_id === app.id || vm.app_name === app.name),
      ).length
    : 0,
  );

  function formatReplicaSummary(appInfo: AppInfo) {
    if (appInfo.autoscaling_enabled) {
      if ($vmsLoading && runningReplicaCount === 0)
        return `--/${appInfo.max_replicas}`;
      return `${runningReplicaCount}/${appInfo.max_replicas}`;
    }

    if ($vmsLoading && runningReplicaCount === 0) return "--";
    return `${runningReplicaCount}`;
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
    const now = Date.now();
    const key = sample.job_id || sample.vm_id || "default";
    const prev = lastNetwork.get(key);

    const { cache, sample: replicaSample } = buildMetricSnapshot(sample, now, prev);
    lastNetwork.set(key, cache);
    replicaSamples.set(key, replicaSample);

    for (const [replicaKey, data] of replicaSamples.entries()) {
      if (now - data.lastUpdate > 15000) {
        replicaSamples.delete(replicaKey);
      }
    }

    const activeReplicas = Array.from(replicaSamples.values());
    if (activeReplicas.length === 0) return;

    liveMetrics = sample;
    metricsHistory = [...metricsHistory.slice(-29), aggregateReplicaMetrics(activeReplicas)];
  }

  onMount(() => {
    const token = getToken();
    if (!token) return;

    let cleanupDeployments: (() => void) | null = null;
    let cleanupMetrics: (() => void) | null = null;
    let cleanupLogs: (() => void) | null = null;

    const init = async (currentAppName: string) => {
      loading = true;
      liveMetrics = null;
      metricsHistory = [];
      liveLogs = [];
      _logsLoading = true;
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
      if (cleanupLogs) cleanupLogs();

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
      cleanupLogs = watchAppLogsSSE(token, currentAppName, (payload) => {
        const lines = Array.isArray(payload) ? payload : [payload];
        const targetDeploymentId = activeDeploymentId;
        const filtered = targetDeploymentId
          ? lines.filter((line) => !line.deployment_id || line.deployment_id === targetDeploymentId)
          : lines;
        if (filtered.length > 0) {
          liveLogs = [...liveLogs, ...filtered].slice(-500);
        }
        _logsLoading = false;
      });
    };

    const unsub = page.subscribe(($page) => {
      const name = decodeURIComponent($page.params.appName ?? "");
      if (name) init(name);
    });

    return () => {
      unsub();
      if (cleanupDeployments) cleanupDeployments();
      if (cleanupMetrics) cleanupMetrics();
      if (cleanupLogs) cleanupLogs();
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
      await refreshApps();
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

  function openPortModal() {
    selectedPort = String(app?.port ?? 8080);
    showPortModal = true;
  }

  function refreshCurrentSection() {
    if (!app) return;

    if (activeTab === "overview") {
      void refreshApps();
      return;
    }

    if (activeTab === "deployments") {
      void refreshDeployments();
      void refreshApps();
      return;
    }

    if (activeTab === "performance") {
      liveMetrics = null;
      metricsHistory = [];
      return;
    }

    if (activeTab === "settings") {
      void refreshApps();
    }
  }

  async function handleUpdatePort(event: SubmitEvent) {
    event.preventDefault();
    const token = getToken();
    if (!token || !app) return;

    const nextPort = Number(selectedPort);
    if (!Number.isInteger(nextPort) || nextPort < 1 || nextPort > 65535) {
      toast.error("Enter a valid port between 1 and 65535");
      return;
    }

    const result = await updateApp(token, appName, { port: nextPort });
    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success(`Port updated to ${nextPort}`);
    await refreshApps();
    showPortModal = false;
  }

  let active: DeploymentInfo | null = $derived(
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
          deployments[0],
  );
  let activeDeploymentId = $derived(active?.id ?? null);
  let inFlight: DeploymentInfo | undefined = $derived(
    deployments.find((d) =>
      ["HEALTH_CHECKING", "STARTING", "BUILDING", "SCHEDULED"].includes(d.status),
    ),
  );
  let latestMetrics = $derived(
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
      : { time: "", cpu: 0, ram: 0, rx: 0, tx: 0, total_rx: 0, total_tx: 0 }),
  );
  let recentDeploymentsList = $derived(sortDeployments(deployments).slice(0, 5));
  let totalTrafficBytes = $derived((latestMetrics.total_rx || 0) + (latestMetrics.total_tx || 0));
  let metricCards = $derived([
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
  ]);
  let showLivePerformance = $derived(appScaleState !== "scaled_to_zero" && runningReplicaCount > 0);
  let statusBadgeLabel = $derived(
    appScaleState === "scaled_to_zero"
      ? "Scaled to zero"
      : runningReplicaCount > 0
        ? "Running"
        : "Idle",
  );
  let replicaSummary = $derived(app ? formatReplicaSummary(app) : "--");
  let appUpdatedAt = $derived(
    app?.updated_at || app?.created_at ? formatDate(app?.updated_at || app?.created_at || new Date()) : null,
  );

  function onDeploy() {
    activeTab = "deployments";
    refreshDeployments();
  }

  $effect(() => {
    if (appScaleState === "scaled_to_zero" || runningReplicaCount === 0) {
      liveMetrics = null;
      metricsHistory = [];
      replicaSamples.clear();
    }
  });

  let logsContainer = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!logsContainer || liveLogs.length === 0) return;
    logsContainer.scrollTop = logsContainer.scrollHeight;
  });
</script>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="rounded-2xl border border-border bg-card p-5 shadow-sm md:p-6">
      <div class="flex flex-col gap-6 xl:flex-row xl:items-start xl:justify-between">
        <div class="flex min-w-0 flex-1 gap-4">
          <div
            class="flex size-12 shrink-0 items-center justify-center rounded-xl border border-border bg-background text-foreground"
          >
            <Boxes />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-3">
              <h1 class="truncate text-3xl font-semibold tracking-tight">
                {app?.name || appName}
              </h1>
              <Badge variant="secondary" class="uppercase">
                {statusBadgeLabel}
              </Badge>
              <span class="inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-semibold tracking-wide transition-colors {scaleStateBadge.color}">
                {scaleStateBadge.label}
              </span>
              {#if app?.hostname}
                <Badge variant="outline" class="truncate">
                  {app.hostname}
                </Badge>
              {/if}
            </div>
            <p class="mt-2 max-w-3xl text-sm text-muted-foreground">
              Manage {app?.name || "application"} deployments and monitor production
              instances from a single place.
            </p>
            <div class="mt-4 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
              <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-3 py-1.5">
                <span class="font-mono">{app?.git_url || "No repository linked"}</span>
              </span>
              <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-3 py-1.5">
                <span>Updated {appUpdatedAt || "--"}</span>
              </span>
            </div>
          </div>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            href="/apps"
          >
            <ArrowLeft class="size-4" />
            Back
          </Button>
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
        </div>
      </div>

      <Separator class="my-6" />

      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
          <CardContent class="flex flex-col gap-1">
            <span class="text-xs text-muted-foreground">Current deployment</span>
            <span class="text-2xl font-semibold">{active?.status || "No active deployment"}</span>
            <span class="text-xs text-muted-foreground">Prod or preview target</span>
          </CardContent>
        </Card>
        <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
          <CardContent class="flex flex-col gap-1">
            <span class="text-xs text-muted-foreground">Replicas</span>
            <span class="text-2xl font-semibold">{replicaSummary}</span>
            <span class="text-xs text-muted-foreground">Running microVMs</span>
          </CardContent>
        </Card>
        <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
          <CardContent class="flex flex-col gap-1">
            <span class="text-xs text-muted-foreground">Repository</span>
            <span class="truncate text-xl font-semibold">{app?.git_url || "No repository"}</span>
            <span class="text-xs text-muted-foreground">Source of the current build</span>
          </CardContent>
        </Card>
        <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
          <CardContent class="flex flex-col gap-1">
            <span class="text-xs text-muted-foreground">Updated</span>
            <span class="text-2xl font-semibold">{appUpdatedAt || "--"}</span>
            <span class="text-xs text-muted-foreground">Latest app metadata change</span>
          </CardContent>
        </Card>
      </div>
    </div>

    <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
      <SectionTabs bind:active={activeTab} tabs={appTabs} />
      <Button variant="outline" size="sm" onclick={refreshCurrentSection} disabled={!app}>
        <RefreshCw class="size-4" />
        Refresh section
      </Button>
    </div>

    {#if activeTab === "overview"}
      <div class="grid gap-6 lg:grid-cols-[1fr_360px]">
        <Card>
          <CardHeader>
            <div class="flex items-center gap-2">
              <Boxes class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Application Snapshot</CardTitle>
            </div>
            <CardDescription>Core status and the current production target for this app.</CardDescription>
          </CardHeader>
          <CardContent class="grid gap-4 sm:grid-cols-2">
            <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
              <span class="text-xs font-medium text-muted-foreground">Current deployment</span>
              <span class="text-lg font-semibold">{active?.status || "No active deployment"}</span>
              <span class="text-xs text-muted-foreground">Production target or preview build</span>
            </div>
            <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
              <span class="text-xs font-medium text-muted-foreground">Replicas</span>
              <span class="text-lg font-semibold">{replicaSummary}</span>
              <span class="text-xs text-muted-foreground">Running microVMs</span>
            </div>
            <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
              <span class="text-xs font-medium text-muted-foreground">Repository</span>
              <span class="truncate font-mono text-sm">{app?.git_url || "No repository linked"}</span>
              <span class="text-xs text-muted-foreground">Source of the current build</span>
            </div>
            <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
              <span class="text-xs font-medium text-muted-foreground">Updated</span>
              <span class="text-lg font-semibold">{appUpdatedAt || "--"}</span>
              <span class="text-xs text-muted-foreground">Latest metadata change</span>
            </div>
          </CardContent>
        </Card>

        <Card class="border-border/70 bg-muted/20">
          <CardHeader>
            <CardTitle class="text-base">Quick actions</CardTitle>
            <CardDescription>Open the views that change app state the most often.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <Button class="w-full" onclick={() => (showDeployModal = true)} disabled={!app}>
              <Rocket class="size-4" />
              Deploy Now
            </Button>
            <Button variant="outline" class="w-full" onclick={() => (showScaleModal = true)}>
              <Scale class="size-4" />
              Scaling
            </Button>
            <Button variant="outline" class="w-full" onclick={() => (showWebhookModal = true)}>
              <Cog class="size-4" />
              Auto-deploy
            </Button>
            {#if active?.job_id}
              <Button
                variant="outline"
                class="w-full"
                onclick={() => (showLogsModal = true)}
              >
                <Terminal class="size-4" />
                View active logs
              </Button>
            {/if}
          </CardContent>
        </Card>
      </div>
    {:else if activeTab === "deployments"}
      <section class="flex flex-col gap-4">
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
                      <td class="px-4 py-4" colspan="6"><Skeleton class="h-8 w-full" /></td>
                    </tr>
                  {/each}
                {:else if deployments.length === 0}
                  <tr>
                    <td class="py-10" colspan="6">
                      <EmptyState>
                        <Rocket class="size-10 text-muted-foreground" />
                        <h3 class="text-xl font-semibold">No deployments yet</h3>
                        <p class="text-sm text-muted-foreground">
                          Deploy your application to see the deployment history and runtime state here.
                        </p>
                        <div class="flex flex-wrap justify-center gap-2">
                          <Button size="sm" onclick={() => (showDeployModal = true)} disabled={!app}>
                            Deploy Now
                          </Button>
                          <Button variant="outline" size="sm" onclick={() => (activeTab = "settings")}>Open settings</Button>
                        </div>
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
                      ["RUNNING", "PAUSED", "STOPPED", "FAILED"].includes(dep.status) &&
                      !isProduction}
                    {@const deploymentBadge = getDeploymentBadgeProps(dep.status)}
                    <tr
                      class="border-b border-border"
                      style={isProduction
                        ? "background-color: color-mix(in srgb, var(--accent) 12%, var(--background));"
                        : undefined}
                    >
                      <td class="px-4 py-4">
                        <div class="flex flex-col gap-1">
                          <span class="max-w-[300px] truncate text-sm font-semibold line-clamp-1">
                            {dep.git_commit_message ||
                              dep.image_tag ||
                              (dep.status === "BUILDING"
                                ? `Deploying ${dep.id.split("-")[0]}...`
                                : `Deployment ${dep.id.split("-")[0]}`)}
                          </span>
                          <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                            <span class="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5">
                              <GitBranch class="size-3" />{dep.git_branch || "main"}
                            </span>
                            <span class="font-mono">{dep.git_commit_hash?.substring(0, 7) || dep.id.split("-")[0]}</span>
                            <span class="inline-flex items-center gap-1">
                              {#if dep.trigger_source === "github_webhook"}
                                <Zap class="size-3 fill-status-warning text-status-warning" />
                              {:else}
                                <User class="size-3" />
                              {/if}
                              {dep.trigger_source || "manual"}
                            </span>
                          </div>
                        </div>
                      </td>
                      <td class="px-4 py-4">
                        <Badge
                          variant={deploymentBadge.variant}
                          class={`font-semibold capitalize ${deploymentBadge.className}`}
                        >
                          {dep.status}
                        </Badge>
                      </td>
                      <td class="px-4 py-4 text-xs font-medium text-muted-foreground">
                        {#if currentApp && isCurrentTarget}
                          {formatReplicaSummary(currentApp)}
                        {:else}
                          0
                        {/if}
                      </td>
                      <td class="whitespace-nowrap px-4 py-4 text-xs text-muted-foreground">
                        {formatDeploymentDate(dep.created_at)}
                      </td>
                      <td class="px-4 py-4">
                        {#if isProduction}
                          <div class="flex items-center gap-1.5 text-sm font-semibold text-status-info">
                            <CheckCircle2 class="size-5" />
                            <span>Production</span>
                          </div>
                        {:else}
                          <span class="text-xs italic text-muted-foreground">Preview</span>
                        {/if}
                      </td>
                      <td class="px-4 py-4 text-right">
                        <div class="ml-auto flex justify-end gap-2">
                          {#if dep.job_id}
                            <Button
                              size="sm"
                              variant="outline"
                              href={`/apps/${encodeURIComponent(appName)}/deployments/${encodeURIComponent(dep.job_id)}/logs`}
                            >
                              View logs
                            </Button>
                          {/if}
                          {#if isProduction}
                            <Button size="sm" variant="outline" disabled>
                              Currently in Prod
                            </Button>
                          {:else}
                            <Button
                              size="sm"
                              variant="outline"
                              class="border-transparent bg-status-info/10 text-status-info hover:bg-status-info/20"
                              disabled={!canActivate || activatingDeploymentId !== null}
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
                        </div>
                      </td>
                    </tr>
                  {/each}
                {/if}
              </tbody>
            </table>
          </div>
        </Card>
      </section>
    {:else if activeTab === "performance"}
      <section class="flex flex-col gap-4">
        <div class="flex items-center justify-between">
          <h2 class="text-lg font-bold tracking-tight">Live Performance</h2>
        </div>
        {#if showLivePerformance}
          {#if !liveMetrics}
            <Card class="p-12">
              <div class="flex flex-col gap-4">
                <Skeleton class="h-6 w-44" />
                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  {#each Array.from({ length: 4 }) as _}
                    <div class="rounded-2xl border border-border/70 bg-background/80 p-4 shadow-xs">
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
                <div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                  <div class="flex flex-col gap-1.5">
                    <CardTitle>System Performance</CardTitle>
                    <CardDescription>
                      Live CPU, RAM and network throughput. Total traffic: {formatBytes(totalTrafficBytes)}.
                    </CardDescription>
                  </div>
                  <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                    {#each metricCards as metric}
                      {@const MetricIcon = metric.icon}
                      <div class="min-w-36 rounded-2xl border border-border/70 bg-background/80 p-4 shadow-xs">
                        <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                          <span class={`size-2 rounded-full ${metric.color}`}></span>
                          <MetricIcon class="size-4" />
                          {metric.label}
                        </div>
                        <div class="mt-2 flex flex-col gap-1">
                          <span class="text-xl font-semibold tabular-nums">{metric.value}</span>
                          <span class="text-xs text-muted-foreground">{metric.detail}</span>
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
        {:else}
          <Card class="border-border/70 bg-muted/20">
            <CardContent class="flex flex-col gap-3 p-8">
              <h3 class="text-base font-semibold">No live performance available</h3>
              <p class="text-sm text-muted-foreground">
                Performance data appears once the application is running and at least one replica is active.
              </p>
            </CardContent>
          </Card>
        {/if}
      </section>
    {:else if activeTab === "settings"}
      <div class="grid gap-6 lg:grid-cols-3">
        <Card class="border-border/70 bg-muted/20">
          <CardHeader>
            <div class="flex items-center gap-2">
              <Cog class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Auto-deploy</CardTitle>
            </div>
            <CardDescription>Configure the GitHub webhook that keeps this app in sync.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <p class="text-sm text-muted-foreground">
              Use the webhook secret and payload URL to enable automatic deployments on pushes to your main branch.
            </p>
            <Button variant="outline" class="w-full" onclick={() => (showWebhookModal = true)}>
              Open webhook configuration
            </Button>
          </CardContent>
        </Card>

        <Card class="border-border/70 bg-muted/20">
          <CardHeader>
            <div class="flex items-center gap-2">
              <Scale class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Scaling</CardTitle>
            </div>
            <CardDescription>Adjust replica count and autoscaling for this application.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <p class="text-sm text-muted-foreground">
              The current scale state is <span class="font-medium text-foreground">{appScaleState.replaceAll("_", " ")}</span>.
            </p>
            <Button variant="outline" class="w-full" onclick={() => (showScaleModal = true)}>
              Open scaling controls
            </Button>
          </CardContent>
        </Card>

        <Card class="border-border/70 bg-muted/20">
          <CardHeader>
            <div class="flex items-center gap-2">
              <Cog class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Runtime Port</CardTitle>
            </div>
            <CardDescription>Update the port that the router should use for this app.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <p class="text-sm text-muted-foreground">
              Current port is <span class="font-medium text-foreground">{app?.port ?? "--"}</span>.
            </p>
            <Button variant="outline" class="w-full" onclick={openPortModal} disabled={!app}>
              Change runtime port
            </Button>
          </CardContent>
        </Card>

        <Card class="border-destructive/20 bg-destructive/5 lg:col-span-3">
          <CardHeader>
            <div class="flex items-center gap-2 text-destructive">
              <Trash2 class="size-4" />
              <CardTitle class="text-base">Danger Zone</CardTitle>
            </div>
            <CardDescription>Remove this application and all of its deployments.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <p class="text-sm text-muted-foreground">
              Deleting the app removes its deployments, volumes and security rules from the active project.
            </p>
            <Button
              variant="destructive"
              class="w-full"
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
          </CardContent>
        </Card>
      </div>
    {/if}

    {#if showLogsModal}
      <Modal
        bind:open={showLogsModal}
        title="Live deployment logs"
        description={active ? `Streaming logs for ${active.job_id || active.id}` : "Streaming logs for the selected deployment"}
        width="max-w-5xl"
      >
        {@const modalSelectedDeployment = active}
        <div class="flex flex-col gap-4">
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div class="text-sm text-muted-foreground">
              {#if modalSelectedDeployment}
                Showing {modalSelectedDeployment.id} · {modalSelectedDeployment.status} · {formatDeploymentDate(modalSelectedDeployment.created_at)}
              {/if}
            </div>
            {#if modalSelectedDeployment?.job_id}
              <Button
                variant="outline"
                size="sm"
                href={`/apps/${encodeURIComponent(appName)}/deployments/${encodeURIComponent(modalSelectedDeployment.job_id)}/logs`}
              >
                Open full page
              </Button>
            {/if}
          </div>
          <div class="rounded-xl border border-border bg-[#0b1020] p-4 font-mono text-xs leading-5 text-slate-100">
            {#if liveLogs.length === 0}
              <div class="text-slate-400">Waiting for log stream...</div>
            {:else}
              <div class="max-h-[30rem] overflow-auto" bind:this={logsContainer}>
                {#each liveLogs as log}
                  <div class="flex gap-3 border-b border-white/5 py-1 last:border-b-0">
                    <span class="shrink-0 text-slate-400">{new Date(log.timestamp).toLocaleTimeString()}</span>
                    <span class="min-w-0 flex-1 whitespace-pre-wrap break-words">{log.line}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </Modal>
    {/if}
  </div>

  {#if showWebhookModal}
    <Modal
      open={showWebhookModal}
      title="GitHub Auto-deploy Configuration"
      width="max-w-[600px]"
      onclose={() => (showWebhookModal = false)}
    >
      <div class="flex flex-col gap-6 pt-4">
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

        <div class="flex flex-col gap-4 pt-2">
          <div class="flex flex-col gap-1.5">
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

          <div class="flex flex-col gap-1.5">
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
                  onclick={() => (showSecret = !showSecret)}
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

        <Card size="sm" class="border-border/70 bg-muted/50 shadow-none">
          <CardContent class="flex flex-col gap-2">
            <h4 class="text-xs font-bold uppercase tracking-wide">Instructions</h4>
            <ol class="flex list-inside list-decimal flex-col gap-2 text-xs text-muted-foreground">
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
          </CardContent>
        </Card>
      </div>
    </Modal>
  {/if}

  {#if showPortModal && app}
    <Modal
      open={showPortModal}
      title={`Change ${app.name} port`}
      description="Updates the app and active deployment port so the router can reach the container."
      width="max-w-[480px]"
      onclose={() => (showPortModal = false)}
    >
      <form class="flex flex-col gap-6 pt-4" onsubmit={handleUpdatePort}>
        <Field
          label="Container Port"
          forId="app_runtime_port"
          description="Use the port that the process inside the microVM is actually listening on."
        >
          <Input
            id="app_runtime_port"
            bind:value={selectedPort}
            type="number"
            min="1"
            max="65535"
            step="1"
            inputmode="numeric"
            placeholder="3000"
          />
        </Field>

        <div class="flex justify-end gap-3 pt-2">
          <Button variant="outline" type="button" onclick={() => (showPortModal = false)}>Cancel</Button>
          <Button type="submit">Update port</Button>
        </div>
      </form>
    </Modal>
  {/if}

  {#if app && showScaleModal}
    <ScaleAppModal bind:open={showScaleModal} app={app!} />
  {/if}

  {#if app && showDeployModal}
    <DeployAppModal bind:open={showDeployModal} app={app!} ondeploy={onDeploy} />
  {/if}

  <AlertDialog
    open={showDeleteAppDialog}
    title="Delete application?"
    description={`This will permanently delete ${app?.name || appName} and all of its deployments, volumes and security rules.`}
    confirmLabel="Delete App"
    variant="destructive"
    onclose={() => (showDeleteAppDialog = false)}
    onconfirm={async () => {
      showDeleteAppDialog = false;
      await handleDeleteApp();
    }}
  />
</DashboardLayout>
