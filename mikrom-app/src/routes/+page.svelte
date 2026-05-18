<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Activity, ArrowRight, Bot, CalendarClock, Container, Cpu, Hammer, LayoutDashboard, Plus, Rocket, Router, Server } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Button from "$lib/components/Button.svelte";
  import Alert from "$lib/components/Alert.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import CreateAppModal from "$lib/components/CreateAppModal.svelte";
  import Separator from "$lib/components/Separator.svelte";
  import { formatDate } from "$lib/utils";
  import { getToken } from "$lib/auth";
  import { health, type AppInfo } from "$lib/api";
  import { vmsStore, refreshVms } from "$lib/stores/vms";
  import { appsStore, appsLoading, refreshApps } from "$lib/stores/apps";
  import { derived } from "svelte/store";

  let healthData: Awaited<ReturnType<typeof health>> | null = null;
  let loadingHealth = true;
  let showCreate = false;

  const runningCountStore = derived(vmsStore, ($vms) => $vms.filter((vm) => vm.status.toLowerCase() === "running").length);
  const pendingCountStore = derived(vmsStore, ($vms) => $vms.filter((vm) => ["scheduled", "pending", "building"].includes(vm.status.toLowerCase())).length);
  const appsWithStatusStore = derived([appsStore, vmsStore], ([$apps, $vms]) => 
    [...$apps]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
      .slice(0, 5)
      .map((app) => ({
        ...app,
        liveVm: $vms.find((vm) => vm.app_id === app.id || vm.app_name === app.name),
        status: $vms.find((vm) => vm.app_id === app.id || vm.app_name === app.name)?.status || "Stopped",
      }))
  );
  const hasUndeployedAppsStore = derived([appsStore, vmsStore], ([$apps, $vms]) => 
    $apps.length > 0 && $apps.every((app) => !$vms.some((vm) => vm.app_id === app.id || vm.app_name === app.name))
  );

  onMount(async () => {
    const token = getToken();
    if (!token) return;

    if ($appsStore.length === 0) {
      void refreshApps();
    }
    
    if ($vmsStore.length === 0) {
      void refreshVms();
    }

    const healthResult = await health().catch(() => null);
    healthData = healthResult;
    loadingHealth = false;
  });

  $: offlineServices = Object.values(healthData?.services || {}).filter((status) => status !== "ONLINE").length;
  const hasHealthError = () => !loadingHealth && !healthData;
  const healthServices = [
    { name: "API", key: "API", icon: Cpu },
    { name: "Agents", key: "Agents", icon: Bot },
    { name: "Scheduler", key: "Scheduler", icon: CalendarClock },
    { name: "Builder", key: "Builder", icon: Hammer },
    { name: "Router", key: "Router", icon: Router },
  ] as const;
</script>

<svelte:head>
  <title>Mikrom - Dashboard</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-8">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <LayoutDashboard />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Dashboard</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">Monitor and manage your cloud infrastructure.</p>
      </div>
      {#if !(!$appsLoading && $appsStore.length === 0)}
        <Button onclick={() => (showCreate = true)}>
          <Plus class="size-4" />
          New Application
        </Button>
      {/if}
    </div>

    {#if $hasUndeployedAppsStore}
      <Alert>
        <Rocket class="size-4 shrink-0" />
        <div class="flex w-full flex-wrap items-center justify-between gap-4">
          <div class="space-y-1">
            <div class="font-medium">Deploy your first app</div>
            <div>You have applications created but none are currently running in a microVM.</div>
          </div>
          <Button size="sm" href={`/apps/${encodeURIComponent($appsStore[0].name)}`}>Deploy now</Button>
        </div>
      </Alert>
    {/if}

    <div class="grid gap-4 md:grid-cols-3">
      <Card>
        <div class="flex items-center justify-between gap-4 border-b border-border p-5 pb-2">
          <div class="text-sm font-medium">Applications</div>
          <Container class="size-4 text-muted-foreground" />
        </div>
        <div class="p-5 pt-0">
          <div class="text-3xl font-semibold">{$appsStore.length}</div>
          <p class="text-xs text-muted-foreground">Git projects in the workspace</p>
        </div>
      </Card>
      <Card>
        <div class="flex items-center justify-between gap-4 border-b border-border p-5 pb-2">
          <div class="text-sm font-medium">Running VMs</div>
          <Activity class="size-4 text-muted-foreground" />
        </div>
        <div class="p-5 pt-0">
          <div class="text-3xl font-semibold">{$runningCountStore}</div>
          <p class="text-xs text-muted-foreground">Instances currently serving traffic</p>
        </div>
      </Card>
      <Card>
        <div class="flex items-center justify-between gap-4 border-b border-border p-5 pb-2">
          <div class="text-sm font-medium">Deploying</div>
          <Rocket class="size-4 text-muted-foreground" />
        </div>
        <div class="p-5 pt-0">
          <div class="text-3xl font-semibold">{$pendingCountStore}</div>
          <p class="text-xs text-muted-foreground">Builds or starts in progress</p>
        </div>
      </Card>
    </div>

    {#if $appsStore.length === 0 && !$appsLoading}
        <EmptyState class="min-h-[420px] border">
          <Rocket class="size-10 text-muted-foreground" />
          <h2 class="text-xl font-semibold">No applications found</h2>
          <p class="max-w-md text-sm text-muted-foreground">Create your first application and deploy it to a Mikrom microVM.</p>
          <Button onclick={() => (showCreate = true)}>
            <Plus class="size-4" />
          Create Application
        </Button>
      </EmptyState>
    {:else}
      <div class="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
        <Card class="min-w-0">
          <div class="border-b border-border p-5">
            <div class="flex items-center justify-between gap-4">
              <div>
                <h2 class="text-lg font-semibold">Recent Applications</h2>
                <p class="text-sm text-muted-foreground">Latest projects and their runtime state.</p>
              </div>
              <Button variant="outline" size="sm" href="/apps">
                View all
                <ArrowRight class="size-4" />
              </Button>
            </div>
          </div>
          <div class="overflow-x-auto">
            <table class="w-full table-fixed">
              <thead>
                <tr class="border-b border-border text-left text-sm">
                  <th class="w-[52%] px-4 py-3">Application</th>
                  <th class="w-[24%] px-4 py-3">Status</th>
                  <th class="hidden w-[24%] px-4 py-3 xl:table-cell">Created</th>
                  <th class="w-[96px] px-4 py-3 text-right">Actions</th>
                </tr>
              </thead>
              <tbody>
                {#if $appsLoading && $appsStore.length === 0}
                  {#each Array.from({ length: 3 }) as _, i}
                    <tr class="border-b border-border">
                      <td class="px-4 py-4"><div class="h-9 w-44 animate-pulse rounded bg-muted"></div></td>
                      <td class="px-4 py-4"><div class="h-5 w-20 animate-pulse rounded bg-muted"></div></td>
                      <td class="hidden px-4 py-4 xl:table-cell"><div class="h-5 w-24 animate-pulse rounded bg-muted"></div></td>
                      <td class="px-4 py-4 text-right"><div class="ml-auto h-8 w-20 animate-pulse rounded bg-muted"></div></td>
                    </tr>
                  {/each}
                {:else}
                  {#each $appsWithStatusStore as app}
                    <tr class="border-b border-border">
                      <td class="px-4 py-4">
                        <div class="flex min-w-0 items-center gap-3">
                          <div class="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-background">
                            <Server class="size-4" />
                          </div>
                          <div class="min-w-0">
                            <div class="truncate font-medium">{app.name}</div>
                            <div class="truncate text-xs text-muted-foreground">{app.hostname || "No public hostname"}</div>
                          </div>
                        </div>
                      </td>
                      <td class="px-4 py-4">
                        <Badge variant={app.status.toLowerCase() === "running" ? "success" : app.status.toLowerCase() === "building" || app.status.toLowerCase() === "pending" ? "warning" : "secondary"} className="capitalize">{app.status}</Badge>
                      </td>
                      <td class="hidden px-4 py-4 text-sm text-muted-foreground xl:table-cell">{formatDate(app.created_at)}</td>
                      <td class="px-4 py-4 text-right">
                        <Button size="sm" variant="outline" href={`/apps/${encodeURIComponent(app.name)}`}>Manage</Button>
                      </td>
                    </tr>
                  {/each}
                {/if}
              </tbody>
            </table>
          </div>
        </Card>

        <Card>
          <div class="border-b border-border p-5">
              <div class="flex items-center justify-between gap-4">
              <div class="flex flex-col gap-1.5">
                <h2 class="text-lg font-semibold">System Status</h2>
                <p class="text-sm text-muted-foreground">Health of core services.</p>
              </div>
              <Badge variant={hasHealthError() || offlineServices > 0 ? "destructive" : "secondary"}>
                {hasHealthError() || offlineServices > 0 ? "Degraded" : "Operational"}
              </Badge>
            </div>
          </div>
          <div class="flex flex-col gap-4 p-5">
            {#each healthServices as service, index}
              {@const ServiceIcon = service.icon}
              {@const status = healthData?.services?.[service.key] || (hasHealthError() ? "OFFLINE" : "CHECKING")}
              {@const isOnline = status === "ONLINE"}
              {@const isChecking = status === "CHECKING"}
              <div class="flex flex-col gap-4">
                <div class="flex items-center justify-between gap-4">
                  <div class="flex items-center gap-2">
                    <ServiceIcon class="size-4 text-muted-foreground" />
                    <span class="text-sm font-medium">{service.name}</span>
                  </div>
                  <Badge variant={isOnline ? "secondary" : isChecking ? "outline" : "destructive"} className="uppercase">
                    {status}
                  </Badge>
                </div>
                {#if index < healthServices.length - 1}
                  <Separator />
                {/if}
              </div>
            {/each}
          </div>
          <div class="border-t border-border p-5 pt-4">
            <p class="text-xs text-muted-foreground">Version {healthData?.version || "0.0.0"}</p>
          </div>
        </Card>
      </div>
    {/if}
  </div>

  {#if showCreate}
    <CreateAppModal bind:open={showCreate} />
  {/if}
</DashboardLayout>
