<script lang="ts">
  import { goto } from "$app/navigation";
  import { ChevronsUpDown, Check, Folder } from "lucide-svelte";
  import {
    beginProjectSwitch,
    activeProjectStore,
    activeProjectSlugStore,
    projectsLoading,
    projectsStore,
    setActiveProjectSlug,
  } from "$lib/stores/projects";
  import { cn } from "$lib/utils";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";

  let { collapsed = false, compact = false, className = "" } = $props();

  function switchProject(slug: string) {
    if (slug === $activeProjectSlugStore) return;
    beginProjectSwitch();
    setActiveProjectSlug(slug);
    void goto("/", {
      replaceState: true,
      invalidateAll: true,
      noScroll: true,
      keepFocus: true,
    });
  }

  function projectLabel() {
    return $activeProjectStore?.name || $activeProjectStore?.tenant_id || $activeProjectSlugStore || "Select project";
  }

  function projectSlug() {
    return $activeProjectStore?.tenant_id || $activeProjectSlugStore || "";
  }
</script>

<div class={cn(compact ? "p-0" : "px-2 pt-2")}>
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <button
          type="button"
          {...props}
          class={cn(
            "group flex items-center rounded-md border border-border/80 bg-gradient-to-br from-background to-muted/40 text-left text-sm shadow-sm outline-none transition-[transform,box-shadow,background-color] hover:-translate-y-0.5 hover:shadow-md hover:bg-muted/70 focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-muted",
            compact ? "h-9 w-auto gap-2 px-3" : "h-12 w-full px-2",
            collapsed ? "justify-center" : "gap-3",
            className
          )}
          aria-label="Project selector"
        >
          <div class="flex size-8 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background/80 text-foreground shadow-[0_1px_0_rgba(255,255,255,0.05)_inset]">
            <Folder class="size-4 transition-transform duration-200 group-data-[state=open]:scale-105" />
          </div>
          {#if !collapsed || compact}
            <div class="grid flex-1 text-left leading-tight">
              {#if !compact}
                <span class="truncate text-[11px] uppercase tracking-[0.22em] text-muted-foreground">Project</span>
              {/if}
              <span class="truncate font-medium">{projectLabel()}</span>
              {#if projectSlug() && !compact}
                <span class="truncate text-xs text-muted-foreground">{projectSlug()}</span>
              {/if}
            </div>
            <ChevronsUpDown class={cn("shrink-0 text-muted-foreground transition-transform duration-200 group-data-[state=open]:rotate-180", compact ? "ml-0 size-4" : "ml-auto size-4")} />
          {/if}
        </button>
      {/snippet}
    </DropdownMenu.Trigger>

    <DropdownMenu.Content
      class={cn(
        "w-[19rem] max-w-[calc(100vw-1rem)] overflow-hidden rounded-xl border border-border/70 bg-popover p-0 shadow-2xl",
        compact ? "max-h-[24rem]" : "max-h-[26rem]"
      )}
      side={compact ? "bottom" : "right"}
      align={compact ? "end" : "start"}
      sideOffset={8}
    >
      <div class="border-b border-border/70 bg-muted/40 px-4 py-3">
        <div class="flex items-center gap-3">
          <div class="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground">
            <Folder class="size-4" />
          </div>
          <div class="min-w-0 flex-1">
            <p class="text-[11px] font-medium uppercase tracking-[0.22em] text-muted-foreground">Active project</p>
            <p class="truncate text-sm font-semibold">{projectLabel()}</p>
            {#if projectSlug()}
              <p class="truncate text-xs text-muted-foreground">{projectSlug()}</p>
            {/if}
          </div>
        </div>
      </div>

      <div class="max-h-[16rem] overflow-y-auto p-1">
        {#if $projectsLoading && $projectsStore.length === 0}
          <DropdownMenu.Item disabled class="rounded-lg px-3 py-2">Loading projects...</DropdownMenu.Item>
        {:else if $projectsStore.length === 0}
          <DropdownMenu.Item disabled class="rounded-lg px-3 py-2">No projects available</DropdownMenu.Item>
        {:else}
          {#each $projectsStore as project}
            <DropdownMenu.Item
              onSelect={() => switchProject(project.tenant_id)}
              class="mb-1 rounded-lg px-3 py-2.5"
            >
              <div class="flex w-full items-center justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="truncate text-sm font-medium">{project.name}</div>
                  <div class="truncate text-xs text-muted-foreground">{project.tenant_id}</div>
                </div>
                <div
                  class={cn(
                    "flex size-5 shrink-0 items-center justify-center rounded-full border transition-colors",
                    project.tenant_id === $activeProjectSlugStore
                      ? "border-foreground/30 bg-foreground text-background"
                      : "border-border bg-background text-transparent"
                  )}
                >
                  <Check class="size-3.5" />
                </div>
              </div>
            </DropdownMenu.Item>
          {/each}
        {/if}
      </div>

      <div class="border-t border-border/70 p-2">
        <DropdownMenu.Item class="rounded-lg px-3 py-2">
          {#snippet child({ props })}
            <a href="/projects" {...props} class="flex w-full items-center gap-2">
              <Folder class="size-4 text-muted-foreground" />
              <span class="text-sm font-medium">Manage projects</span>
            </a>
          {/snippet}
        </DropdownMenu.Item>
      </div>
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</div>
