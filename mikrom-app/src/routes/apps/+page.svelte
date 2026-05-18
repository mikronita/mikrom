<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Boxes, Calendar, Cpu, FolderPlus, HardDrive, Plus, Radio } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import CardHeader from "$lib/components/CardHeader.svelte";
  import CardTitle from "$lib/components/CardTitle.svelte";
  import CardDescription from "$lib/components/CardDescription.svelte";
  import CardContent from "$lib/components/CardContent.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Button from "$lib/components/Button.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import CreateAppModal from "$lib/components/CreateAppModal.svelte";
  import { formatDate } from "$lib/utils";
  import { getToken } from "$lib/auth";
  import { type AppInfo } from "$lib/api";
  import { getCurrentVms, subscribeVms } from "$lib/stores/vms";
  import { appsStore, appsLoading, appsError, refreshApps } from "$lib/stores/apps";
  import { toast } from "$lib/toast";

  let vms = getCurrentVms();
  let showCreate = false;

  const unsubscribe = subscribeVms((next) => {
    vms = next;
  });
  onDestroy(() => unsubscribe());

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
  });

  $: if ($appsError) {
    toast.error($appsError);
  }

  const vmsMap = () => new Map(vms.map((vm) => [vm.app_id || vm.app_name, vm]));
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
        <p class="max-w-2xl text-sm text-muted-foreground">Manage your Git-based projects and deployments.</p>
      </div>
      <Button onclick={() => (showCreate = true)}>
        <Plus class="size-4" />
        New Application
      </Button>
    </div>

    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
    {#if $appsLoading && $appsStore.length === 0}
      {#each Array.from({ length: 6 }) as _}
        <Card class="overflow-hidden">
          <CardHeader>
            <div class="flex items-start gap-4">
              <div class="flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground">
                <div class="size-5 animate-pulse rounded bg-muted"></div>
              </div>
              <div class="flex flex-1 flex-col gap-2">
                <div class="h-5 w-32 animate-pulse rounded bg-muted"></div>
                <div class="h-4 w-full animate-pulse rounded bg-muted"></div>
              </div>
              <div class="h-6 w-14 animate-pulse rounded-full bg-muted"></div>
            </div>
          </CardHeader>
          <CardContent class="flex flex-col gap-3">
            <div class="h-4 w-40 animate-pulse rounded bg-muted"></div>
            <div class="flex gap-2">
              <div class="h-6 w-20 animate-pulse rounded-full bg-muted"></div>
              <div class="h-6 w-24 animate-pulse rounded-full bg-muted"></div>
            </div>
          </CardContent>
        </Card>
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
          {@const appVm = vmsMap().get(app.id) || vmsMap().get(app.name)}
          {@const hasActiveDeployment = !!app.active_deployment_id}
          {@const isRunningVm = appVm?.status?.toLowerCase() === "running"}
          {@const isActive = hasActiveDeployment || isRunningVm}
          <a class="block" href={`/apps/${encodeURIComponent(app.name)}`}>
            <Card class="h-full overflow-hidden transition-colors hover:bg-muted/30">
              <CardHeader>
                <div class="flex items-start gap-4">
                  <div class="flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground">
                    <Boxes class="size-5" />
                  </div>
                  <div class="flex min-w-0 flex-1 flex-col gap-2">
                    <div class="flex min-w-0 items-center gap-2">
                      <CardTitle class="truncate text-base">{app.name}</CardTitle>
                    </div>
                    <CardDescription>Application workspace</CardDescription>
                  </div>
                  <Badge variant={isActive ? "success" : "secondary"} className="shrink-0 gap-1.5 uppercase">
                    <Radio class="size-3" />
                    {isActive ? "Live" : "Idle"}
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
                    {#if isRunningVm && appVm}
                      <Badge variant="secondary" className="gap-1.5">
                        <Cpu class="size-3" />
                        <span>{appVm.vcpus || 1} vCPU</span>
                      </Badge>
                      <Badge variant="secondary" className="gap-1.5">
                        <HardDrive class="size-3" />
                        <span>{appVm.memory_mib || 128} MB</span>
                      </Badge>
                    {:else if hasActiveDeployment}
                      <Badge variant="secondary">Active deployment</Badge>
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
