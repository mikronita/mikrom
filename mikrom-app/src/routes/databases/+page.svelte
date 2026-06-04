<script lang="ts">
  import { onMount } from "svelte";
  import { Database as DatabaseIcon, Plus, Calendar, Cpu, HardDrive, Radio } from "lucide-svelte";
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
    CardSkeleton,
    EmptyState,
  } from "$lib/components";
  import CreateDatabaseModal from "$lib/components/CreateDatabaseModal.svelte";
  import { formatDate } from "$lib/utils";
  import { matchesSearch } from "$lib/search";
  import { databasesStore, databasesLoading, databasesError, refreshDatabases } from "$lib/stores/databases";

  let showCreate = false;
  let query = "";
  let statusFilter: "all" | "running" | "provisioning" | "deleting" | "failed" = "all";

  const statusFilters = [
    { value: "all", label: "All" },
    { value: "running", label: "Running" },
    { value: "provisioning", label: "Provisioning" },
    { value: "deleting", label: "Deleting" },
    { value: "failed", label: "Failed" },
  ] as const;

  function getStatusBadgeClass(status: string) {
    switch (status) {
      case "Running":
        return "border-transparent bg-status-online/10 text-status-online";
      case "Provisioning":
        return "border-transparent bg-status-info/10 text-status-info";
      case "Deleting":
      case "Failed":
        return "border-transparent bg-status-offline/10 text-status-offline";
      default:
        return "border-transparent bg-muted/70 text-muted-foreground";
    }
  }

  $: filteredDatabases = [...$databasesStore]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .filter((db) => {
      const matchesStatus = statusFilter === "all" || db.status.toLowerCase() === statusFilter;
      return matchesStatus && matchesSearch([db.name, `PostgreSQL ${db.postgres_version}`, db.status], query);
    });

  onMount(() => {
    if ($databasesStore.length === 0) {
      void refreshDatabases();
    }
  });
</script>

<svelte:head>
  <title>Mikrom - Databases</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <DatabaseIcon />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Databases</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">
          Managed Neon-backed PostgreSQL databases for the active project.
        </p>
      </div>
      <Button onclick={() => (showCreate = true)}>
        <Plus class="size-4" />
        New Database
      </Button>
    </div>

    {#if $databasesError}
      <Card class="border-destructive/20 bg-destructive/5">
        <CardContent class="flex items-center justify-between gap-4 py-4">
          <div class="flex flex-col gap-1">
            <p class="text-sm font-medium">Could not load databases</p>
            <p class="text-sm text-muted-foreground">{$databasesError}</p>
          </div>
          <Button variant="outline" size="sm" onclick={() => refreshDatabases()}>Retry</Button>
        </CardContent>
      </Card>
    {/if}

    <Card size="sm" class="overflow-hidden">
      <CardContent class="flex flex-col gap-4">
        <div class="min-w-0 flex-1">
          <Input bind:value={query} placeholder="Search by database name, version or status" />
        </div>
        <div class="flex overflow-x-auto border-b border-border">
          {#each statusFilters as filter}
            <button
              class={`whitespace-nowrap border-b-2 px-4 py-2 text-sm font-medium transition-colors ${
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
      </CardContent>
    </Card>

    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      {#if $databasesLoading && $databasesStore.length === 0}
        {#each Array.from({ length: 6 }) as _}
          <CardSkeleton
            titleClassName="w-32"
            descriptionClassName="w-full"
            footerLineClassName="w-40"
            footerPills={["w-20", "w-24"]}
          />
        {/each}
      {:else if filteredDatabases.length === 0}
        <div class="col-span-full">
          <EmptyState class="py-16">
            <DatabaseIcon class="size-10 text-muted-foreground" />
            <h2 class="text-xl font-semibold">
              {query || statusFilter !== "all" ? "No matching databases" : "No databases found"}
            </h2>
            <p class="max-w-md text-sm text-muted-foreground">
              {query || statusFilter !== "all"
                ? "Try a different search term or clear the filters."
                : "Provision your first PostgreSQL instance to get started."}
            </p>
            <Button size="sm" onclick={() => (showCreate = true)}>
              <Plus class="size-4" />
              Create your first database
            </Button>
          </EmptyState>
        </div>
      {:else}
        {#each filteredDatabases as db}
          <a class="block" href={`/databases/${encodeURIComponent(db.name)}`}>
            <Card size="sm">
              <CardHeader>
                <div class="flex items-start gap-4">
                  <div class="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <DatabaseIcon class="size-5" />
                  </div>
                  <div class="flex min-w-0 flex-1 flex-col gap-2">
                    <div class="flex min-w-0 items-center gap-2">
                      <CardTitle class="truncate text-base">{db.name}</CardTitle>
                    </div>
                    <CardDescription>PostgreSQL {db.postgres_version}</CardDescription>
                  </div>
                  <Badge variant="outline" class={`shrink-0 gap-1.5 uppercase ${getStatusBadgeClass(db.status)}`}>
                    <Radio class="size-3" />
                    {db.status}
                  </Badge>
                </div>
              </CardHeader>
              <CardContent class="flex flex-col gap-4">
                <div class="flex flex-col gap-2 text-xs text-muted-foreground">
                  <div class="flex flex-wrap items-center justify-between gap-3">
                    <span class="inline-flex items-center gap-1.5">
                      <Calendar class="size-4" />
                      Created {formatDate(db.created_at)}
                    </span>
                    <span class="inline-flex items-center gap-1.5">
                      <Calendar class="size-4" />
                      Updated {formatDate(db.updated_at)}
                    </span>
                  </div>
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="gap-1.5">
                      <Cpu class="size-3" />
                      <span>{db.vcpus} vCPU</span>
                    </Badge>
                    <Badge variant="outline" class="gap-1.5">
                      <HardDrive class="size-3" />
                      <span>{Math.max(1, Math.round(db.disk_mib / 1024))} GB</span>
                    </Badge>
                    <Badge variant="outline" class="gap-1.5">
                      <DatabaseIcon class="size-3" />
                      <span>Neon</span>
                    </Badge>
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
    <CreateDatabaseModal bind:open={showCreate} />
  {/if}
</DashboardLayout>
