<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import AuthGuard from "$lib/components/AuthGuard.svelte";
  import NotificationsMenu from "$lib/components/NotificationsMenu.svelte";
  import ProjectSwitcher from "$lib/components/ProjectSwitcher.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import { buildBreadcrumbs } from "$lib/domain/navigation";
  import { useProfileBootstrap } from "$lib/stores/profile";
  import { useProjectBootstrap } from "$lib/stores/projects";

  useProfileBootstrap();
  useProjectBootstrap();

  const SIDEBAR_COOKIE_NAME = "sidebar_state";
  const SIDEBAR_COOKIE_MAX_AGE = 60 * 60 * 24 * 7;

  let sidebarCollapsed = $page.data.sidebarCollapsed ?? false;

  function persistSidebarState(nextCollapsed: boolean) {
    sidebarCollapsed = nextCollapsed;
    document.cookie = `${SIDEBAR_COOKIE_NAME}=${nextCollapsed}; path=/; max-age=${SIDEBAR_COOKIE_MAX_AGE}`;
    window.localStorage.setItem(SIDEBAR_COOKIE_NAME, String(nextCollapsed));
  }

  onMount(() => {
    const persisted = window.localStorage.getItem(SIDEBAR_COOKIE_NAME);
    if (persisted === "true" || persisted === "false") {
      sidebarCollapsed = persisted === "true";
    }
  });
</script>

<AuthGuard>
  <div class="flex min-h-screen bg-background">
    <Sidebar bind:collapsed={sidebarCollapsed} />
    <div class="flex min-w-0 flex-1 flex-col">
      <header class="sticky top-0 z-10 flex flex-col gap-2 border-b border-border bg-background/95 px-3 py-2 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:hidden">
        <div class="flex items-center gap-2">
          <a href="/" class="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-card text-foreground">
            <span class="text-sm font-semibold">M</span>
          </a>
          <div class="min-w-0 flex-1"></div>
          <NotificationsMenu />
          <ThemeToggle />
        </div>
        <ProjectSwitcher compact className="w-full" />
      </header>

      <header class="sticky top-0 z-10 hidden h-14 shrink-0 items-center gap-3 border-b border-border bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:flex md:px-4">
        <button
          type="button"
          class="flex size-9 items-center justify-center rounded-md hover:bg-muted"
          aria-label="Toggle Sidebar"
          title="Toggle Sidebar"
          onclick={() => persistSidebarState(!sidebarCollapsed)}
        >
          <PanelLeft class="size-4" />
        </button>
        <div class="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-sm text-muted-foreground">
          <a href="/" class="hidden font-medium text-foreground md:block">Home</a>
          {#if buildBreadcrumbs($page.url.pathname).length}
            {#each buildBreadcrumbs($page.url.pathname) as crumb}
              <span class="hidden md:block">/</span>
              {#if crumb.current}
                <span class="max-w-[140px] truncate font-medium text-foreground sm:max-w-none">
                  {crumb.label}
                </span>
              {:else}
                <a href={crumb.href} class="hidden font-medium text-foreground hover:underline sm:block">
                  {crumb.label}
                </a>
              {/if}
            {/each}
          {/if}
        </div>
        <div class="hidden lg:block">
          <ProjectSwitcher compact className="min-w-[220px]" />
        </div>
        <NotificationsMenu />
        <ThemeToggle />
      </header>

      <main class="flex-1 p-3 md:p-4">
        <div class="mx-auto w-full max-w-7xl">
          <slot />
        </div>
      </main>
    </div>
  </div>
</AuthGuard>
