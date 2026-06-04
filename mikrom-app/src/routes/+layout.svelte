<script lang="ts">
  import "../app.css";
  import { Boxes, LoaderCircle } from "lucide-svelte";
  import { initTheme } from "$lib/theme";
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { Toaster } from "$lib/components/ui/sonner";
  import { initWorkspaceSSE, closeWorkspaceSSE } from "$lib/stores/workspace";
  import { clearVms, initVmsSSE, refreshVms, stopVmsSSE } from "$lib/stores/vms";
  import { clearApps, refreshApps } from "$lib/stores/apps";
  import { clearVolumes, clearSnapshots } from "$lib/stores/volumes";
  import { clearSecurityRules } from "$lib/stores/networking";
  import { clearDatabases, refreshDatabases } from "$lib/stores/databases";
  import { activeProjectStore, endProjectSwitch, projectSwitchingStore, refreshProjects } from "$lib/stores/projects";

  initTheme();

  onMount(() => {
    initWorkspaceSSE();
    initVmsSSE();

    const handleProjectChange = async () => {
      clearApps();
      clearVms();
      clearVolumes();
      clearSnapshots();
      clearSecurityRules();
      clearDatabases();
      closeWorkspaceSSE();
      stopVmsSSE();
      try {
        await Promise.all([refreshProjects(), refreshApps(), refreshVms(), refreshDatabases()]);
        initWorkspaceSSE();
        initVmsSSE({ seed: false });
      } finally {
        endProjectSwitch();
      }
    };

    window.addEventListener("mikrom-project-change", handleProjectChange);

    return () => {
      window.removeEventListener("mikrom-project-change", handleProjectChange);
    };
  });
</script>

<svelte:head>
  <title>Mikrom</title>
  <meta name="description" content="Micromobility management platform" />
</svelte:head>

<slot />
{#if $projectSwitchingStore}
  <div class="pointer-events-auto fixed inset-0 z-50 overflow-hidden" transition:fade={{ duration: 140 }}>
    <div class="absolute inset-0 bg-background/80 backdrop-blur-sm"></div>
    <div class="absolute -left-24 top-12 size-72 rounded-full bg-status-info/10 blur-3xl"></div>
    <div class="absolute -right-20 bottom-10 size-80 rounded-full bg-status-warning/10 blur-3xl"></div>
    <div class="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.04),transparent_34%),linear-gradient(to_bottom,transparent,rgba(0,0,0,0.04))]"></div>

    <div class="relative flex h-full items-center justify-center px-4">
      <div
        class="w-full max-w-md rounded-2xl border border-border/70 bg-card/95 p-6 shadow-2xl shadow-black/10"
        transition:scale={{ duration: 180, start: 0.98, opacity: 0.2 }}
      >
        <div class="flex items-start gap-4">
          <div class="flex size-12 shrink-0 items-center justify-center rounded-xl border border-border/70 bg-background text-foreground shadow-sm">
            <Boxes class="size-5" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="text-[11px] font-medium uppercase tracking-[0.24em] text-muted-foreground">Project context</p>
            <h2 class="mt-1 truncate text-lg font-semibold">
              {$activeProjectStore?.name || $activeProjectStore?.tenant_id || "Loading project"}
            </h2>
            <p class="mt-1 text-sm text-muted-foreground">Switching to the new project and reloading its data.</p>
          </div>
          <LoaderCircle class="mt-1 size-5 animate-spin text-muted-foreground" />
        </div>

        <div class="mt-5 rounded-2xl border border-border/70 bg-card/95 p-5 shadow-sm">
          <div class="flex items-center gap-3">
            <div class="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground">
              <LoaderCircle class="size-4 animate-spin text-muted-foreground" />
            </div>
            <div class="min-w-0">
              <p class="text-sm font-medium">Refreshing dashboard</p>
              <p class="text-xs text-muted-foreground">Apps, VMs, storage and networking are being rehydrated.</p>
            </div>
          </div>
          <div class="mt-4 h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div class="h-full w-2/3 rounded-full bg-gradient-to-r from-foreground/40 via-foreground to-foreground/40 animate-pulse"></div>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
<Toaster position="bottom-right" />
