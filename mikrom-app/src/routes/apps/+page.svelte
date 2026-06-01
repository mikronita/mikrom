<script lang="ts">
  import { onMount } from "svelte";
  import { Boxes, Calendar, Cpu, FolderPlus, HardDrive, Plus, Radio } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { 
    Card, 
    CardHeader, 
    CardTitle, 
    CardDescription, 
    CardContent, 
    Badge, 
    Button, 
    Input,
    EmptyState, 
    CardSkeleton 
  } from "$lib/components";
  import CreateAppModal from "$lib/components/CreateAppModal.svelte";
  import { formatDate } from "$lib/utils";
  import { matchesSearch } from "$lib/search";
  import { vmsStore } from "$lib/stores/vms";
  import { appsStore, appsLoading, appsError, refreshApps } from "$lib/stores/apps";
  import { toast } from "$lib/toast";

  let showCreate = false;
  let query = "";
  let statusFilter: "all" | "active" | "paused" | "idle" = "all";

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
  });

  $: if ($appsError) {
    toast.error($appsError);
  }

  function getAppResources(appId: string, appName: string) {
    const appVms = $vmsStore.filter((vm) => (vm.app_id === appId || vm.app_name === appName) && vm.status.toLowerCase() === "running");
    return {
      vcpus: appVms.reduce((total, vm) => total + (vm.vcpus || 1), 0),
      memory_mib: appVms.reduce((total, vm) => total + (vm.memory_mib || 128), 0),
      count: appVms.length,
    };
  }

  function getScaleStateLabel(scaleState: string) {
    if (scaleState === "scaled_to_zero") return "Paused";
    return "Running";
  }

  function getScaleStateBadgeClass(scaleState: string) {
    if (scaleState === "scaled_to_zero") {
      return "border-transparent bg-muted/70 text-muted-foreground";
    }
    return "border-transparent bg-status-info/10 text-status-info";
  }

  function getAppCardState(scaleState: string, hasRunningReplicas: boolean) {
    if (hasRunningReplicas) return "active";
    if (scaleState === "scaled_to_zero") return "paused";
    return "idle";
  }

  $: filteredApps = [...$appsStore]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .filter((app) => {
      const appVms = $vmsStore.filter((vm) => (vm.app_id === app.id || vm.app_name === app.name) && vm.status.toLowerCase() === "running");
      const effectiveScaleState = app.scale_state || (appVms.length > 0 ? "active" : "scaled_to_zero");
      const cardState = getAppCardState(effectiveScaleState, appVms.length > 0);
      return (statusFilter === "all" || cardState === statusFilter) && matchesSearch([app.name, app.hostname, app.git_url], query);
    });
</script>

<svelte:head>
  <title>Mikrom - Applications</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Boxes />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Applications</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">Manage the Git-based applications in your active project.</p>
      </div>
      <Button onclick={() => (showCreate = true)}>
        <Plus class="size-4" />
        New Application
      </Button>
    </div>

    <Card size="sm" class="overflow-hidden">
      <CardContent class="flex flex-col gap-4">
        <div class="min-w-0 flex-1">
          <Input bind:value={query} placeholder="Search by app name, hostname or repository" />
        </div>
        <div class="flex border-b border-border overflow-x-auto">
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "all"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "all")}
          >
            All
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "active"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "active")}
          >
            Active
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "paused"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "paused")}
          >
            Paused
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "idle"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "idle")}
          >
            Idle
          </button>
        </div>
      </CardContent>
    </Card>

    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
    {#if $appsLoading && $appsStore.length === 0}
      {#each Array.from({ length: 6 }) as _}
        <CardSkeleton
          titleClassName="w-32"
          descriptionClassName="w-full"
          footerLineClassName="w-40"
          footerPills={["w-20", "w-24"]}
        />
      {/each}
      {:else if filteredApps.length === 0}
        <div class="col-span-full">
          <EmptyState class="py-16">
            <FolderPlus class="size-10 text-muted-foreground" />
            <h2 class="text-xl font-semibold">
              {query || statusFilter !== "all" ? "No matching applications" : "No applications found"}
            </h2>
            <p class="max-w-md text-sm text-muted-foreground">
              {query || statusFilter !== "all"
                ? "Try a different search term or clear the status filter."
                : "Get started by connecting your first repository."}
            </p>
            <Button size="sm" onclick={() => (showCreate = true)}>
              <Plus class="size-4" />
              Connect your first repository
            </Button>
          </EmptyState>
        </div>
      {:else}
        {#each filteredApps as app}
          {@const resources = getAppResources(app.id, app.name)}
          {@const effectiveScaleState = app.scale_state || (resources.count > 0 ? "active" : "scaled_to_zero")}
          {@const hasRunningReplicas = resources.count > 0}
          <a class="block" href={`/apps/${encodeURIComponent(app.name)}`}>
            <Card size="sm">
              <CardHeader>
                <div class="flex items-start gap-4">
                  <div class="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <Boxes class="size-5" />
                  </div>
                  <div class="flex min-w-0 flex-1 flex-col gap-2">
                    <div class="flex min-w-0 items-center gap-2">
                      <CardTitle class="truncate text-base">{app.name}</CardTitle>
                    </div>
                    <CardDescription>Application scope</CardDescription>
                  </div>
                  <Badge variant="outline" class={`shrink-0 gap-1.5 uppercase ${getScaleStateBadgeClass(effectiveScaleState)}`}>
                    <Radio class="size-3" />
                    {getScaleStateLabel(effectiveScaleState)}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent class="flex flex-col gap-4">
                <div class="flex flex-col gap-2 text-xs text-muted-foreground">
                  <div class="flex flex-wrap items-center justify-between gap-3">
                    <span class="inline-flex items-center gap-1.5">
                      <Calendar class="size-4" />
                      Created {formatDate(app.created_at)}
                    </span>
                    <span class="inline-flex items-center gap-1.5">
                      <Calendar class="size-4" />
                      Updated {formatDate(app.updated_at || app.created_at)}
                    </span>
                  </div>
                  <div class="flex flex-wrap items-center gap-2">
                    {#if hasRunningReplicas}
                      <Badge variant="outline" class="gap-1.5">
                        <Cpu class="size-3" />
                        <span>{resources.vcpus} vCPU</span>
                      </Badge>
                      <Badge variant="outline" class="gap-1.5">
                        <HardDrive class="size-3" />
                        <span>{resources.memory_mib} MB</span>
                      </Badge>
                      {#if resources.count > 1}
                        <Badge variant="outline" class="bg-status-online/10 text-status-online border-transparent gap-1.5">
                          <span>{resources.count} replicas</span>
                        </Badge>
                      {/if}
                    {:else if effectiveScaleState === "scaled_to_zero"}
                      <Badge variant="outline" class="border-transparent bg-muted/70 text-muted-foreground">
                        0 replicas
                      </Badge>
                    {:else}
                      <Badge variant="outline">No active deployment</Badge>
                    {/if}
                  </div>
                </div>
              </CardContent>
            </Card>
          </a>
        {/each}
      {/if}
    </div>
  </div>

  {#if showCreate}
    <CreateAppModal bind:open={showCreate} />
  {/if}
</DashboardLayout>
