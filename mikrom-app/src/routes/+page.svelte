<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Activity, ArrowRight, Bot, CalendarClock, Container, Cpu, Hammer, LayoutDashboard, Plus, Rocket, Router, Server } from "lucide-svelte";
  import { 
    Card, 
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    CardFooter,
    Badge, 
    Button, 
    EmptyState, 
    Skeleton, 
    Separator,
    Table,
    TableHeader,
    TableBody,
    TableRow,
    TableHead,
    TableCell
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import CreateAppModal from "$lib/components/CreateAppModal.svelte";
  import { formatDate } from "$lib/utils";
  import { getToken } from "$lib/auth";
  import { vmsStore, vmsLoading, refreshVms } from "$lib/stores/vms";
  import { appsStore, appsLoading, refreshApps } from "$lib/stores/apps";
  import { healthStore, healthLoading, initHealthPolling } from "$lib/stores/health";
  import { toast } from "$lib/toast";
  import { derived } from "svelte/store";

  let showCreate = false;

  const runningCountStore = derived(vmsStore, ($vms) => $vms.filter((vm) => vm.status.toLowerCase() === "running").length);
  const pendingCountStore = derived(vmsStore, ($vms) => $vms.filter((vm) => ["scheduled", "pending", "building"].includes(vm.status.toLowerCase())).length);
  const appsWithStatusStore = derived([appsStore, vmsStore], ([$apps, $vms]) => 
    [...$apps]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
      .slice(0, 5)
      .map((app) => {
        const liveVm = $vms.find((vm) => vm.app_id === app.id || vm.app_name === app.name);
        let status = liveVm?.status || (app.active_deployment_id ? "Paused" : "Stopped");
        
        if (app.scale_state === "scaled_to_zero" && status !== "Stopped") {
          status = "Paused";
        }

        return {
          ...app,
          liveVm,
          status,
        };
      })
  );
  const hasUndeployedAppsStore = derived([appsStore, vmsStore], ([$apps, $vms]) => 
    $apps.length > 0 && $apps.every((app) => !$vms.some((vm) => vm.app_id === app.id || vm.app_name === app.name))
  );

  let unsubscribeHealth: (() => void) | undefined;
  let unsubscribeUndeployed: () => void;

  onMount(async () => {
    const token = getToken();
    if (!token) return;

    if ($appsStore.length === 0) {
      void refreshApps();
    }
    
    if ($vmsStore.length === 0) {
      void refreshVms();
    }

    unsubscribeHealth = initHealthPolling();

    // Check for undeployed apps and notify
    unsubscribeUndeployed = hasUndeployedAppsStore.subscribe($hasUndeployedApps => {
      if ($appsLoading || $vmsLoading) return;
      if ($hasUndeployedApps && $appsStore.length > 0 && $vmsStore.length === 0) {
        toast.info("You have undeployed applications. Deploy your first app now!", {
          action: {
            label: "Deploy",
            onClick: () => {
              window.location.href = `/apps/${encodeURIComponent($appsStore[0].name)}`;
            }
          }
        });
      }
    });
  });

  onDestroy(() => {
    if (unsubscribeUndeployed) unsubscribeUndeployed();
    if (unsubscribeHealth) unsubscribeHealth();
  });

  $: offlineServices = Object.values($healthStore?.services || {}).filter((status) => status !== "ONLINE").length;
  const hasHealthError = () => !$healthLoading && !$healthStore;
  const healthServices = [
    { name: "API", key: "API", icon: Cpu },
    { name: "Agents", key: "Agents", icon: Bot },
    { name: "Scheduler", key: "Scheduler", icon: CalendarClock },
    { name: "Builder", key: "Builder", icon: Hammer },
    { name: "Router", key: "Router", icon: Router },
  ] as const;

  function getAppStatusVariant(status: string) {
    const normalized = status.toLowerCase();
    if (normalized === "running") return "outline";
    if (normalized === "paused") return "outline";
    if (["building", "pending", "scheduled", "starting", "draining"].includes(normalized)) return "secondary";
    if (["failed", "cancelled", "offline", "error"].includes(normalized)) return "destructive";
    return "outline";
  }

  function getHealthVariant(status: string) {
    if (status === "ONLINE") return "outline";
    if (status === "CHECKING") return "outline";
    return "destructive";
  }

  function getAppStatusClass(status: string) {
    const normalized = status.toLowerCase();
    if (normalized === "running") {
      return "border-transparent bg-status-info/10 text-status-info";
    }
    if (normalized === "paused") {
      return "border-transparent bg-muted/70 text-muted-foreground";
    }
    if (normalized === "stopped") {
      return "border-transparent bg-muted/40 text-muted-foreground/60";
    }
    return "";
  }

  function getHealthClass(status: string) {
    if (status === "ONLINE") return "!border-transparent !bg-status-online/10 !text-status-online";
    if (status === "CHECKING") return "!border-transparent !bg-muted/70 !text-muted-foreground";
    return "!border-transparent !bg-status-offline/10 !text-status-offline";
  }
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

    <div class="grid gap-4 md:grid-cols-3">
      <Card>
        <CardHeader class="flex flex-row items-start justify-between gap-4 pb-3">
          <div class="flex flex-col gap-1">
            <CardDescription>Applications</CardDescription>
            {#if $appsLoading && $appsStore.length === 0}
              <Skeleton class="mt-1 h-8 w-16" />
            {:else}
              <CardTitle class="text-3xl">{$appsStore.length}</CardTitle>
            {/if}
          </div>
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Container class="size-5" />
          </div>
        </CardHeader>
        <CardContent class="pt-0">
          <p class="text-sm text-muted-foreground">Git projects in the workspace</p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader class="flex flex-row items-start justify-between gap-4 pb-3">
          <div class="flex flex-col gap-1">
            <CardDescription>Running VMs</CardDescription>
            {#if $vmsLoading && $vmsStore.length === 0}
              <Skeleton class="mt-1 h-8 w-16" />
            {:else}
              <CardTitle class="text-3xl">{$runningCountStore}</CardTitle>
            {/if}
          </div>
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Activity class="size-5" />
          </div>
        </CardHeader>
        <CardContent class="pt-0">
          <p class="text-sm text-muted-foreground">Instances currently serving traffic</p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader class="flex flex-row items-start justify-between gap-4 pb-3">
          <div class="flex flex-col gap-1">
            <CardDescription>Deploying</CardDescription>
            {#if $vmsLoading && $vmsStore.length === 0}
              <Skeleton class="mt-1 h-8 w-16" />
            {:else}
              <CardTitle class="text-3xl">{$pendingCountStore}</CardTitle>
            {/if}
          </div>
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Rocket class="size-5" />
          </div>
        </CardHeader>
        <CardContent class="pt-0">
          <p class="text-sm text-muted-foreground">Builds or starts in progress</p>
        </CardContent>
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
        <Card class="min-w-0 h-fit">
          <CardHeader class="border-b">
            <div class="flex items-center justify-between gap-4">
              <div class="grid gap-1">
                <CardTitle>Recent Applications</CardTitle>
                <CardDescription>Latest projects and their runtime state.</CardDescription>
              </div>
              <Button variant="outline" size="sm" href="/apps">
                View all
                <ArrowRight class="size-4" />
              </Button>
            </div>
          </CardHeader>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead class="w-[52%]">Application</TableHead>
                <TableHead class="w-[24%]">Status</TableHead>
                <TableHead class="hidden w-[24%] xl:table-cell">Created</TableHead>
                <TableHead class="w-[96px] text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {#if $appsLoading && $appsStore.length === 0}
                {#each Array.from({ length: 3 }) as _}
                  <TableRow>
                    <TableCell><Skeleton class="h-9 w-44" /></TableCell>
                    <TableCell><Skeleton class="h-5 w-20" /></TableCell>
                    <TableCell class="hidden xl:table-cell"><Skeleton class="h-5 w-24" /></TableCell>
                    <TableCell class="text-right"><Skeleton class="ml-auto h-8 w-20" /></TableCell>
                  </TableRow>
                {/each}
              {:else}
                {#each $appsWithStatusStore as app}
                  <TableRow>
                    <TableCell>
                      <div class="flex min-w-0 items-center gap-3">
                        <div class="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-background">
                          <Server class="size-4" />
                        </div>
                        <div class="min-w-0">
                          <div class="truncate font-medium">{app.name}</div>
                          <div class="truncate text-xs text-muted-foreground">{app.hostname || "No public hostname"}</div>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant={getAppStatusVariant(app.status)} class={`capitalize ${getAppStatusClass(app.status)}`}>{app.status}</Badge>
                    </TableCell>
                    <TableCell class="hidden text-muted-foreground xl:table-cell">{formatDate(app.created_at)}</TableCell>
                    <TableCell class="text-right">
                      <Button size="sm" variant="outline" href={`/apps/${encodeURIComponent(app.name)}`}>Manage</Button>
                    </TableCell>
                  </TableRow>
                {/each}
              {/if}
            </TableBody>
          </Table>
        </Card>

        <Card>
          <CardHeader class="border-b">
            <div class="flex items-center justify-between gap-4">
              <div class="grid gap-1">
                <CardTitle>System Status</CardTitle>
                <CardDescription>Health of core services.</CardDescription>
              </div>
              <Badge variant="outline" class={hasHealthError() || offlineServices > 0 ? getHealthClass("OFFLINE") : getHealthClass("ONLINE")}>
                {hasHealthError() || offlineServices > 0 ? "Degraded" : "Operational"}
              </Badge>
            </div>
          </CardHeader>
          <CardContent class="flex flex-col gap-4 py-5">
            {#each healthServices as service, index}
              {@const ServiceIcon = service.icon}
              {@const status = $healthStore?.services?.[service.key] || (hasHealthError() ? "OFFLINE" : "CHECKING")}
              <div class="flex flex-col gap-4">
                <div class="flex items-center justify-between gap-4">
                  <div class="flex items-center gap-2">
                    <ServiceIcon class="size-4 text-muted-foreground" />
                    <span class="text-sm font-medium">{service.name}</span>
                  </div>
                  <Badge variant="outline" class={`uppercase ${getHealthClass(status)}`}>
                    {status}
                  </Badge>
                </div>
                {#if index < healthServices.length - 1}
                  <Separator />
                {/if}
              </div>
            {/each}
          </CardContent>
          <CardFooter class="border-t py-3">
            <p class="text-xs text-muted-foreground">Version {$healthStore?.version || "0.0.0"}</p>
          </CardFooter>
        </Card>
      </div>
    {/if}
  </div>

  {#if showCreate}
    <CreateAppModal bind:open={showCreate} />
  {/if}
</DashboardLayout>
