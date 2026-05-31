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
    EmptyState, 
    CardSkeleton 
  } from "$lib/components";
  import CreateAppModal from "$lib/components/CreateAppModal.svelte";
  import { formatDate } from "$lib/utils";
  import { vmsStore } from "$lib/stores/vms";
  import { appsStore, appsLoading, appsError, refreshApps } from "$lib/stores/apps";
  import { toast } from "$lib/toast";

  let showCreate = false;

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
      {:else if $appsStore.length === 0}
        <div class="col-span-full">
          <EmptyState class="py-16">
            <FolderPlus class="size-10 text-muted-foreground" />
            <h2 class="text-xl font-semibold">No applications found</h2>
            <p class="max-w-md text-sm text-muted-foreground">Get started by connecting your first repository.</p>
            <Button size="sm" onclick={() => (showCreate = true)}>
              <Plus class="size-4" />
              Connect your first repository
            </Button>
          </EmptyState>
        </div>
      {:else}
        {#each [...$appsStore].sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()) as app}
          {@const resources = getAppResources(app.id, app.name)}
          {@const effectiveScaleState = app.scale_state || (resources.count > 0 ? "active" : "scaled_to_zero")}
          {@const hasRunningReplicas = resources.count > 0}
          <a class="block" href={`/apps/${encodeURIComponent(app.name)}`}>
            <Card class="h-full overflow-hidden transition-colors hover:bg-muted/30">
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
                <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground">
                  <span class="inline-flex items-center gap-1.5">
                    <Calendar class="size-4" />
                    Created {formatDate(app.created_at)}
                  </span>
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
