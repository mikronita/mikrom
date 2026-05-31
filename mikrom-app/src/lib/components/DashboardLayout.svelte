<script lang="ts">
  import { page } from "$app/stores";
  import { Bell, PanelLeft } from "lucide-svelte";
  import AuthGuard from "$lib/components/AuthGuard.svelte";
  import ProjectSwitcher from "$lib/components/ProjectSwitcher.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import { useProfileBootstrap } from "$lib/stores/profile";
  import { useProjectBootstrap } from "$lib/stores/projects";

  useProfileBootstrap();
  useProjectBootstrap();

  const segmentName = (segment: string) => decodeURIComponent(segment).replace(/^\w/, (c) => c.toUpperCase());
  const breadcrumbHref = (index: number) => `/${pathParts.slice(0, index + 1).map(encodeURIComponent).join("/")}`;

  let pathParts: string[] = [];
  let sidebarCollapsed = false;
  $: pathParts = $page.url.pathname.split("/").filter(Boolean);

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    document.cookie = `sidebar_state=${sidebarCollapsed}; path=/; max-age=${60 * 60 * 24 * 7}`;
  }
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
          on:click={toggleSidebar}
        >
          <PanelLeft class="size-4" />
        </button>
        <div class="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-sm text-muted-foreground">
          <a href="/" class="hidden font-medium text-foreground md:block">Home</a>
          {#if pathParts.length}
            {#each pathParts as part, index}
              <span class="hidden md:block">/</span>
              {#if index === pathParts.length - 1}
                <span class="max-w-[140px] truncate font-medium text-foreground sm:max-w-none">
                  {segmentName(part)}
                </span>
              {:else}
                <a href={breadcrumbHref(index)} class="hidden font-medium text-foreground hover:underline sm:block">
                  {segmentName(part)}
                </a>
              {/if}
            {/each}
          {/if}
        </div>
        <div class="hidden lg:block">
          <ProjectSwitcher compact className="min-w-[220px]" />
        </div>
        <button class="flex size-9 items-center justify-center rounded-md hover:bg-muted" aria-label="Notifications" type="button">
          <Bell class="size-4" />
        </button>
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
