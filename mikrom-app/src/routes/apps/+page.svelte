<script lang="ts">
  import { onMount } from "svelte";
    import Boxes from "@lucide/svelte/icons/boxes";
  import Calendar from "@lucide/svelte/icons/calendar";
  import Cpu from "@lucide/svelte/icons/cpu";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Plus from "@lucide/svelte/icons/plus";
  import Radio from "@lucide/svelte/icons/radio";
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
  import { vmsStore } from "$lib/stores/vms";
  import { appsStore, appsLoading, appsError, refreshApps } from "$lib/stores/apps";
  import { toast } from "$lib/toast";
  import {
    type AppCard,
    buildAppCards,
    filterAppCards,
  } from "$lib/domain/ui";

  let showCreate = false;
  let query = "";
  let statusFilter: "all" | "active" | "paused" | "idle" = "all";
  let appCards: AppCard[];
  let filteredApps: AppCard[];

  const statusFilters = [
    { value: "all", label: "All" },
    { value: "active", label: "Active" },
    { value: "paused", label: "Paused" },
    { value: "idle", label: "Idle" },
  ] as const;

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
  });

  $: appCards = buildAppCards($appsStore, $vmsStore);
  $: filteredApps = filterAppCards(appCards, query, statusFilter);

  $: if ($appsError) {
    toast.error($appsError);
  }

  function clearFilters() {
    query = "";
    statusFilter = "all";
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

    <Card size="sm" class="overflow-hidden">
      <CardContent class="flex flex-col gap-4">
        <div class="min-w-0 flex-1">
          <Input bind:value={query} placeholder="Search by app name, hostname or repository" />
        </div>
        <div class="flex border-b border-border overflow-x-auto">
          {#each statusFilters as filter}
            <button
              class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
                statusFilter === filter.value
                  ? "border-primary text-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground"
              }`}
              onclick={() => (statusFilter = filter.value)}
            >
              {filter.label}
            </button>
          {/each}
        </div>
        {#if query || statusFilter !== "all"}
          <div class="flex justify-end">
            <Button variant="ghost" size="sm" onclick={clearFilters}>Clear filters</Button>
          </div>
        {/if}
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
              {query || statusFilter !== "all" ? "No matching applications" : "No applications yet"}
            </h2>
            <p class="max-w-md text-sm text-muted-foreground">
              {query || statusFilter !== "all"
                ? "Try a different search term or clear the status filter."
                : "Connect your first repository to start deploying workloads."}
            </p>
            <div class="flex flex-wrap justify-center gap-2">
              <Button size="sm" onclick={() => (showCreate = true)}>
                <Plus class="size-4" />
                Connect your first repository
              </Button>
              <Button variant="outline" size="sm" href="/apps">
                View applications
              </Button>
            </div>
          </EmptyState>
        </div>
      {:else}
        {#each filteredApps as app}
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
                  <Badge variant="outline" class={`shrink-0 gap-1.5 uppercase ${app.scaleBadgeClass}`}>
                    <Radio class="size-3" />
                    {app.scaleLabel}
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
                    {#if app.resources.count > 0}
                      <Badge variant="outline" class="gap-1.5">
                        <Cpu class="size-3" />
                        <span>{app.resources.vcpus} vCPU</span>
                      </Badge>
                      <Badge variant="outline" class="gap-1.5">
                        <HardDrive class="size-3" />
                        <span>{app.resources.memory_mib} MB</span>
                      </Badge>
                      {#if app.resources.count > 1}
                        <Badge variant="outline" class="border-transparent gap-1.5 bg-status-online/10 text-status-online">
                          <span>{app.resources.count} replicas</span>
                        </Badge>
                      {/if}
                    {:else if app.scale_state === "scaled_to_zero"}
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
