<script lang="ts">
  import { onMount } from "svelte";
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
    Network,
  } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import CardHeader from "$lib/components/CardHeader.svelte";
  import CardTitle from "$lib/components/CardTitle.svelte";
  import CardDescription from "$lib/components/CardDescription.svelte";
  import CardContent from "$lib/components/CardContent.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Button from "$lib/components/Button.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Input from "$lib/components/Input.svelte";
  import MetricChart from "$lib/components/MetricChart.svelte";
  import { getToken } from "$lib/auth";
  import {
    API_BASE_URL,
    activateDeployment,
    deleteApp,
    deployAppVersion,
    getAppSecret,
    listDeployments,
    type DeploymentInfo,
    type VmMetricsResponse,
    watchAppMetrics,
    watchDeploymentsSSE,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";

  let deployments: DeploymentInfo[] = [];
  let loading = true;
  let error = "";
  let liveMetrics: VmMetricsResponse | null = null;
  let metricsHistory: Array<{ time: string; cpu: number; ram: number; rx: number; tx: number; total_rx: number; total_tx: number }> = [];
  let secret: string | null = null;
  let showSecret = false;
  let showWebhookModal = false;
  let confirmDeleteApp = false;
  let deployingApp = false;
  let deletingApp = false;
  let activatingDeploymentId: string | null = null;

  $: appName = decodeURIComponent($page.params.appName ?? "");
  $: app = $appsStore.find((item) => item.name === appName);

  type MetricsSnapshot = {
    time: string;
    cpu: number;
    ram: number;
    rx: number;
    tx: number;
    total_rx: number;
    total_tx: number;
  };

  function normalizeCpuUsage(cpuUsage?: number) {
    const value = cpuUsage || 0;
    return value <= 1 ? value * 100 : value;
  }

  function formatNetworkRate(kibPerSecond: number) {
    if (!kibPerSecond || kibPerSecond <= 0) return "0 KiB/s";
    if (kibPerSecond < 0.1) return `${(kibPerSecond * 1024).toFixed(0)} B/s`;
    if (kibPerSecond >= 1024) return `${(kibPerSecond / 1024).toFixed(1)} MiB/s`;
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

  let lastNetwork = new Map<string, { tx: number; rx: number; time: number }>();

  function activeDeployment() {
    if (!deployments.length) return null;
    
    // 1. Try to find the one explicitly marked as active by the app
    const activeById = app?.active_deployment_id ? deployments.find((dep) => dep.id === app.active_deployment_id) : null;
    if (activeById) return activeById;
    
    // 2. Fallback to any running/draining deployment
    const running = [...deployments]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
      .find((dep) => ["RUNNING", "DRAINING", "HEALTHY", "STARTING"].includes((dep.status || "").toUpperCase()));
    if (running) return running;
    
    // 3. Fallback to the most recent deployment
    return [...deployments].sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())[0];
  }

  function productionDepId() {
    return (
      app?.active_deployment_id ||
      [...deployments]
        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
        .find((d) => (d.status || "").toUpperCase() === "RUNNING")?.id
    );
  }

  function getStatusClass(status: string) {
    const s = (status || "").toLowerCase();
    if (s === "running") return "success";
    if (["building", "scheduled", "pending", "paused", "draining"].includes(s)) return "warning";
    if (["failed", "cancelled"].includes(s)) return "destructive";
    return "secondary";
  }

  function copy(text: string) {
    if (!text) return;
    navigator.clipboard.writeText(text).then(() => toast.success("Copied to clipboard!")).catch(() => toast.error("Failed to copy to clipboard"));
  }

  function handleMetrics(sample: VmMetricsResponse) {
    if (!sample) return;
    liveMetrics = sample;
    if (sample.error_message) {
      toast.error(`Termination Error: ${sample.error_message}`);
    }

    const txBytes = sample.tx_bytes || 0;
    const rxBytes = sample.rx_bytes || 0;
    const now = Date.now();
    const key = sample.deployment_id || sample.job_id || sample.vm_id || "current";
    const prev = lastNetwork.get(key);
    let txRate = 0;
    let rxRate = 0;

    if (prev) {
      const deltaTime = (now - prev.time) / 1000;
      if (deltaTime > 0.8) {
        txRate = Math.max(0, txBytes - prev.tx) / deltaTime / 1024;
        rxRate = Math.max(0, rxBytes - prev.rx) / deltaTime / 1024;
        lastNetwork.set(key, { tx: txBytes, rx: rxBytes, time: now });
      } else {
        return;
      }
    } else {
      lastNetwork.set(key, { tx: txBytes, rx: rxBytes, time: now });
    }

    const newPoint = {
      time: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }),
      cpu: normalizeCpuUsage(sample.cpu_usage),
      ram: (sample.ram_used_bytes || 0) / (1024 * 1024),
      rx: rxRate,
      tx: txRate,
      total_rx: rxBytes,
      total_tx: txBytes,
    };

    metricsHistory = [...metricsHistory.slice(-29), newPoint];
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
      lastNetwork.clear();

      if ($appsStore.length === 0) {
        await refreshApps();
      }

      const [deploymentsResult, secretResult] = await Promise.all([
        listDeployments(token, currentAppName),
        getAppSecret(token, currentAppName),
      ]);

      if (deploymentsResult.data) deployments = deploymentsResult.data;
      if (secretResult.data) secret = secretResult.data.github_webhook_secret;
      if (deploymentsResult.error) {
        toast.error(deploymentsResult.error || "Failed to load deployments");
      }
      loading = false;

      if (cleanupDeployments) cleanupDeployments();
      if (cleanupMetrics) cleanupMetrics();

      cleanupDeployments = watchDeploymentsSSE(token, (deployment) => {
        if (deployment.app_name !== currentAppName) return;
        const index = deployments.findIndex((dep) => dep.id === deployment.deployment_id || dep.job_id === deployment.job_id);
        if (index === -1) {
          deployments = [...deployments, { ...(deployment as unknown as DeploymentInfo), id: deployment.deployment_id ?? deployment.vm_id } as DeploymentInfo];
        } else {
          deployments = deployments.map((dep, depIndex) => (depIndex === index ? { 
            ...dep, 
            ...deployment,
            git_commit_hash: deployment.git_commit_hash ?? dep.git_commit_hash,
            git_commit_message: deployment.git_commit_message ?? dep.git_commit_message,
            git_branch: deployment.git_branch ?? dep.git_branch,
          } : dep));
        }
      });

      cleanupMetrics = watchAppMetrics(token, currentAppName, handleMetrics);
    };

    // Use a reactive block for appName changes
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

  async function handleDeployApp() {
    const token = getToken();
    if (!token || !app) return;
    deployingApp = true;
    try {
      const result = await deployAppVersion(token, appName);
      if (result.error) {
        toast.error(result.error);
        return;
      }
      toast.success(`Deployment for ${app.name} initiated`);
    } finally {
      deployingApp = false;
    }
  }

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

  $: active = ((_deps, _app) => {
    if (!deployments || deployments.length === 0) return null;
    const activeById = app?.active_deployment_id ? deployments.find((dep) => dep.id === app.active_deployment_id) : null;
    if (activeById) return activeById;
    return [...deployments]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
      .find((dep) => ["RUNNING", "DRAINING", "HEALTHY", "STARTING"].includes((dep.status || "").toUpperCase())) 
      || deployments[0];
  })(deployments, app);

  $: prodId = ((_deps, _app) => {
    return (
      app?.active_deployment_id ||
      [...deployments]
        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
        .find((d) => (d.status || "").toUpperCase() === "RUNNING")?.id
    );
  })(deployments, app);

  $: latestMetrics = (metricsHistory.length > 0 ? metricsHistory[metricsHistory.length - 1] : null) || (liveMetrics ? {
      time: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }),
      cpu: normalizeCpuUsage(liveMetrics.cpu_usage),
      ram: (liveMetrics.ram_used_bytes || 0) / (1024 * 1024),
      rx: 0,
      tx: 0,
      total_rx: liveMetrics.rx_bytes || 0,
      total_tx: liveMetrics.tx_bytes || 0,
    } : { time: "", cpu: 0, ram: 0, rx: 0, tx: 0, total_rx: 0, total_tx: 0 });

  $: totalTrafficBytes = (latestMetrics.total_rx || 0) + (latestMetrics.total_tx || 0);

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

  function getStatusBadgeClass(status: string): string {
    const s = status.toLowerCase();
    if (s === "running") return "success";
    if (s === "draining") return "warning";
    if (s === "building" || s === "scheduled" || s === "pending" || s === "paused") return "warning";
    if (s === "failed" || s === "cancelled") return "destructive";
    return "secondary";
  }

  function formatMetricValue(name: string | number, value: unknown): string {
    const numericValue = typeof value === "number" ? value : Number(value);
    if (!Number.isFinite(numericValue)) return "--";
    if (name === "cpu") return `${numericValue.toFixed(1)}%`;
    if (name === "ram") return `${numericValue.toFixed(0)} MiB`;
    if (name === "rx" || name === "tx") return formatNetworkRate(numericValue);
    return numericValue.toLocaleString();
  }

  function getDeploymentButtonText(dep: DeploymentInfo, isCurrentlyInProd: boolean) {
    if (isCurrentlyInProd) return "Currently in Prod";
    if (dep.status === "DRAINING") return "Draining...";
    if (dep.status === "BUILDING") return "Building...";
    if (dep.status === "STARTING" || dep.status === "SCHEDULED") return "Starting...";
    return "Promote to Prod";
  }

  function formatDuration(start: string, end: string) {
    const diff = new Date(end).getTime() - new Date(start).getTime();
    if (diff < 0) return "--";
    if (diff < 1000) return `${diff}ms`;
    const seconds = Math.floor(diff / 1000);
    if (seconds < 60) {
      const ms = diff % 1000;
      return ms > 0 ? `${seconds}s ${ms}ms` : `${seconds}s`;
    }
    const minutes = Math.floor(seconds / 60);
    return `${minutes}m ${seconds % 60}s`;
  }
</script>

<svelte:head>
  <title>Mikrom - {appName}</title>
</svelte:head>

<DashboardLayout>
    <div class="flex flex-col justify-between gap-4 md:flex-row md:items-center">
      <div class="flex items-center gap-4">
        <div class="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground">
          <Boxes />
        </div>
        <div>
          <div class="flex items-center gap-3">
            <h1 class="text-2xl font-semibold tracking-tight">{app?.name || appName}.apps.mikrom.spluca.org</h1>
            <Button variant="outline" size="sm" href={`https://${app?.name || appName}.apps.mikrom.spluca.org`} target="_blank" rel="noreferrer" className="shrink-0">
              <Globe2 class="size-4" />
              <span class="hidden sm:inline">Visit site</span>
              <ExternalLink class="size-4" />
            </Button>
          </div>
          <p class="mt-1 text-sm text-muted-foreground">Manage {app?.name || "application"} deployments and monitor production instances.</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <Button size="sm" onclick={handleDeployApp} disabled={deployingApp}>
          {#if deployingApp}
            <Loader2 class="size-4 animate-spin" />
          {:else}
            <Rocket class="size-4" />
          {/if}
          Deploy Now
        </Button>
        <Button size="sm" variant="outline" onclick={() => (showWebhookModal = true)}>
          <Cog class="size-4" />
          Auto-deploy
        </Button>
        <Button size="sm" variant="destructive" onclick={() => (confirmDeleteApp ? handleDeleteApp() : (confirmDeleteApp = true))} disabled={deletingApp}>
          {#if deletingApp}
            <Loader2 class="size-4 animate-spin" />
          {:else}
            <Trash2 class="size-4" />
          {/if}
          {confirmDeleteApp ? "Confirm Delete?" : "Delete App"}
        </Button>
      </div>
    </div>

    <section class="space-y-4">
      <h2 class="text-lg font-bold tracking-tight">Deployment History</h2>
      <Card class="overflow-hidden">

        <div class="overflow-x-auto">
          <table class="min-w-[900px] w-full">
            <thead>
              <tr class="border-b border-border text-left text-sm">
                <th class="px-4 py-3">Deployment</th>
                <th class="px-4 py-3">Status</th>
                <th class="px-4 py-3">Duration</th>
                <th class="px-4 py-3">Created</th>
                <th class="px-4 py-3">Environment</th>
                <th class="px-4 py-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#if loading && deployments.length === 0}
                {#each Array.from({ length: 3 }) as _}
                  <tr class="border-b border-border">
                    <td class="px-4 py-4" colspan="6"><div class="h-8 animate-pulse rounded bg-muted"></div></td>
                  </tr>
                {/each}
              {:else if deployments.length === 0}
                <tr>
                  <td class="py-10" colspan="6">
                    <EmptyState>
                      <Rocket class="size-10 text-muted-foreground" />
                      <h3 class="text-xl font-semibold">No deployments yet</h3>
                      <p class="text-sm text-muted-foreground">Deployments for this application will appear here.</p>
                    </EmptyState>
                  </td>
                </tr>
              {:else}
                {#each [...deployments].sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()) as dep}
                  {@const isProduction = prodId === dep.id}
                  {@const canActivate = ["RUNNING", "PAUSED", "STOPPED", "FAILED"].includes(dep.status) && !isProduction}
                  <tr class="border-b border-border">
                    <td class="px-4 py-4">
                      <div class="flex flex-col gap-1">
                        <span class="max-w-[300px] truncate text-sm font-semibold line-clamp-1">
                          {dep.git_commit_message || dep.image_tag || (dep.status === "BUILDING" ? `Deploying ${dep.id.split("-")[0]}...` : `Deployment ${dep.id.split("-")[0]}`)}
                        </span>
                        <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                          <span class="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5"><GitBranch class="size-3" />{dep.git_branch || "main"}</span>
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
                    <td class="px-4 py-4"><Badge variant={getStatusBadgeClass(dep.status) as "default" | "secondary" | "outline" | "success" | "warning" | "destructive"} className="font-semibold capitalize">{dep.status}</Badge></td>
                    <td class="px-4 py-4 text-xs font-medium text-muted-foreground">{dep.status === "RUNNING" || dep.status === "FAILED" || dep.status === "PAUSED" || dep.status === "STOPPED" || dep.status === "DRAINING" ? formatDuration(dep.created_at, dep.updated_at) : "..."}</td>
                    <td class="whitespace-nowrap px-4 py-4 text-xs text-muted-foreground">{new Date(dep.created_at).toLocaleString()}</td>
                    <td class="px-4 py-4">
                      {#if isProduction}
                        <div class="flex items-center gap-1.5 text-sm font-semibold text-status-online">
                          <CheckCircle2 class="size-5" />
                          <span>Production</span>
                        </div>
                      {:else}
                        <span class="text-xs italic text-muted-foreground">Preview</span>
                      {/if}
                    </td>
                    <td class="px-4 py-4 text-right">
                      <Button size="sm" variant={isProduction ? "outline" : "default"} disabled={!canActivate || activatingDeploymentId !== null} onclick={() => handleActivate(dep.id)} className="ml-auto">
                        {getDeploymentButtonText(dep, isProduction)}
                        {#if !isProduction && !["BUILDING", "STARTING", "SCHEDULED", "DRAINING"].includes(dep.status)}
                          <Rocket class="ml-2 size-3" />
                        {/if}
                        {#if dep.status === "BUILDING" || dep.status === "DRAINING" || activatingDeploymentId === dep.id}
                          <Loader2 class="ml-2 size-3 animate-spin" />
                        {/if}
                      </Button>
                    </td>
                  </tr>
                {/each}
              {/if}
            </tbody>
          </table>
        </div>
      </Card>
    </section>

    {#if active && (["RUNNING", "DRAINING", "STARTING", "HEALTHY"].includes(active.status.toUpperCase()) || liveMetrics)}
      <div class="space-y-6 animate-in fade-in duration-500 border-t border-border pt-6">
        <h2 class="text-lg font-bold tracking-tight">Live Performance</h2>

        {#if !liveMetrics}
          <Card class="p-12 flex flex-col items-center justify-center text-center space-y-4">
            <Loader2 class="size-8 animate-spin text-muted-foreground" />
            <p class="text-muted-foreground">Connecting to instance metrics...</p>
          </Card>
        {:else}
          <Card class="overflow-hidden">
            <CardHeader class="border-b bg-muted/20">
              <div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
                <div class="flex flex-col gap-1.5">
                  <CardTitle>System Performance</CardTitle>
                  <CardDescription>Live CPU, RAM and network throughput. Total traffic: {formatBytes(totalTrafficBytes)}.</CardDescription>
                </div>
                <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                  {#each metricCards as metric}
                    <div class="min-w-36 rounded-lg border bg-background/80 p-3 shadow-sm">
                      <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                        <span class={`size-2 rounded-full ${metric.color}`}></span>
                        <svelte:component this={metric.icon} class="size-4" />
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
      </div>
    {/if}

  {#if showWebhookModal}
    <Modal open={showWebhookModal} title="GitHub Auto-deploy Configuration" width="max-w-[600px]" on:close={() => (showWebhookModal = false)}>
      <div class="space-y-6 pt-4">
        <div class="flex items-start gap-3">
          <Info class="mt-0.5 size-6 shrink-0 text-indigo-500" />
          <div>
            <p class="text-sm text-muted-foreground">
              Set up a webhook in your GitHub repository to enable automatic deployments on every push to the <code class="rounded bg-muted px-1 text-foreground">main</code> or <code class="rounded bg-muted px-1 text-foreground">master</code> branch.
            </p>
          </div>
        </div>

        <div class="space-y-4 pt-2">
          <div class="space-y-1.5">
            <p class="text-[10px] font-bold uppercase tracking-wider text-muted-foreground">Payload URL</p>
            <div class="flex items-center gap-2">
              <Input className="flex-1 font-mono text-xs" readOnly value={`${API_BASE_URL}/webhooks/github/${appName}`} />
              <Button variant="outline" size="sm" className="h-9 px-3" onclick={() => copy(`${API_BASE_URL}/webhooks/github/${appName}`)}>
                <Clipboard class="size-4" />
              </Button>
            </div>
          </div>

          <div class="space-y-1.5">
            <p class="text-[10px] font-bold uppercase tracking-wider text-muted-foreground">Secret</p>
            <div class="flex items-center gap-2">
              <div class="relative flex-1">
                <Input className="w-full pr-10 font-mono text-xs" readOnly type={showSecret ? "text" : "password"} value={secret || ""} />
                <button class="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground" on:click={() => (showSecret = !showSecret)}>
                  {#if showSecret}<EyeOff class="size-4" />{:else}<Eye class="size-4" />{/if}
                </button>
              </div>
              <Button variant="outline" size="sm" className="h-9 px-3" onclick={() => copy(secret || "")}>
                <Clipboard class="size-4" />
              </Button>
            </div>
          </div>
        </div>

        <div class="rounded-lg border border-border bg-muted/50 p-4">
          <h4 class="mb-2 text-xs font-bold">Instructions:</h4>
          <ol class="list-inside list-decimal space-y-2 text-xs text-muted-foreground">
            <li>Go to your repository on GitHub.</li>
            <li>Click on <span class="font-medium text-foreground">Settings</span> &gt; <span class="font-medium text-foreground">Webhooks</span>.</li>
            <li>Click <span class="font-medium text-foreground">Add webhook</span>.</li>
            <li>Paste the <span class="font-medium text-foreground">Payload URL</span> and <span class="font-medium text-foreground">Secret</span> above.</li>
            <li>Set <span class="font-medium text-foreground">Content type</span> to <span class="font-mono">application/json</span>.</li>
            <li>Click <span class="font-medium text-foreground">Add webhook</span> at the bottom.</li>
          </ol>
        </div>
      </div>
    </Modal>
  {/if}

</DashboardLayout>
