<script lang="ts">
  import { Database as DatabaseIcon, Plus, Calendar, Cpu, HardDrive, Radio } from "lucide-svelte";
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
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import CreateDatabaseModal from "$lib/components/CreateDatabaseModal.svelte";
  import { formatDate } from "$lib/utils";
  import { matchesSearch } from "$lib/search";
  import { databasesStore, databasesLoading } from "$lib/stores/databases";

  let showCreate = false;
  let query = "";
  let statusFilter: "all" | "running" | "provisioning" | "deleting" | "stopped" = "all";

  function getStatusBadgeClass(status: string) {
    switch (status) {
      case "Running":
        return "border-transparent bg-status-online/10 text-status-online";
      case "Provisioning":
        return "border-transparent bg-status-info/10 text-status-info";
      case "Deleting":
        return "border-transparent bg-status-offline/10 text-status-offline";
      default:
        return "border-transparent bg-muted/70 text-muted-foreground";
    }
  }

  $: filteredDatabases = [...$databasesStore]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .filter((db) => {
      const matchesStatus = statusFilter === "all" || db.status.toLowerCase() === statusFilter;
      return matchesStatus && matchesSearch([db.name, db.version, db.status], query);
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
        <p class="max-w-2xl text-sm text-muted-foreground">Managed PostgreSQL instances for your applications.</p>
      </div>
      <Button onclick={() => (showCreate = true)}>
        <Plus class="size-4" />
        New Database
      </Button>
    </div>

    <Card size="sm" class="overflow-hidden">
      <CardContent class="flex flex-col gap-4">
        <div class="min-w-0 flex-1">
          <Input bind:value={query} placeholder="Search by database name, version or status" />
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
              statusFilter === "running"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "running")}
          >
            Running
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "provisioning"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "provisioning")}
          >
            Provisioning
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "deleting"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "deleting")}
          >
            Deleting
          </button>
          <button
            class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 whitespace-nowrap ${
              statusFilter === "stopped"
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => (statusFilter = "stopped")}
          >
            Stopped
          </button>
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
                    <CardDescription>PostgreSQL {db.version}</CardDescription>
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
                      Updated {formatDate(db.updated_at || db.created_at)}
                    </span>
                  </div>
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="gap-1.5">
                      <Cpu class="size-3" />
                      <span>{db.vcpus} vCPU</span>
                    </Badge>
                    <Badge variant="outline" class="gap-1.5">
                      <HardDrive class="size-3" />
                      <span>{db.memory_mib / 1024} GB</span>
                    </Badge>
                    <Badge variant="outline" class="gap-1.5">
                      <DatabaseIcon class="size-3" />
                      <span>{db.storage_gb} GB</span>
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
